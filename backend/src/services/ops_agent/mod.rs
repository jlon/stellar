//! OLAP 集群智能运维 Agent（第一阶段：对话式只读诊断助手）
//!
//! 实现形态参照 Flink AI job diagnostic assistant（其 agent loop 源自 pi-from-scratch）：
//! - OpenAI 兼容 chat + tools（共享层 `services::ai::llm`），进程内直调查询能力，key 不落浏览器
//! - 每轮刷新集群快照注入上下文（`context.rs`），避免过期事实
//! - 只读工具注册表（`tools/`），复用 metrics/adapter/audit/profile 规则引擎
//! - 会话持久化到共享 `services::ai`（`ai_sessions` / `ai_messages`，channel='agent'），步骤级审计
//! - 本阶段不提供任何写动作（Non-goal：动作闭环在后续阶段）

pub mod agent;
pub mod chat_actions;
pub mod context;
pub mod skills;
pub mod tool;
pub mod tools;

use std::sync::Arc;

use stellar_macros::app_impl;

use crate::config::AuditLogConfig;
use crate::db::AppDb;
use crate::models::cluster::Cluster;
use crate::services::ai::{self, AgentStep, AiSessionStore};
use crate::services::cluster_service::ClusterService;
use crate::services::llm::LLMRepository;
use crate::services::metrics_collector_service::MetricsCollectorService;
use crate::services::mysql_pool_manager::MySQLPoolManager;
use crate::utils::{ApiError, ApiResult};

use self::agent::OltpDiagnosisAgent;
use self::context::{build_system_message_with_context, render_snapshot};
use self::tool::ToolContext;

/// AI 会话通道（与智能问数 ask 共享会话存储，见 docs/agent/ai-common-design.md）。
const SESSION_CHANNEL: &str = "agent";

/// 会话内保留的最大历史消息数（超过则只保留最近 N 条；LLM 压缩留待后续阶段）。
const MAX_HISTORY_MESSAGES: usize = 20;

/// 会话历史字符预算（对齐 Flink assistant 的 CONVERSATION_MAX_CHARS 思路：
/// 工具结果可能数十 KB，仅按条数截断不足以约束上下文体积）。
const MAX_HISTORY_CHARS: usize = 60_000;
/// 每轮最大工具调用次数。
const DEFAULT_MAX_TOOL_CALLS: usize = 8;

/// One chat turn outcome returned to the API layer.
#[derive(Debug, serde::Serialize)]
pub struct ChatOutcome {
    pub session_id: i64,
    pub cluster_id: i64,
    pub steps: Vec<AgentStep>,
    pub final_answer: String,
}

/// Streaming turn event (SSE payloads), emitted in order:
/// `Step*` -> `Answer` -> `Done`（或任一阶段 `Error`）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatStreamEvent {
    Step {
        step: AgentStep,
    },
    /// 模型内容增量（打字机文本）。
    Delta {
        text: String,
    },
    /// 回合语义标记：reasoning | answer（前端把打字机缓冲归位）。
    Phase {
        phase: String,
    },
    /// 对话内动作申请确认卡（propose_action 截获）。
    ActionRequest {
        action: serde_json::Value,
    },
    Answer {
        final_answer: String,
    },
    Done {
        session_id: i64,
        usage_tokens: i64,
    },
    Error {
        message: String,
    },
}

/// Pre-resolved turn inputs shared by `chat` and `chat_stream`.
struct PreparedTurn {
    session_id: i64,
    history: Vec<ai::ChatMessage>,
    system: ai::ChatMessage,
    agent: OltpDiagnosisAgent,
}

pub struct OpsAgentService<DB: AppDb> {
    pool: sqlx::Pool<DB>,
    mysql_pool_manager: Arc<MySQLPoolManager>,
    audit_config: AuditLogConfig,
    cluster_service: Arc<ClusterService<DB>>,
    metrics_collector_service: Arc<MetricsCollectorService<DB>>,
    provider_repo: LLMRepository<DB>,
    max_tool_calls: usize,
    /// 每会话信号量（permit=1）：同一 session 同时只允许一个 turn 进行中（对齐 Flink
    /// "queue the next question until the current turn finishes"，5b6d5af36a2）。
    /// 前端 `sending` 禁发只防了单客户端；双客户端/脚本并发会串写历史，必须服务端兜底。
    /// 用 Semaphore 而非 Mutex：owned permit 可 move 进 'static 任务（SSE 流式在
    /// spawn 内持锁），MutexGuard 的借用生命周期做不到。信号量随进程保留（对象极小）。
    session_guards: std::sync::Mutex<std::collections::HashMap<i64, Arc<tokio::sync::Semaphore>>>,
    /// 会话保留惰性清理：距上次清理超过 24h 才执行一次（进程内标记，不引定时组件）。
    last_cleanup: std::sync::Mutex<std::time::Instant>,
}

