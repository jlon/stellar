//! 联网查证工具（只读、白名单）：
//! - `search_web`：DuckDuckGo lite 关键词搜索，返回标题+URL 列表（免 API key）；
//! - `fetch_doc`：抓取白名单域（StarRocks / Doris 官方文档等）的 https 页面正文。
//!
//! 只做 https GET，重定向逐跳校验白名单；响应做 HTML 标签/脚本剥离 + 截断，
//! 防止任意网页内容撑爆 LLM 上下文。白名单精确匹配域名，天然排除内网/IP 直连。

use async_trait::async_trait;
use regex::Regex;
use reqwest::redirect::Policy;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::db::AppDb;
use crate::services::ops_agent::tool::{AgentTool, ToolContext, truncate};

/// 允许抓取的文档站（StarRocks / Doris 官方文档与官方仓库）。
const ALLOWED_HOSTS: &[&str] = &[
    "docs.starrocks.io",
    "cn.starrocks.io",
    "doris.apache.org",
    "github.com",
    "raw.githubusercontent.com",
];

const HTTP_TIMEOUT_SECS: u64 = 10;
/// 抓取页面最多跟随的重定向次数（每跳重新校验白名单）。
const MAX_REDIRECTS: usize = 3;
const SEARCH_RESULT_LIMIT: usize = 8;
const FETCH_BODY_MAX_CHARS: usize = 8000;
const FETCH_TEXT_MAX_CHARS: usize = 6000;

/// 白名单 + SSRF 防护的 GET 客户端（手动跟随重定向，每跳重新校验）。
struct GuardedGet {
    http: reqwest::Client,
}

impl GuardedGet {
    fn new() -> Result<Self, String> {
        let http = reqwest::Client::builder()
            // 重定向手动处理：reqwest 的 Policy 拿不到完整跳转链，逐跳校验必须在循环里做
            .redirect(Policy::none())
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) stellar-ops-agent/1.0")
            .build()
            .map_err(|e| format!("HTTP 客户端初始化失败: {}", e))?;
        Ok(Self { http })
    }

    fn check_url(&self, raw: &str) -> Result<reqwest::Url, String> {
        let parsed = reqwest::Url::parse(raw).map_err(|_| format!("URL 无法解析: {}", raw))?;
        if parsed.scheme() != "https" {
            return Err("仅允许 https URL".into());
        }
        // 显式拒绝非默认端口（reqwest::Url::host() 不含端口，需单独校验）
        if parsed.port().is_some() {
            return Err("不允许自定义端口".into());
        }
        let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
        // 精确匹配白名单：IP 直连、内网地址、带端口变体、仿冒域全部不命中
        if !ALLOWED_HOSTS.iter().any(|h| host == *h) {
            return Err(format!(
                "域名 {} 不在允许列表内（仅限官方文档站：{}）",
                host,
                ALLOWED_HOSTS.join(", ")
            ));
        }
        Ok(parsed)
    }

    /// GET 并手动跟随重定向（每跳重新校验白名单）。
    async fn get(&self, url: &str) -> Result<String, String> {
        let mut current = self.check_url(url)?;
        for _ in 0..=MAX_REDIRECTS {
            let resp = self
                .http
                .get(current.clone())
                .send()
                .await
                .map_err(|e| format!("请求失败 {}: {}（检查服务器外网连通性）", current, e))?;
            let status = resp.status();
            if status.is_redirection() {
                let loc = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| format!("重定向缺少 Location: {}", current))?;
                let next = resp
                    .url()
                    .join(loc)
                    .map_err(|e| format!("重定向地址无效: {}", e))?
                    .to_string();
                current = self.check_url(&next)?;
                continue;
            }
            if !status.is_success() {
                return Err(format!(
                    "HTTP {}（{}），可换一个 URL 或改用 search_web 查找",
                    status.as_u16(),
                    current
                ));
            }
            return resp
                .text()
                .await
                .map_err(|e| format!("读取响应失败: {}", e));
        }
        Err(format!("重定向超过 {} 次", MAX_REDIRECTS))
    }
}

/// 剥离 HTML：去掉 script/style/nav 等噪音块与全部标签，保留正文文本。
fn strip_html(html: &str) -> String {
    // regex crate 不支持反向引用，按标签名逐个匹配开闭对
    let mut s = html.to_string();
    for tag in ["script", "style", "noscript", "svg", "nav", "footer", "header", "aside"] {
        let re = Regex::new(&format!(r"(?is)<{0}[^>]*>.*?</{0}>", tag)).unwrap();
        s = re.replace_all(&s, "").into_owned();
    }
    let comments = Regex::new(r"(?s)<!--.*?-->").unwrap();
    let tags = Regex::new(r"(?s)<[^>]*>").unwrap();
    let spaces = Regex::new(r"[ \t]+").unwrap();
    let blank_lines = Regex::new(r"\n{3,}").unwrap();

    let s = comments.replace_all(&s, "");
    let s = tags.replace_all(&s, " ");
    // 文档站高频实体，其余交给 LLM 容错
    let s = s
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let s = spaces.replace_all(&s, " ");
    let s = blank_lines.replace_all(&s, "\n\n");
    s.trim().to_string()
}

/// 最小 HTML 实体反转义（搜索结果标题/URL 里常见）。
fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
}

