//! OpenAI-compatible chat completions client (non-streaming + streaming).
//!
//! Shared by ops agent and (future) ask-data. Provider settings come from the
//! active `llm_providers` row (id/api_base/model_name/api_key_encrypted).
//! Streaming follows OpenAI SSE (`data:` line per chunk, `[DONE]` terminator),
//! mirroring Flink AiChatStreamHandler + pi-from-scratch token streaming.

use serde::Deserialize;
use serde_json::{Value, json};
use tokio_stream::StreamExt;

use super::types::{ChatCompletion, ChatMessage, ToolCallDelta};
use crate::services::llm::LLMProvider;

/// 流式推送的内容增量（打字机文本）。回合语义由调用方在回合结束时判定
/// （有 tool_calls 即思考段进 reasoning step，否则为最终答案）——
/// 与 Flink DefaultJobDiagnosisAgent 的 publishIfMissing 思路一致，
/// 避免"首个 chunk 预判回合类型"带来的误判。
pub type StreamDelta = String;

#[derive(Deserialize)]
struct WireResponse {
    choices: Vec<WireChoice>,
    #[serde(default)]
    usage: WireUsage,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<WireToolCall>,
}

#[derive(Deserialize)]
struct WireToolCall {
    id: String,
    function: WireFunction,
}

#[derive(Deserialize)]
struct WireFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Deserialize, Default)]
struct WireUsage {
    #[serde(default)]
    total_tokens: i64,
}

// ---- streaming wire types ----

#[derive(Deserialize)]
struct WireChunk {
    choices: Vec<WireChunkChoice>,
}

#[derive(Deserialize)]
struct WireChunkChoice {
    #[serde(default)]
    delta: WireChunkDelta,
}

#[derive(Deserialize, Default)]
struct WireChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<WireChunkToolCall>,
}

#[derive(Deserialize)]
struct WireChunkToolCall {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: WireChunkFunction,
}

#[derive(Deserialize, Default)]
struct WireChunkFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// 按 index 累积的流式工具调用。
struct AccToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Minimal OpenAI-compatible chat client.
pub struct ChatClient {
    http: reqwest::Client,
    provider: LLMProvider,
}