#[app_impl]
impl<DB: AppDb> OpsAgentService<DB> {
    pub fn new(
        pool: sqlx::Pool<DB>,
        mysql_pool_manager: Arc<MySQLPoolManager>,
        audit_config: AuditLogConfig,
        cluster_service: Arc<ClusterService<DB>>,
        metrics_collector_service: Arc<MetricsCollectorService<DB>>,
    ) -> Self {
        let provider_repo = LLMRepository::new(pool.clone());
        Self {
            pool,
            mysql_pool_manager,
            audit_config,
            cluster_service,
            metrics_collector_service,
            provider_repo,
            max_tool_calls: DEFAULT_MAX_TOOL_CALLS,
            session_guards: std::sync::Mutex::new(std::collections::HashMap::new()),
            last_cleanup: std::sync::Mutex::new(std::time::Instant::now()),
        }
    }

    /// Whether an enabled LLM provider exists (fail fast with a clear message).
    pub async fn llm_ready(&self) -> bool {
        self.provider_repo
            .get_active_provider()
            .await
            .map(|p| p.is_some())
            .unwrap_or(false)
    }

    /// Resolve the target cluster (explicit id or the active one).
    pub async fn resolve_cluster(&self, cluster_id: Option<i64>) -> ApiResult<Cluster> {
        match cluster_id {
            Some(id) => self.cluster_service.get_cluster(id).await,
            None => self.cluster_service.get_active_cluster().await,
        }
    }

    /// Resolve the session id, persist the user message, and build every input
    /// the agent loop needs (history bounded by count and total chars, fresh
    /// cluster snapshot, tools). Shared by sync and streaming chat.
    async fn prepare_turn(
        &self,
        cluster: &Cluster,
        session_id: Option<i64>,
        user_id: i64,
        created_by: &str,
        message_text: &str,
        page_context: Option<&serde_json::Value>,
    ) -> ApiResult<PreparedTurn> {
        let sessions = AiSessionStore::new(self.pool.clone());

        // 1. Session: reuse or create; persist the user message first so a refresh
        //    can never lose the submitted question (Flink assistant policy).
        let session_id = match session_id {
            Some(id) => {
                // 归属校验（对齐 Flink assistant session jobId 校验）：会话必须存在、
                // 属于同一 channel 与集群，否则拒绝，避免跨集群串写历史。
                let meta = sessions
                    .get_session(id, Some(user_id))
                    .await
                    .map_err(api_err)?
                    .ok_or_else(|| ApiError::not_found("会话不存在"))?;
                if meta.channel != SESSION_CHANNEL || meta.cluster_id != cluster.id {
                    return Err(ApiError::not_found("会话不存在"));
                }
                id
            },
            None => {
                // 标题截断（首条消息前 30 字符），避免长问题撑爆会话列表
                let title: String = message_text.chars().take(30).collect();
                sessions
                    .create_session(SESSION_CHANNEL, cluster.id, None, Some(user_id), &title)
                    .await
                    .map_err(api_err)?
            },
        };
        sessions
            .save_message(session_id, "user", message_text, &[])
            .await
            .map_err(api_err)?;

        // 2. Recent history. load_recent_messages 包含刚保存的 user 消息
        // （保存先于加载），需要弹出最后一条 user 记录——否则 agent 会带
        // 双 user 消息入上下文（实测：内部网关对此无碍但浪费 token 且语义重复）。
        let history = {
            let mut records = sessions
                .load_recent_messages(session_id, MAX_HISTORY_MESSAGES)
                .await
                .map_err(api_err)?;
            if records.last().map(|r| r.role == "user").unwrap_or(false) {
                records.pop();
            }
            // 字符预算：从最新往回保留，超预算的更早消息丢弃。
            let mut used = message_text.len();
            let keep_from = records
                .iter()
                .rev()
                .take_while(|r| {
                    used += r.content.len();
                    used <= MAX_HISTORY_CHARS
                })
                .count();
            let records = &records[records.len().saturating_sub(keep_from)..];
            AiSessionStore::to_chat_messages(records)
        };

        // 3. Provider + fresh snapshot + tools + agent.
        let provider = self
            .provider_repo
            .get_active_provider()
            .await
            .map_err(|e| ApiError::internal_error(format!("读取 LLM Provider 失败: {}", e)))?
            .ok_or_else(|| {
                ApiError::invalid_data("未配置可用的 LLM Provider（请在系统设置中启用）")
            })?;

        let snapshot = match self
            .metrics_collector_service
            .get_latest_snapshot(cluster.id)
            .await
        {
            Ok(s) => render_snapshot(cluster, s.as_ref()),
            Err(_) => render_snapshot(cluster, None),
        };

        let tool_ctx = Arc::new(ToolContext {
            cluster: cluster.clone(),
            pool: self.pool.clone(),
            mysql_pool_manager: Arc::clone(&self.mysql_pool_manager),
            audit_config: self.audit_config.clone(),
            session_id,
            user_id,
            username: created_by.to_string(),
        });
        let tools = tools::create_tools(tool_ctx);
        let system = build_system_message_with_context(&snapshot, &tools, page_context);
        let agent =
            OltpDiagnosisAgent::new(ai::llm::ChatClient::new(provider), tools, self.max_tool_calls);

        Ok(PreparedTurn { session_id, history, system, agent })
    }