/// DuckDuckGo lite 搜索：POST q → 解析结果链接（标题+URL 列表）。
async fn ddg_search(http: &reqwest::Client, query: &str) -> Result<String, String> {
    let resp = http
        .post("https://lite.duckduckgo.com/lite/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("q={}", urlencoding::encode(query)))
        .send()
        .await
        .map_err(|e| format!("搜索请求失败: {}（网络不可达或被防火墙拦截）", e))?;
    if !resp.status().is_success() {
        return Err(format!(
            "搜索返回 HTTP {}（可稍后重试，或改用 fetch_doc 直取已知文档 URL）",
            resp.status().as_u16()
        ));
    }
    let html = resp
        .text()
        .await
        .map_err(|e| format!("读取搜索结果失败: {}", e))?;

    // lite 版结果结构：<a rel="nofollow" href="URL">TITLE</a>
    let link_re = Regex::new(r#"<a[^>]+href="(https?://[^"]+)"[^>]*>(.*?)</a>"#).unwrap();
    let mut lines: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cap in link_re.captures_iter(&html) {
        let url = html_unescape(&cap[1]);
        let title = strip_html(&cap[2]);
        if title.is_empty() || url.contains("duckduckgo.com") || !seen.insert(url.clone()) {
            continue;
        }
        lines.push(format!("- {}：{}", title, url));
        if lines.len() >= SEARCH_RESULT_LIMIT {
            break;
        }
    }
    if lines.is_empty() {
        return Ok(
            "未搜索到相关结果，建议换关键词（可用英文技术术语）或用 fetch_doc 直取已知文档 URL。"
                .into(),
        );
    }
    Ok(format!("搜索「{}」结果：\n{}", query, lines.join("\n")))
}

// ---- 两个工具：无需数据库，但与其它工具同构挂在 ToolContext<DB> 上，ctx 仅用于统一构造 ----

pub struct SearchWebTool<DB: AppDb> {
    #[allow(dead_code)]
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

pub struct FetchDocTool<DB: AppDb> {
    #[allow(dead_code)]
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

#[async_trait]
impl<DB: AppDb> AgentTool for SearchWebTool<DB> {
    fn name(&self) -> &'static str {
        "search_web"
    }

    fn description(&self) -> &'static str {
        "联网搜索（DuckDuckGo）：当对 StarRocks/Doris 某个版本的行为、语法、参数取值不确定时，\
         先用本工具找到官方文档链接，再用 fetch_doc 抓取正文。仅返回标题与 URL 列表，不返回正文。\
         建议用英文技术关键词（如 starrocks colocate join）。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "搜索关键词（建议英文技术术语 + 引擎名）" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .ok_or("缺少 query 参数")?;
        let client = GuardedGet::new()?;
        let result = ddg_search(&client.http, query).await?;
        Ok(truncate(&result, FETCH_TEXT_MAX_CHARS))
    }
}

#[async_trait]
impl<DB: AppDb> AgentTool for FetchDocTool<DB> {
    fn name(&self) -> &'static str {
        "fetch_doc"
    }

    fn description(&self) -> &'static str {
        "抓取官方文档页面正文（只读 GET，仅限白名单域：docs.starrocks.io、cn.starrocks.io、\
         doris.apache.org、github.com、raw.githubusercontent.com）。先用 search_web 找到 URL 再抓取；\
         返回剥离 HTML 后的纯文本（超长自动截断）。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "白名单域内的 https 文档 URL" }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let url = args
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .ok_or("缺少 url 参数")?;
        let client = GuardedGet::new()?;
        let html = client.get(url).await?;
        let text = strip_html(&html);
        if text.is_empty() {
            return Err("页面无可提取正文（可能是纯 JS 渲染页），建议换 raw.githubusercontent.com 上的 Markdown 源文件".into());
        }
        Ok(truncate(&text, FETCH_TEXT_MAX_CHARS))
    }
}

/// 编译期常量自检：正文上限必须小于原始抓取上限（先截 HTML 再剥皮，防止内存放大）。
const _: () = assert!(FETCH_TEXT_MAX_CHARS <= FETCH_BODY_MAX_CHARS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_removes_noise_and_keeps_body() {
        let html = r#"<html><head><style>.x{color:red}</style><script>evil()</script></head>
            <body><nav>menu</nav><h1>Colocate Join</h1><p>A &amp; B &lt;tag&gt;</p></body></html>"#;
        let text = strip_html(html);
        assert!(text.contains("Colocate Join"));
        assert!(text.contains("A & B <tag>"));
        assert!(!text.contains("evil"));
        assert!(!text.contains("color:red"));
        assert!(!text.contains("menu"));
    }

    #[test]
    fn check_url_enforces_https_and_whitelist() {
        let g = GuardedGet::new().unwrap();
        // 白名单命中
        assert!(g.check_url("https://docs.starrocks.io/sql/SELECT").is_ok());
        // http 拒绝
        assert!(g.check_url("http://docs.starrocks.io/x").is_err());
        // 外域 / 仿冒域拒绝
        assert!(g.check_url("https://evil.com/doc").is_err());
        assert!(
            g.check_url("https://docs.starrocks.io.evil.com/doc")
                .is_err()
        );
        // 内网 / IP / 带端口变体拒绝
        assert!(g.check_url("https://127.0.0.1:8080/api").is_err());
        assert!(g.check_url("https://docs.starrocks.io:8443/x").is_err());
    }

    #[test]
    fn html_unescape_restores_entities() {
        assert_eq!(html_unescape("a&amp;b&#39;c"), "a&b'c");
    }
}