impl ChatClient {
    pub fn new(provider: LLMProvider) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                // 内部网关对 keep-alive 复用连接的流式响应存在状态污染
                // （同一连接上非流式请求后，流式请求的工具调用会被静默丢弃）——
                // 禁用空闲连接复用，每次请求新建连接，规避网关缺陷。
                .pool_max_idle_per_host(0)
                .build()
                .expect("failed to build reqwest client"),
            provider,
        }
    }

    /// Whether a usable provider is available.
    pub fn is_available(&self) -> bool {
        self.provider.enabled && self.provider.api_key_encrypted.is_some()
    }

    /// One chat completion round. `tools` is the full OpenAI tools array (or `null`).
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: Option<&Value>,
    ) -> Result<ChatCompletion, String> {
        self.chat_with_max_tokens(messages, tools, self.provider.max_tokens.max(1) as u32)
            .await
    }

    /// One completion with a caller-selected output budget (used by compaction).
    pub async fn chat_with_max_tokens(
        &self,
        messages: &[ChatMessage],
        tools: Option<&Value>,
        max_tokens: u32,
    ) -> Result<ChatCompletion, String> {
        let api_key = self
            .provider
            .api_key_encrypted
            .as_deref()
            .ok_or_else(|| "LLM provider has no API key".to_string())?;

        let mut body = json!({
            "model": self.provider.model_name,
            "messages": messages,
            "temperature": self.provider.temperature,
            "max_tokens": max_tokens,
        });
        if let Some(tools) = tools {
            body["tools"] = tools.clone();
            body["tool_choice"] = json!("auto");
        }

        let url = format!("{}/chat/completions", self.provider.api_base.trim_end_matches('/'));

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(self.provider.timeout_seconds.max(30) as u64))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("LLM request failed: {}", e))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("LLM read body failed: {}", e))?;
        if !status.is_success() {
            return Err(format!("LLM API error {}: {}", status, truncate(&text, 500)));
        }

        let wire: WireResponse =
            serde_json::from_str(&text).map_err(|e| format!("LLM response parse failed: {}", e))?;

        let msg = wire
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| "LLM response has no choices".to_string())?;

        let tool_calls = msg
            .message
            .tool_calls
            .into_iter()
            // 部分 OpenAI 兼容网关会返回空 function.name（Flink 已在流式 delta 上踩坑：
            // hotfix 69ce7741c23）。空名工具调用无法路由，直接丢弃，避免 agent 死循环。
            .filter(|tc| !tc.function.name.trim().is_empty())
            .map(|tc| ToolCallDelta {
                id: tc.id,
                name: tc.function.name,
                arguments: tc.function.arguments,
            })
            .collect();

        Ok(ChatCompletion {
            content: msg.message.content,
            tool_calls,
            usage_tokens: wire.usage.total_tokens,
            finish_reason: msg.finish_reason,
        })
    }

    /// Stream one chat completion: every content delta is passed to `on_delta`
    /// synchronously as it arrives (typewriter effect); the accumulated
    /// `ChatCompletion` is returned when the stream ends. Tool-call chunks are
    /// accumulated internally by index; empty `function.name` deltas are
    /// tolerated (Flink hotfix 69ce7741c23) and empty-name calls dropped.
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: Option<&Value>,
        mut on_delta: impl FnMut(String),
    ) -> Result<ChatCompletion, String> {
        let api_key = self
            .provider
            .api_key_encrypted
            .as_deref()
            .ok_or_else(|| "LLM provider has no API key".to_string())?;

        let mut body = json!({
            "model": self.provider.model_name,
            "messages": messages,
            "temperature": self.provider.temperature,
            "max_tokens": self.provider.max_tokens,
            "stream": true,
        });
        if let Some(tools) = tools {
            body["tools"] = tools.clone();
            body["tool_choice"] = json!("auto");
        }

        let url = format!("{}/chat/completions", self.provider.api_base.trim_end_matches('/'));
        tracing::debug!(
            "chat_stream req: model={}, msgs={}, tools_present={}",
            self.provider.model_name,
            messages.len(),
            tools.is_some()
        );
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(self.provider.timeout_seconds.max(30) as u64))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("LLM request failed: {}", e))?;

        let status = resp.status();
        tracing::debug!("chat_stream resp status: {}", status);
        if !status.is_success() {
            let text = resp
                .text()
                .await
                .map_err(|e| format!("LLM read body failed: {}", e))?;
            // 调试留档：失败请求的完整 body（排障用）
            let _ = std::fs::write(
                "/tmp/llm_last_body.json",
                serde_json::to_string_pretty(&body).unwrap_or_default(),
            );
            return Err(format!("LLM API error {}: {}", status, truncate(&text, 500)));
        }

        let mut content_acc = String::new();
        let mut acc: std::collections::BTreeMap<usize, AccToolCall> = Default::default();

        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut saw_data_line = false; // 非 SSE 响应（整包 JSON/HTML）时无 data: 行
        while let Some(bytes) = stream.next().await {
            let bytes = bytes.map_err(|e| format!("LLM stream error: {}", e))?;
            buf.extend_from_slice(&bytes);
            // 按行切分 SSE
            let mut consumed = 0;
            while let Some(pos) = buf[consumed..].iter().position(|&b| b == b'\n') {
                let end = consumed + pos;
                let line = String::from_utf8_lossy(&buf[consumed..end])
                    .trim()
                    .to_string();
                consumed = end + 1;
                if !line.starts_with("data:") {
                    continue; // ignore comments/heartbeats
                }
                saw_data_line = true;
                let data = line[5..].trim();
                if data == "[DONE]" {
                    tracing::debug!(
                        "chat_stream parsed[DONE]: content_chars={}, tool_calls={}, saw_data={}",
                        content_acc.len(),
                        acc.len(),
                        saw_data_line
                    );
                    return finish_stream(content_acc, acc);
                }
                let chunk: WireChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(_) => continue, // 部分网关带 extra whitespace/注解行
                };
                for choice in chunk.choices {
                    let d = choice.delta;
                    if let Some(c) = d.content {
                        // 空 content 只跳过文本增量；绝不能 continue 外层行循环——
                        // 工具调用常与空 content 同 chunk，continue 会连 tool_calls 一起丢。
                        if !c.is_empty() {
                            content_acc.push_str(&c);
                            on_delta(c);
                        }
                    }
                    for tc in d.tool_calls {
                        let slot = acc.entry(tc.index).or_insert_with(|| AccToolCall {
                            id: String::new(),
                            name: String::new(),
                            arguments: String::new(),
                        });
                        if let Some(id) = tc.id {
                            if !id.is_empty() {
                                slot.id = id;
                            }
                        }
                        if let Some(name) = tc.function.name {
                            if !name.is_empty() {
                                slot.name = name; // 空 name delta 忽略，保留首个非空（Flink 教训）
                            }
                        }
                        if let Some(args) = tc.function.arguments {
                            slot.arguments.push_str(&args);
                        }
                    }
                }
            }
            buf.drain(..consumed);
        }

        if !saw_data_line && content_acc.is_empty() && acc.is_empty() {
            return Err(
                "LLM 流式响应不是 SSE 格式（未收到任何 data 行），请检查 Provider 的流式兼容性"
                    .to_string(),
            );
        }
        tracing::debug!(
            "chat_stream parsed: content_chars={}, tool_calls={}, saw_data={}",
            content_acc.len(),
            acc.len(),
            saw_data_line
        );
        finish_stream(content_acc, acc)
    }
}