    /// 获取（或创建）会话互斥信号量（1 个 permit）。会话 id 未知（新建会话）时
    /// 在 prepare 返回后再取，两步竞态窗口极小（前端已禁发）。
    fn session_guard(&self, session_id: i64) -> Arc<tokio::sync::Semaphore> {
        let mut guards = match self.session_guards.lock() {
            Ok(g) => g,
            Err(_) => {
                // 毒化时重建（仅我们的代码持锁，理论不可达）
                tracing::error!("session_guards poisoned，重建守卫表");
                *self.session_guards.lock().unwrap() = std::collections::HashMap::new();
                self.session_guards.lock().unwrap()
            },
        };
        guards
            .entry(session_id)
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
            .clone()
    }

    /// 会话保留：超过 30 天无活动的会话删除（带 ai_messages 级联）。
    /// 进程内每 24h 至多执行一次（第一次调用即触发，因为 last_cleanup 是启动时刻）。
    async fn maybe_cleanup_sessions(&self) {
        let due = {
            let last = self.last_cleanup.lock().unwrap();
            last.elapsed() >= std::time::Duration::from_secs(24 * 3600)
        };
        if !due {
            return;
        }
        *self.last_cleanup.lock().unwrap() = std::time::Instant::now();
        let sessions = AiSessionStore::new(self.pool.clone());
        match sessions.delete_stale(SESSION_CHANNEL, 30).await {
            Ok(n) if n > 0 => tracing::info!("agent: 清理 {} 个超过 30 天未活动的会话", n),
            Ok(_) => {},
            Err(e) => tracing::warn!("agent: 会话清理失败: {}", e),
        }
    }

    /// Run one chat turn: persist the user message, run the agent loop, persist the answer.
    pub async fn chat(
        &self,
        cluster: &Cluster,
        session_id: Option<i64>,
        user_id: i64,
        created_by: &str,
        user_message: &str,
        page_context: Option<&serde_json::Value>,
    ) -> ApiResult<ChatOutcome> {
        let message_text = user_message.trim();
        if message_text.is_empty() {
            return Err(ApiError::invalid_data("消息不能为空"));
        }
        self.maybe_cleanup_sessions().await;
        let sessions = AiSessionStore::new(self.pool.clone());
        // 整轮串行化（含 user 消息落库）：已有 session_id 时先取 permit 再 prepare。
        let pre_permit = match session_id {
            Some(id) => Some(
                self.session_guard(id)
                    .acquire_owned()
                    .await
                    .map_err(|_| ApiError::internal_error("会话信号量关闭"))?,
            ),
            None => None,
        };
        let prepared = self
            .prepare_turn(cluster, session_id, user_id, created_by, message_text, page_context)
            .await?;
        // 新建会话：创建后立即取 permit（此时无其他竞争者，不会乱序）。
        let _permit = match pre_permit {
            Some(p) => p,
            None => self
                .session_guard(prepared.session_id)
                .acquire_owned()
                .await
                .map_err(|_| ApiError::internal_error("会话信号量关闭"))?,
        };

        let result = prepared
            .agent
            .run(prepared.system, prepared.history, message_text)
            .await
            .map_err(api_err)?;

        // 4. Persist assistant answer with the full trace.
        sessions
            .save_message(prepared.session_id, "assistant", &result.final_answer, &result.steps)
            .await
            .map_err(api_err)?;

        Ok(ChatOutcome {
            session_id: prepared.session_id,
            cluster_id: cluster.id,
            steps: result.steps,
            final_answer: result.final_answer,
        })
    }

