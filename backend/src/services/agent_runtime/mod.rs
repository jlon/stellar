//! Agent runtime（事件闭环）——固定流水线：采集 → 收敛 → Incident 升级 → 取证 → 规则诊断。
//! 设计见 docs/agent/ops-agent-design.md §5.1 / §6；与对话助手（ops_agent）共享底层服务与
//! `services/ai` 通道，业务互不耦合。

pub mod actions;
pub mod capacity;
pub mod collectors;
pub mod diagnoser;
pub mod llm_diagnosis;
pub mod models;
pub mod service;

#[cfg(test)]
mod tests;

pub use service::AgentRuntimeService;