fn finish_stream(
    content_acc: String,
    acc: std::collections::BTreeMap<usize, AccToolCall>,
) -> Result<ChatCompletion, String> {
    let tool_calls = acc
        .into_values()
        .map(|t| ToolCallDelta { id: t.id, name: t.name, arguments: t.arguments })
        // 空名工具调用无法路由，直接丢弃，避免 agent 死循环（Flink 教训）。
        .filter(|tc| !tc.name.trim().is_empty())
        .collect();
    Ok(ChatCompletion {
        content: if content_acc.is_empty() { None } else { Some(content_acc) },
        tool_calls,
        usage_tokens: 0, // 流式响应默认不携带 usage（未请求 include_usage）
        finish_reason: None,
    })
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { format!("{}...", &s[..n]) }
}
#[cfg(test)]
mod wire_tests {
    use super::*;

    #[test]
    fn parse_tool_call_chunk() {
        // 直接取自内部网关真实响应（带 type 字段、无 name 的后续分片）
        let raw = r#"{"choices":[{"delta":{"content":"","role":"assistant","tool_calls":[{"function":{"arguments":"{\"limit\": 5}","name":"query_metrics"},"id":"call_x","index":0,"type":"function"}]},"index":0}],"created":1,"id":"x","object":"chat.completion.chunk"}"#;
        let chunk: WireChunk = serde_json::from_str(raw).expect("chunk should parse");
        let delta = &chunk.choices[0].delta;
        assert_eq!(delta.tool_calls.len(), 1, "tool_calls should be 1");
        assert_eq!(delta.tool_calls[0].function.name.as_deref(), Some("query_metrics"));
        assert_eq!(delta.tool_calls[0].index, 0);
    }

    #[test]
    fn parse_partial_tool_args_chunk() {
        let raw = r#"{"choices":[{"delta":{"tool_calls":[{"function":{"arguments":"{\"limit\": "},"index":0}]},"index":0}]}"#;
        let chunk: WireChunk = serde_json::from_str(raw).unwrap();
        assert_eq!(chunk.choices[0].delta.tool_calls.len(), 1);
    }
}