    /// Streaming variant of `chat`: identical semantics, but every step is
    /// pushed to the returned receiver as it happens. The turn runs in a
    /// spawned task; when the consumer stops reading (client disconnect), the
    /// agent loop notices the closed sink at the next round boundary and
    /// aborts instead of burning LLM calls (Flink lesson: cancellation must
    /// release in-flight work).
    pub async fn chat_stream(
        &self,
        cluster: &Cluster,
        session_id: Option<i64>,
        user_id: i64,
        created_by: &str,
        user_message: &str,
        page_context: Option<&serde_json::Value>,
    ) -> ApiResult<tokio::sync::mpsc::UnboundedReceiver<ChatStreamEvent>> {
        let message_text = user_message.trim();
        if message_text.is_empty() {
            return Err(ApiError::invalid_data("消息不能为空"));
        }
        self.maybe_cleanup_sessions().await;
        let sessions = AiSessionStore::new(self.pool.clone());
        let pre_permit = match session_id {
            Some(id) => Some(
                self.session_guard(id)
                    .acquire_owned()
                    .await
                    .map_err(|_| ApiError::internal_error("会话信号量关闭"))?,
            ),
            None => None,
        };
        let prepared = self
            .prepare_turn(cluster, session_id, user_id, created_by, message_text, page_context)
            .await?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ChatStreamEvent>();
        let session_id = prepared.session_id;
        let message_text = message_text.to_string();
        // permit 随 turn 任务持有，同会话下一个提问须等本轮结束（Flink 排队语义）。
        let permit = match pre_permit {
            Some(p) => p,
            None => self
                .session_guard(session_id)
                .acquire_owned()
                .await
                .map_err(|_| ApiError::internal_error("会话信号量关闭"))?,
        };
        tokio::spawn(async move {
            let _permit = permit;
            let (progress_tx, progress_rx) =
                tokio::sync::mpsc::unbounded_channel::<agent::ProgressEvent>();
            // Re-emit steps as stream events; stops as soon as the outer sink
            // is gone (which in turn unblocks the agent loop via is_closed).
            let out = tx.clone();
            let forwarder = tokio::spawn(async move {
                let mut progress_rx = progress_rx;
                while let Some(ev) = progress_rx.recv().await {
                    let sent = match ev {
                        agent::ProgressEvent::Step(step) => {
                            out.send(ChatStreamEvent::Step { step })
                        },
                        agent::ProgressEvent::Delta(text) => {
                            out.send(ChatStreamEvent::Delta { text })
                        },
                        agent::ProgressEvent::Phase(phase) => {
                            let phase = match phase {
                                agent::PhaseKind::Reasoning => "reasoning",
                                agent::PhaseKind::Answer => "answer",
                            };
                            out.send(ChatStreamEvent::Phase { phase: phase.to_string() })
                        },
                        agent::ProgressEvent::ActionRequest(action) => {
                            out.send(ChatStreamEvent::ActionRequest { action })
                        },
                    };
                    if sent.is_err() {
                        break;
                    }
                }
            });
            match prepared
                .agent
                .run_with_progress(
                    prepared.system,
                    prepared.history,
                    &message_text,
                    Some(progress_tx),
                )
                .await
            {
                Ok(result) => {
                    let _ = sessions
                        .save_message(session_id, "assistant", &result.final_answer, &result.steps)
                        .await;
                    let _ = tx.send(ChatStreamEvent::Answer { final_answer: result.final_answer });
                    let _ = tx.send(ChatStreamEvent::Done {
                        session_id,
                        usage_tokens: result.usage_tokens,
                    });
                },
                Err(e) => {
                    let _ = tx.send(ChatStreamEvent::Error { message: e });
                },
            }
            forwarder.abort();
        });

        Ok(rx)
    }

    pub async fn list_sessions(
        &self,
        cluster_id: i64,
        user_id: i64,
    ) -> ApiResult<Vec<ai::SessionInfo>> {
        AiSessionStore::new(self.pool.clone())
            .list_sessions(SESSION_CHANNEL, cluster_id, Some(user_id), 100)
            .await
            .map_err(api_err)
    }

