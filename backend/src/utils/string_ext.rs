//! 字符串扩展工具模块
//!
//! 提供常用的字符串处理 lambda 表达式和辅助函数

/// 清理并验证字符串，返回 Option<String>
///
/// 用于处理可选的字符串字段，去除空白并过滤空字符串
///
/// # Example
/// ```ignore
/// let fe_host = clean_optional_string(req.fe_host.as_ref());
/// // 等价于:
/// // req.fe_host.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
/// ```
#[inline]
pub fn clean_optional_string(s: Option<&String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// 清理字符串，去除首尾空白
#[inline]
pub fn trim_string(s: &str) -> String {
    s.trim().to_string()
}

/// 字符串清理扩展 trait
pub trait StringExt {
    /// 清理字符串并返回 Option，空字符串返回 None
    fn clean(&self) -> Option<String>;

    /// 清理字符串，返回清理后的字符串
    fn trimmed(&self) -> String;
}

impl StringExt for str {
    #[inline]
    fn clean(&self) -> Option<String> {
        let trimmed = self.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    }

    #[inline]
    fn trimmed(&self) -> String {
        self.trim().to_string()
    }
}

impl StringExt for String {
    #[inline]
    fn clean(&self) -> Option<String> {
        self.as_str().clean()
    }

    #[inline]
    fn trimmed(&self) -> String {
        self.as_str().trimmed()
    }
}

impl<T: AsRef<str>> StringExt for Option<T> {
    #[inline]
    fn clean(&self) -> Option<String> {
        self.as_ref().and_then(|s| s.as_ref().clean())
    }

    #[inline]
    fn trimmed(&self) -> String {
        self.as_ref()
            .map(|s| s.as_ref().trim().to_string())
            .unwrap_or_default()
    }
}

/// Truncate oversized tool output, keeping the tail (same policy as Flink assistant).
/// 共享层工具函数：ops_agent 与 agent_runtime 共用，避免业务模块间引用。
pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        format!(
            "{}...[truncated: {} chars dropped; narrow the query parameters to get the full picture]",
            &text[text.len().saturating_sub(max_chars)..],
            text.len() - max_chars
        )
    }
}
