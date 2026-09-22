//! Shared AI conversation types (chat messages, tool-call deltas, step traces).
//! Used by both the ops agent and (future) ask-data; kept free of business logic.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One message in the chat conversation.
/// 自定义 Serialize：`tool_calls` 必须以 OpenAI 嵌套格式输出
/// （`{"id","type":"function","function":{"name","arguments"}}`），
/// 扁平格式会被严格网关以 400 拒绝（内部网关实测）。
#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
    pub tool_call_id: Option<String>,
}

impl Serialize for ChatMessage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serde_json::Map::new();
        map.insert("role".to_string(), serde_json::json!(self.role));
        if !self.content.is_empty() || self.tool_calls.is_none() {
            map.insert("content".to_string(), serde_json::json!(self.content));
        }
        if let Some(tcs) = &self.tool_calls {
            map.insert(
                "tool_calls".to_string(),
                serde_json::json!(
                    tcs.iter()
                        .map(|t| t.as_openai_message_value())
                        .collect::<Vec<_>>()
                ),
            );
        }
        if let Some(tid) = &self.tool_call_id {
            map.insert("tool_call_id".to_string(), serde_json::json!(tid));
        }
        serde_json::Value::Object(map).serialize(serializer)
    }
}

/// A tool call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub id: String,
    pub name: String,
    pub arguments: String, // JSON-encoded arguments
}

impl ToolCallDelta {
    /// OpenAI 消息格式序列化：`{"id","type":"function","function":{"name","arguments"}}`。
    /// 回传模型时必须以嵌套格式；扁平格式会被严格网关（如内部 responses 网关）以 400 拒绝
    /// （实测：同消息扁平 400、嵌套通过；mock 不校验所以扁平从没暴露）。
    pub fn as_openai_message_value(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "type": "function",
            "function": {
                "name": self.name,
                "arguments": self.arguments,
            }
        })
    }
}

/// The LLM's reply to one chat call.
#[derive(Debug, Clone, Default)]
pub struct ChatCompletion {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCallDelta>,
    pub usage_tokens: i64,
    pub finish_reason: Option<String>,
}

/// One recorded step of the agent run (TraceStep-like event model).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStep {
    pub kind: String, // reasoning | tool | tool_result | end | error
    pub label: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    pub duration_ms: i64,
    pub status: String, // ok | error
}

impl AgentStep {
    pub fn reasoning(detail: impl Into<String>) -> Self {
        Self {
            kind: "reasoning".into(),
            label: "reasoning".into(),
            detail: detail.into(),
            args: None,
            result: None,
            duration_ms: 0,
            status: "ok".into(),
        }
    }
    pub fn tool(name: &str, args: Value) -> Self {
        Self {
            kind: "tool".into(),
            label: name.into(),
            detail: String::new(),
            args: Some(args),
            result: None,
            duration_ms: 0,
            status: "pending".into(),
        }
    }
    pub fn tool_result(name: &str, result: String, duration_ms: i64) -> Self {
        Self {
            kind: "tool_result".into(),
            label: name.into(),
            detail: String::new(),
            args: None,
            result: Some(result),
            duration_ms,
            status: "ok".into(),
        }
    }
    pub fn error(name: &str, error: &str) -> Self {
        Self {
            kind: "error".into(),
            label: name.into(),
            detail: String::new(),
            args: None,
            result: Some(error.to_string()),
            duration_ms: 0,
            status: "error".into(),
        }
    }
    pub fn end() -> Self {
        Self {
            kind: "end".into(),
            label: "turn_end".into(),
            detail: String::new(),
            args: None,
            result: None,
            duration_ms: 0,
            status: "ok".into(),
        }
    }
}

/// Result of one agent turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub steps: Vec<AgentStep>,
    pub final_answer: String,
    pub usage_tokens: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_message_tool_calls_serialize_nested() {
        let m = ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(vec![ToolCallDelta {
                id: "call_x".into(),
                name: "query_metrics".into(),
                arguments: "{\"limit\":5}".into(),
            }]),
            tool_call_id: None,
        };
        let v = serde_json::to_value(&m).unwrap();
        let tc = &v["tool_calls"][0];
        assert_eq!(tc["type"], "function");
        assert_eq!(tc["function"]["name"], "query_metrics");
        assert_eq!(tc["id"], "call_x");
        assert!(v.get("content").is_none(), "空 content 应省略: {}", v);
    }
}