    pub async fn get_session(
        &self,
        session_id: i64,
        user_id: Option<i64>,
        feedback_user: Option<i64>,
    ) -> ApiResult<Vec<ai::MessageRecord>> {
        // 归属校验：非本人会话不可读（None = 管理员放行，用于历史/孤儿清理）
        let store = AiSessionStore::new(self.pool.clone());
        let meta = store
            .get_session(session_id, user_id)
            .await
            .map_err(api_err)?
            .ok_or_else(|| ApiError::not_found("会话不存在"))?;
        if meta.channel != SESSION_CHANNEL {
            return Err(ApiError::not_found("会话不存在"));
        }
        let mut messages = store.load_session(session_id).await.map_err(api_err)?;
        // 回显当前用户的点赞状态（访问控制用 owner 旁路，评价主体恒为真实用户）。
        if let Some(uid) = feedback_user {
            let ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
            if let Ok(map) = store.feedback_map(&ids, uid).await {
                for m in &mut messages {
                    m.feedback = map.get(&m.id).cloned();
                }
            }
        }
        Ok(messages)
    }

    /// 回答点赞/点踩（rating up/down；None = 取消评价）。仅 assistant 消息可评价，
    /// 且消息必须属于 agent 通道、评价者必须是会话 owner（None = 管理员放行）。
    pub async fn set_message_feedback(
        &self,
        message_id: i64,
        owner: Option<i64>,
        actor: i64,
        rating: Option<String>,
    ) -> ApiResult<Option<String>> {
        if let Some(ref r) = rating {
            if r != "up" && r != "down" {
                return Err(ApiError::invalid_data("rating 只能是 up/down"));
            }
        }
        let store = AiSessionStore::new(self.pool.clone());
        let (session_id, channel, session_owner) = store
            .get_message_context(message_id)
            .await
            .map_err(api_err)?
            .ok_or_else(|| ApiError::not_found("消息不存在"))?;
        if channel != SESSION_CHANNEL {
            return Err(ApiError::not_found("消息不存在"));
        }
        if let Some(uid) = owner {
            if session_owner != Some(uid) {
                return Err(ApiError::not_found("消息不存在"));
            }
        }
        store
            .set_feedback(message_id, session_id, actor, rating.as_deref())
            .await
            .map_err(api_err)?;
        Ok(rating)
    }

    /// 会话重命名（GPT 同款）：标题 1..60 字符，非本人会话不可改。
    pub async fn rename_session(
        &self,
        session_id: i64,
        user_id: Option<i64>,
        title: &str,
    ) -> ApiResult<String> {
        let title: String = title.trim().chars().take(60).collect();
        if title.is_empty() {
            return Err(ApiError::invalid_data("标题不能为空"));
        }
        let store = AiSessionStore::new(self.pool.clone());
        let meta = store
            .get_session(session_id, user_id)
            .await
            .map_err(api_err)?
            .ok_or_else(|| ApiError::not_found("会话不存在"))?;
        if meta.channel != SESSION_CHANNEL {
            return Err(ApiError::not_found("会话不存在"));
        }
        store
            .rename_session(session_id, &title)
            .await
            .map_err(api_err)?;
        Ok(title)
    }

    pub async fn delete_session(&self, session_id: i64, user_id: Option<i64>) -> ApiResult<()> {
        // 归属校验：非本人会话不可删（None = 管理员放行）
        let store = AiSessionStore::new(self.pool.clone());
        let meta = store
            .get_session(session_id, user_id)
            .await
            .map_err(api_err)?
            .ok_or_else(|| ApiError::not_found("会话不存在"))?;
        if meta.channel != SESSION_CHANNEL {
            return Err(ApiError::not_found("会话不存在"));
        }
        store.delete_session(session_id).await.map_err(api_err)
    }
}

fn api_err(e: String) -> ApiError {
    ApiError::internal_error(format!("OpsAgent: {}", e))
}

/// 独立 impl（无 #[app_impl] 宏）：纯访问器，供 crate 内其他模块取连接池。
impl<DB: crate::db::AppDb> OpsAgentService<DB> {
    /// 连接池访问器（会话/动作 handler 共用同一池）。
    pub fn pool(&self) -> sqlx::Pool<DB> {
        self.pool.clone()
    }
}
