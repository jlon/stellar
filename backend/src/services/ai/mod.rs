//! AI 应用共享基础设施（ask 智能问数 × ops agent 运维助手共用）。
//!
//! - `types.rs`：对话消息 / 工具调用增量 / 步骤审计类型
//! - `llm.rs`：OpenAI 兼容 chat+tools 客户端（复用 `llm_providers` active provider）
//! - `session.rs`：会话持久化（`ai_sessions` / `ai_messages`，channel 隔离业务域）
//!
//! 设计见 docs/agent/ai-common-design.md：业务模块只消费本层，互不引用对方。

pub mod llm;
pub mod session;
pub mod types;

pub use llm::ChatClient;
pub use session::{AiSessionStore, MessageRecord, SessionInfo};
pub use types::{AgentStep, ChatCompletion, ChatMessage, ToolCallDelta};
