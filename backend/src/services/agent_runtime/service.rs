//! AgentRuntimeService：事件闭环调度（tick 编排）。
//!
//! 固定流水线（同进程 ScheduledTask）：
//! `collect → converge → escalate → investigate (取证) → diagnose (规则诊断)`
//! 全程只读；取证失败不阻断（quality=weak）；全局串行执行（护栏：每 tick 至多取证 3 个
//! Incident，单次取证 60s 超时）。

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use sqlx::Row;
use stellar_macros::app_impl;

use crate::config::AgentConfig;
use crate::db::dialect::RowsAffected;
use crate::db::{AppDb, query as db_query};
use crate::models::cluster::Cluster;
use crate::services::agent_runtime::actions::{self, ActionView, row_to_action};
use crate::services::audit_log_service::AuditLogService;
use crate::services::cluster_adapter::create_adapter;
use crate::services::cluster_service::ClusterService;
use crate::services::mysql_pool_manager::MySQLPoolManager;
use crate::utils::ApiResult;

use super::collectors::{collect_cluster_events, converge_events};
use super::diagnoser::{diagnose, digest_of, outcome_json};
use super::llm_diagnosis;
use super::models::{DiagnosisOutcome, EventKind, EventRow, EventState, IncidentRow};

/// Incident 复开窗口（resolved/closed 后 N 天内同 dedupe_key 复开）
const REOPEN_WINDOW_DAYS: i64 = 30;
/// 每 tick 最多自动取证的 Incident 数（防积压）
const MAX_INVESTIGATE_PER_TICK: usize = 3;
/// 单次取证总超时
const INVESTIGATE_TIMEOUT: Duration = Duration::from_secs(60);

/// 证据紧凑摘要（供 LLM 推理，避免全量 payload 进上下文）
#[derive(Debug, Clone)]
pub struct EvidenceSummary {
    pub id: i64,
    pub stage: String,
    pub collector: String,
    pub quality: String,
    pub summary: String,
}

pub struct AgentRuntimeService<DB: AppDb> {
    pool: sqlx::Pool<DB>,
    mysql_pool_manager: Arc<MySQLPoolManager>,
    cluster_service: Arc<ClusterService<DB>>,
    audit_service: Arc<AuditLogService>,
    provider_repo: crate::services::llm::LLMRepository<DB>,
    config: AgentConfig,
}

#[app_impl]
impl<DB: AppDb> AgentRuntimeService<DB> {
    pub fn new(
        pool: sqlx::Pool<DB>,
        mysql_pool_manager: Arc<MySQLPoolManager>,
        cluster_service: Arc<ClusterService<DB>>,
        audit_service: Arc<AuditLogService>,
        config: AgentConfig,
    ) -> Self {
        let provider_repo = crate::services::llm::LLMRepository::new(pool.clone());
        Self { pool, mysql_pool_manager, cluster_service, audit_service, provider_repo, config }
    }

    // ------------------------------------------------------------------
    // 动作闭环（两阶段确认写动作，见 actions.rs）
    // ------------------------------------------------------------------

    /// 动作默认 TTL：15 分钟（Flink PENDING_TTL 教训：过期不可再确认）。
    pub const ACTION_TTL_MINUTES: i64 = 15;

    /// 创建待确认动作：参数白名单校验 + 高熵 UUID + TTL。
    pub async fn create_action(
        &self,
        cluster: &Cluster,
        incident_id: i64,
        kind: &str,
        params: &serde_json::Value,
        created_by: &str,
    ) -> Result<ActionView, String> {
        let title = actions::validate_params(kind, params)?;
        let uuid = uuid::Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now() + chrono::Duration::minutes(Self::ACTION_TTL_MINUTES);
        let raw = db_query::query(
            "INSERT INTO agent_actions                 (incident_id, cluster_id, kind, title, params_json, status, action_uuid,                  created_by, expires_at)              VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
        )
        .bind(incident_id)
        .bind(cluster.id)
        .bind(kind)
        .bind(&title)
        .bind(params.to_string())
        .bind(&uuid)
        .bind(created_by)
        .bind(expires_at.format("%Y-%m-%d %H:%M:%S").to_string())
        .insert_id(&self.pool)
        .await
        .map_err(|e| format!("创建动作失败: {}", e))?;
        let row = db_query::query(
            "SELECT id, incident_id, kind, title, params_json, status, action_uuid, created_by,                     created_at, expires_at, confirmed_at, confirmed_by, executed_at, result_json              FROM agent_actions WHERE id = ?",
        )
        .bind(raw)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "动作创建后读取失败".to_string())?;
        let view = row_to_action::<DB>(&row);

        // 待确认通知（产品语义：需要确认时肯定要通知）
        self.notify_ops(
            crate::services::notification_service::NewNotification::new(
                "action_pending",
                format!("运维动作待确认：{}", view.kind),
            )
            .severity("warning")
            .body(format!("「{}」（15 分钟内有效，单次执行）", view.title))
            .link(format!("/pages/cluster-ops/agent-incidents?incident={}", incident_id))
            .meta(serde_json::json!({ "action_id": view.id, "incident_id": incident_id, "cluster_id": cluster.id })),
        )
        .await;

        Ok(view)
    }

    pub async fn list_actions(&self, incident_id: i64) -> Result<Vec<ActionView>, String> {
        // 惰性过期：列表入口批量校正过期 pending 动作（与 load_action 的惰性标记一致）
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let _ = db_query::query(
            "UPDATE agent_actions SET status = 'expired' \
             WHERE incident_id = ? AND status = 'pending' AND expires_at < ?",
        )
        .bind(incident_id)
        .bind(&now)
        .execute(&self.pool)
        .await;
        let rows = db_query::query(
            "SELECT id, incident_id, kind, title, params_json, status, action_uuid, created_by,                     created_at, expires_at, confirmed_at, confirmed_by, executed_at, result_json              FROM agent_actions WHERE incident_id = ? ORDER BY id DESC",
        )
        .bind(incident_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows.iter().map(|r| row_to_action::<DB>(r)).collect())
    }

    async fn load_action(&self, incident_id: i64, action_id: i64) -> Result<ActionView, String> {
        // 惰性过期：pending 且超过 TTL → 就地标记 expired（列表/确认入口自动校正状态）
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let _ = db_query::query(
            "UPDATE agent_actions SET status = 'expired' \
             WHERE id = ? AND incident_id = ? AND status = 'pending' AND expires_at < ?",
        )
        .bind(action_id)
        .bind(incident_id)
        .bind(&now)
        .execute(&self.pool)
        .await;
        let row = db_query::query(
            "SELECT id, incident_id, kind, title, params_json, status, action_uuid, created_by,                     created_at, expires_at, confirmed_at, confirmed_by, executed_at, result_json              FROM agent_actions WHERE id = ? AND incident_id = ?",
        )
        .bind(action_id)
        .bind(incident_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("动作 {} 不存在", action_id))?;
        Ok(row_to_action::<DB>(&row))
    }

    /// 确认并执行：原子翻转 pending -> executing 保证单次执行
    /// （Flink confirmIsSingleShot 教训：重复确认必须幂等拒绝）。
    pub async fn confirm_action(
        &self,
        cluster: &Cluster,
        incident_id: i64,
        action_id: i64,
        confirmed_by: &str,
    ) -> Result<ActionView, String> {
        let action = self.load_action(incident_id, action_id).await?;
        if action.status != "pending" {
            return Err(format!("动作已处于 {} 状态，不可重复确认（单次执行）", action.status));
        }
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if action.expires_at.as_str() < now.as_str() {
            return Err("动作已过期，不可执行".to_string());
        }
        // 原子翻转（防并发双确认）
        let updated = db_query::query(
            "UPDATE agent_actions SET status = 'executing', confirmed_at = ?, confirmed_by = ? \
             WHERE id = ? AND status = 'pending'",
        )
        .bind(&now)
        .bind(confirmed_by)
        .bind(action_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        if updated.rows_affected() == 0 {
            return Err("动作已被其他会话处理".to_string());
        }
        let _ = confirmed_by;

        // 执行
        let params: serde_json::Value = serde_json::from_str(&action.params.to_string())
            .unwrap_or_else(|_| serde_json::json!({}));
        let result =
            actions::execute_action(cluster, &self.mysql_pool_manager, &action.kind, &params).await;
        let (status, result_json) = match &result {
            Ok(text) => ("executed".to_string(), text.clone()),
            Err(e) => ("failed".to_string(), e.clone()),
        };
        let executed_at = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let rows = db_query::query(
            "UPDATE agent_actions SET status = ?, executed_at = ?, result_json = ? WHERE id = ?",
        )
        .bind(&status)
        .bind(&executed_at)
        .bind(&result_json)
        .bind(action_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        if rows.rows_affected() == 0 {
            return Err("动作状态更新失败".to_string());
        }
        let view = self.load_action(incident_id, action_id).await?;

        // 执行结果通知（executed → info；failed → critical）
        let (sev, title_txt) = if status == "executed" {
            ("info", format!("动作执行成功：{}", view.kind))
        } else {
            ("critical", format!("动作执行失败：{}", view.kind))
        };
        self.notify_ops(
            crate::services::notification_service::NewNotification::new(
                "action_result",
                title_txt,
            )
            .severity(sev)
            .body(crate::utils::string_ext::truncate(&result_json, 120))
            .link(format!("/pages/cluster-ops/agent-incidents?incident={}", incident_id))
            .meta(serde_json::json!({ "action_id": action_id, "incident_id": incident_id, "cluster_id": cluster.id })),
        )
        .await;

        Ok(view)
    }

    /// 取消待处理动作。
    pub async fn cancel_action(
        &self,
        incident_id: i64,
        action_id: i64,
    ) -> Result<ActionView, String> {
        let action = self.load_action(incident_id, action_id).await?;
        if action.status != "pending" {
            return Err(format!("动作已处于 {} 状态，不可取消", action.status));
        }
        db_query::query(
            "UPDATE agent_actions SET status = 'cancelled' WHERE id = ? AND status = 'pending'",
        )
        .bind(action_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        self.load_action(incident_id, action_id).await
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// 一轮完整流水线（每集群错误隔离，ScheduledTask 入口）。
    pub async fn run_once(&self) -> Result<(), anyhow::Error> {
        // 事件保留裁剪：事实表按 last_seen_at 保留 N 天（配置 event_retention_days）。
        // 用应用层 UTC 时间戳比较，规避三方言时间函数差异。
        let cutoff = (chrono::Utc::now()
            - chrono::Duration::days(self.config.event_retention_days as i64))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
        let _ = db_query::query("DELETE FROM agent_events WHERE last_seen_at < ?")
            .bind(&cutoff)
            .execute(&self.pool)
            .await
            .map(|r| tracing::debug!("agent runtime: 事件裁剪完成 rows={}", r.rows_affected()));

        let clusters = match self.cluster_service.list_clusters().await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("agent runtime: 加载集群列表失败: {}", e);
                return Ok(());
            },
        };
        for cluster in &clusters {
            if let Err(e) = self.run_cluster(cluster).await {
                tracing::warn!("agent runtime: cluster {} 流水线失败: {}", cluster.id, e);
            }
        }
        Ok(())
    }

    /// 单集群流水线：采集 → 收敛 → 升级 → 取证 → 诊断。
    async fn run_cluster(&self, cluster: &Cluster) -> Result<(), String> {
        // 慢查询快照（审计，失败不阻断）
        let audit_snapshot = self.collect_audit_slow(cluster).await.unwrap_or_default();

        // 节点明细 adapter（采集器内按需使用）
        let adapter = Some(create_adapter(cluster.clone(), Arc::clone(&self.mysql_pool_manager)));

        let outcome = collect_cluster_events(
            cluster,
            &self.pool,
            adapter.as_deref(),
            Some(audit_snapshot.as_slice()),
        )
        .await;
        let active_events = converge_events(cluster, &self.pool, &outcome).await;

        // 升级/复开
        let incidents = self.escalate(cluster, &active_events).await?;

        // 自动取证 + 规则诊断：
        // - 本 tick 新建/复开的 Incident 立即取证；
        // - open 且尚无证据的 Incident 自动补齐（覆盖取证失败/进程重启场景）。
        let mut pending: Vec<i64> = incidents.iter().map(|i| i.id).collect();
        if let Ok(rows) = db_query::query(
            "SELECT id FROM agent_incidents WHERE cluster_id = ?
             AND status = 'open'
             AND id NOT IN (SELECT DISTINCT incident_id FROM agent_evidences)
             ORDER BY created_at LIMIT ?",
        )
        .bind(cluster.id)
        .bind(MAX_INVESTIGATE_PER_TICK as i64)
        .fetch_all(&self.pool)
        .await
        {
            for r in &rows {
                let id: i64 = r.get("id");
                if !pending.contains(&id) {
                    pending.push(id);
                }
            }
        }
        for incident_id in pending.into_iter().take(MAX_INVESTIGATE_PER_TICK) {
            let _ = tokio::time::timeout(INVESTIGATE_TIMEOUT, async {
                if let Err(e) = self.investigate(cluster, incident_id).await {
                    tracing::warn!("agent runtime: incident {} 取证失败: {}", incident_id, e);
                }
                if let Err(e) = self.diagnose_incident(incident_id).await {
                    tracing::warn!("agent runtime: incident {} 诊断失败: {}", incident_id, e);
                }
            })
            .await;
        }
        Ok(())
    }

    /// 审计慢查询快照：(query_id, sql, duration_ms)，最近窗口内 ≥5s。
    async fn collect_audit_slow(
        &self,
        cluster: &Cluster,
    ) -> Result<Vec<(String, String, i64)>, String> {
        let hours = super::collectors::SLOW_QUERY_WINDOW_HOURS as i32;
        let min_ms = super::collectors::SLOW_QUERY_MS;
        let rows = self
            .audit_service
            .get_slow_queries(cluster, hours, min_ms, 10)
            .await
            .map_err(|e| format!("慢查询摄取失败: {}", e))?;
        Ok(rows
            .into_iter()
            .map(|sq| (sq.query_id.clone(), sq.query_preview.clone(), sq.duration_ms))
            .collect())
    }

    /// Incident 所属集群 id（容量预测按集群取快照序列）。
    async fn incident_cluster_id(&self, incident_id: i64) -> Result<i64, String> {
        let row = db_query::query("SELECT cluster_id FROM agent_incidents WHERE id = ?")
            .bind(incident_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Incident {} 不存在", incident_id))?;
        Ok(row.get("cluster_id"))
    }

    /// 站内通知所有运维角色（admin / super_admin）。Incident/动作类通知的接收方。
    async fn notify_ops(&self, n: crate::services::notification_service::NewNotification) {
        let rows = match db_query::query(
            "SELECT DISTINCT u.id FROM users u \
             JOIN user_roles ur ON ur.user_id = u.id \
             JOIN roles r ON r.id = ur.role_id \
             WHERE r.code IN ('admin', 'super_admin')",
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("agent runtime: 读取通知用户失败: {}", e);
                return;
            },
        };
        for row in rows {
            let user_id: i64 = row.get("id");
            let svc = crate::services::NotificationService::new(self.pool.clone());
            if let Err(e) = svc.create(user_id, n.clone()).await {
                tracing::warn!("agent runtime: 通知创建失败: {}", e);
            }
        }
    }

    /// 事件 → Incident 升级：按对象聚合（cluster / be:host），复开窗口内同 dedupe_key 复开。
    /// 返回本 tick 新建/复开的 Incident。
    async fn escalate(
        &self,
        cluster: &Cluster,
        active_events: &[EventRow],
    ) -> Result<Vec<IncidentRow>, String> {
        if active_events.is_empty() {
            return Ok(vec![]);
        }

        // 按对象聚合：object_type:object_id → 事件列表。
        // 升级门槛（借鉴 sxdevops/Ongrid：默认只自动调查 warning/critical）：
        // 1) slow_query 单独成 incident 太重，跳过；
        // 2) info 级事件只入库不升级（避免噪音 Incident）。
        let mut groups: Vec<(String, String, Vec<EventRow>)> = Vec::new();
        for e in active_events {
            if e.kind == EventKind::SlowQuery || e.severity.as_str() == "info" {
                continue;
            }
            let obj_type = e.object_type.clone().unwrap_or_default();
            let obj_id = e.object_id.clone().unwrap_or_default();
            let key = format!("{}:{}", obj_type, obj_id);
            match groups.iter_mut().find(|(k, _, _)| k == &key) {
                Some((_, _, evs)) => evs.push(e.clone()),
                None => groups.push((obj_type, obj_id, vec![e.clone()])),
            }
        }

        let mut created = Vec::new();
        for (obj_type, obj_id, events) in groups {
            // 最高 severity 事件作为 primary
            let primary = events
                .iter()
                .max_by_key(|e| match e.severity.as_str() {
                    "critical" => 3,
                    "warning" => 2,
                    _ => 1,
                })
                .expect("group non-empty");
            let dedupe_key = format!("{}:{}:{}", cluster.id, obj_type, obj_id);
            let title = format!("{}（{}）", primary.title, obj_id);

            // 1) 已存在活跃 Incident（open/investigating）→ 补事件关联后跳过
            let existing = self
                .find_incident(&dedupe_key, &["open", "investigating"])
                .await?;
            let incident_id = match existing {
                Some(inc) => {
                    self.link_events(inc.id, &events).await?;
                    None
                },
                None => {
                    // 2) 复开：最近 REOPEN_WINDOW_DAYS 内 resolved/closed
                    let reopened = self.find_reopenable(&dedupe_key).await?;
                    match reopened {
                        Some(inc) => {
                            let id = inc.id;
                            let _ = db_query::query(
                                "UPDATE agent_incidents SET status = 'investigating', \
                                        resolved_at = NULL, output_json = NULL WHERE id = ?",
                            )
                            .bind(id)
                            .execute(&self.pool)
                            .await;
                            self.link_events(id, &events).await?;
                            tracing::info!(
                                "agent runtime: incident {} 复开 (dedupe={})",
                                id,
                                dedupe_key
                            );
                            // 复开站内通知
                            self.notify_ops(
                                crate::services::notification_service::NewNotification::new(
                                    "incident_reopened",
                                    format!("Incident 复开：{}", title),
                                )
                                .severity("warning")
                                .body(format!("根因线索再次出现，已自动重新调查（dedupe={}）", dedupe_key))
                                .link(format!("/pages/cluster-ops/agent-incidents?incident={}", id))
                                .meta(serde_json::json!({ "incident_id": id, "cluster_id": cluster.id })),
                            )
                            .await;
                            Some(id)
                        },
                        None => {
                            let id = self
                                .create_incident(cluster, &dedupe_key, &title, &events)
                                .await?;
                            tracing::info!(
                                "agent runtime: incident {} 创建 (dedupe={})",
                                id,
                                dedupe_key
                            );
                            // 新建站内通知（critical 事件 → critical 级别）
                            let severity = if primary.severity.as_str() == "critical" {
                                "critical"
                            } else {
                                "warning"
                            };
                            self.notify_ops(
                                crate::services::notification_service::NewNotification::new(
                                    "incident_created",
                                    format!("新 Incident：{}", title),
                                )
                                .severity(severity)
                                .body(format!("触发事件 {} 个（{}），已开始自动取证与诊断", events.len(), primary.title))
                                .link(format!("/pages/cluster-ops/agent-incidents?incident={}", id))
                                .meta(serde_json::json!({ "incident_id": id, "cluster_id": cluster.id })),
                            )
                            .await;
                            Some(id)
                        },
                    }
                },
            };

            if let Some(id) = incident_id {
                // 事件标 aggregated
                for e in &events {
                    let _ = db_query::query(
                        "UPDATE agent_events SET state = 'aggregated' WHERE id = ? AND state = 'open'",
                    )
                    .bind(e.id)
                    .execute(&self.pool)
                    .await;
                }
                created.push(
                    self.get_incident(id)
                        .await?
                        .ok_or_else(|| "incident 读取失败".to_string())?,
                );
            }
        }
        Ok(created)
    }

    async fn find_incident(
        &self,
        dedupe_key: &str,
        statuses: &[&str],
    ) -> Result<Option<IncidentRow>, String> {
        let placeholders = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, cluster_id, dedupe_key, title, impact_summary, status, output_json, created_at, resolved_at \
             FROM agent_incidents WHERE dedupe_key = ? AND status IN ({}) ORDER BY created_at DESC LIMIT 1",
            placeholders
        );
        let mut q = db_query::query(&sql).bind(dedupe_key);
        for s in statuses {
            q = q.bind(s);
        }
        Ok(q.fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .map(|r| row_to_incident::<DB>(&r)))
    }

    /// 复开候选：最近 REOPEN_WINDOW_DAYS 内 resolved/closed 的同 dedupe Incident。
    async fn find_reopenable(&self, dedupe_key: &str) -> Result<Option<IncidentRow>, String> {
        let since = chrono::Utc::now() - chrono::Duration::days(REOPEN_WINDOW_DAYS);
        let row = db_query::query(
            "SELECT id, cluster_id, dedupe_key, title, impact_summary, status, output_json, created_at, resolved_at \
             FROM agent_incidents WHERE dedupe_key = ? AND status IN ('resolved','closed') \
               AND resolved_at >= ? ORDER BY resolved_at DESC LIMIT 1",
        )
        .bind(dedupe_key)
        .bind(since)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.map(|r| row_to_incident::<DB>(&r)))
    }

    async fn create_incident(
        &self,
        cluster: &Cluster,
        dedupe_key: &str,
        title: &str,
        events: &[EventRow],
    ) -> Result<i64, String> {
        let digest = digest_of(events);
        let id = db_query::query(
            "INSERT INTO agent_incidents (cluster_id, dedupe_key, title, status, input_digest) \
             VALUES (?, ?, ?, 'open', ?)",
        )
        .bind(cluster.id)
        .bind(dedupe_key)
        .bind(title)
        .bind(&digest)
        .insert_id(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        self.link_events(id, events).await?;

        // intake 决策审计
        self.record_decision(
            id,
            "intake",
            "completed",
            Some(json!({ "event_count": events.len(), "kinds": events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>() })),
            None,
            None,
        )
        .await?;
        Ok(id)
    }

    async fn link_events(&self, incident_id: i64, events: &[EventRow]) -> Result<(), String> {
        for e in events {
            let exists = db_query::query(
                "SELECT 1 FROM agent_incident_events WHERE incident_id = ? AND event_id = ?",
            )
            .bind(incident_id)
            .bind(e.id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            if exists.is_none() {
                let _ = db_query::query(
                    "INSERT INTO agent_incident_events (incident_id, event_id) VALUES (?, ?)",
                )
                .bind(incident_id)
                .bind(e.id)
                .execute(&self.pool)
                .await;
            }
        }
        Ok(())
    }

    async fn get_incident(&self, id: i64) -> Result<Option<IncidentRow>, String> {
        let row = db_query::query(
            "SELECT id, cluster_id, dedupe_key, title, impact_summary, status, output_json, created_at, resolved_at \
             FROM agent_incidents WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.map(|r| row_to_incident::<DB>(&r)))
    }

    /// 取证流水线（固定阶段）：metrics → nodes → queries → audit。
    /// 每类证据只采一次；失败不阻断（quality=weak + error 记录）。
    pub async fn investigate(&self, cluster: &Cluster, incident_id: i64) -> Result<(), String> {
        // 幂等：状态推进到 investigating
        let _ = db_query::query(
            "UPDATE agent_incidents SET status = 'investigating' WHERE id = ? AND status = 'open'",
        )
        .bind(incident_id)
        .execute(&self.pool)
        .await;

        let adapter = Some(create_adapter(cluster.clone(), Arc::clone(&self.mysql_pool_manager)));
        let mut evidence_ids = Vec::new();

        // metrics：最近 10 个快照点
        let metrics = db_query::query(
            "SELECT collected_at, qps, query_latency_p95, backend_alive, backend_total, \
                    disk_usage_pct, max_compaction_score, load_running, txn_failed_total \
             FROM metrics_snapshots WHERE cluster_id = ? ORDER BY collected_at DESC LIMIT 10",
        )
        .bind(cluster.id)
        .fetch_all(&self.pool)
        .await;
        match metrics {
            Ok(rows) => {
                let (storage_kind, storage_key) = if cluster.is_shared_data() {
                    ("data_cache", "data_cache_pct")
                } else {
                    ("data_disk", "disk_pct")
                };
                let pts: Vec<Value> = rows
                    .iter()
                    .map(|r| {
                        json!({
                            "at": r.get::<chrono::DateTime<chrono::Utc>, _>("collected_at").to_rfc3339(),
                            "qps": r.get::<f64, _>("qps"),
                            "p95_ms": r.get::<f64, _>("query_latency_p95"),
                            "be": format!("{}/{}", r.get::<i32, _>("backend_alive"), r.get::<i32, _>("backend_total")),
                            "storage_kind": storage_kind,
                            (storage_key): r.get::<f64, _>("disk_usage_pct"),
                            "compaction": r.get::<f64, _>("max_compaction_score"),
                            "load_running": r.get::<i32, _>("load_running"),
                            "txn_failed_total": r.get::<i64, _>("txn_failed_total"),
                        })
                    })
                    .collect();
                if let Ok(id) = self
                    .save_evidence(
                        incident_id,
                        "metrics",
                        "metrics_snapshots",
                        json!({ "storage_kind": storage_kind, "points": pts }),
                        None,
                    )
                    .await
                {
                    evidence_ids.push(id);
                }
            },
            Err(e) => {
                let _ = self
                    .save_evidence(
                        incident_id,
                        "metrics",
                        "metrics_snapshots",
                        json!({}),
                        Some(e.to_string()),
                    )
                    .await;
            },
        }

        // nodes：adapter 节点明细
        match adapter.as_deref() {
            Some(a) => {
                let payload: Value =
                    match serde_json::to_value(&a.get_backends().await.unwrap_or_default()) {
                        Ok(v) => json!({ "backends": v }),
                        Err(_) => json!({}),
                    };
                if let Ok(id) = self
                    .save_evidence(incident_id, "nodes", "cluster_adapter", payload, None)
                    .await
                {
                    evidence_ids.push(id);
                }
            },
            None => {
                let _ = self
                    .save_evidence(
                        incident_id,
                        "nodes",
                        "cluster_adapter",
                        json!({}),
                        Some("adapter 不可用".to_string()),
                    )
                    .await;
            },
        }

        // queries：运行中查询 Top20
        if let Some(a) = adapter.as_deref() {
            match a.get_queries().await {
                Ok(mut qs) => {
                    qs.truncate(20);
                    let payload = serde_json::to_value(&qs).unwrap_or(Value::Null);
                    if let Ok(id) = self
                        .save_evidence(
                            incident_id,
                            "queries",
                            "cluster_adapter",
                            json!({ "running": payload }),
                            None,
                        )
                        .await
                    {
                        evidence_ids.push(id);
                    }
                },
                Err(e) => {
                    let _ = self
                        .save_evidence(
                            incident_id,
                            "queries",
                            "cluster_adapter",
                            json!({}),
                            Some(format!("获取查询失败: {}", e)),
                        )
                        .await;
                },
            }
        }

        // audit：慢查询
        if let Ok(slow) = self.collect_audit_slow(cluster).await {
            if !slow.is_empty() {
                let list: Vec<Value> = slow
                    .iter()
                    .map(|(qid, sql, ms)| json!({ "query_id": qid, "sql": crate::utils::string_ext::truncate(sql, 200), "duration_ms": ms }))
                    .collect();
                if let Ok(id) = self
                    .save_evidence(
                        incident_id,
                        "audit",
                        "audit_log_service",
                        json!({ "slow_queries": list }),
                        None,
                    )
                    .await
                {
                    evidence_ids.push(id);
                }
            }
        }

        // evidence 决策留档
        self.record_decision(
            incident_id,
            "evidence",
            "completed",
            Some(json!({ "collectors": ["metrics", "nodes", "queries", "audit"], "evidence_ids": evidence_ids })),
            None,
            None,
        )
        .await?;
        Ok(())
    }

    /// 规则诊断：依据事件与证据产出确定性结论，写入 incident + decisions。
    pub async fn diagnose_incident(&self, incident_id: i64) -> Result<DiagnosisOutcome, String> {
        let events = self.events_of_incident(incident_id).await?;
        let mut outcome = diagnose(&events);

        // 容量剧本增强：磁盘趋势预测（六阶段容量自适应）
        // 存算分离集群的 disk_usage_pct 是数据缓存配额，不得当成磁盘水位参与诊断结论。
        if outcome.root_cause_type == "capacity" {
            let cluster_id = self.incident_cluster_id(incident_id).await?;
            let shared_data = match self.cluster_service.get_cluster(cluster_id).await {
                Ok(cluster) => cluster.is_shared_data(),
                Err(e) => {
                    tracing::warn!("容量预测跳过：集群 {} 读取失败: {}", cluster_id, e);
                    true
                },
            };
            if !shared_data {
                if let Ok(Some(f)) = super::capacity::forecast_disk(&self.pool, cluster_id).await {
                    let extra = match f.eta_days {
                        Some(days) => format!(
                            "磁盘当前 {:.1}%，近 24h 增长趋势 {:.2}%/天，预计 {:.1} 天后达到 100%",
                            f.current_pct, f.slope_pct_per_day, days
                        ),
                        None if f.current_pct >= 90.0 => {
                            format!("磁盘已接近满盘（{:.1}%），建议立即清理或扩容", f.current_pct)
                        },
                        _ => String::new(),
                    };
                    if !extra.is_empty() {
                        if outcome.summary.trim().is_empty() {
                            outcome.summary = extra;
                        } else {
                            let base = outcome.summary.trim_end_matches(['。', ' ']);
                            outcome.summary = format!("{}。{}", base, extra);
                        }
                    }
                }
            }
        }

        let output = outcome_json(&outcome).to_string();

        let _ = db_query::query(
            "UPDATE agent_incidents SET output_json = ?, impact_summary = ? WHERE id = ?",
        )
        .bind(&output)
        .bind(&outcome.summary)
        .bind(incident_id)
        .execute(&self.pool)
        .await;

        self.record_decision(
            incident_id,
            "rule_diagnosis",
            "completed",
            Some(json!({
                "event_ids": events.iter().map(|e| e.id).collect::<Vec<_>>(),
                "rule": "playbook_match",
                "confidence": outcome.confidence,
            })),
            Some(json!(outcome)),
            None,
        )
        .await?;
        Ok(outcome)
    }

    /// 主动调查 + 诊断（API 手动触发用）。
    pub async fn investigate_and_diagnose(
        &self,
        cluster: &Cluster,
        incident_id: i64,
    ) -> Result<DiagnosisOutcome, String> {
        self.investigate(cluster, incident_id).await?;
        self.diagnose_incident(incident_id).await
    }

    /// LLM 根因分析（第四阶段）：规则诊断未定论时手动触发；
    /// provider 不可用或全部假设被校验拒绝时回退规则摘要（护栏，设计 §6.3）。
    pub async fn llm_analyze_incident(&self, incident_id: i64) -> ApiResult<Value> {
        let incident = self
            .get_incident(incident_id)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?
            .ok_or_else(|| crate::utils::ApiError::not_found("Incident 不存在"))?;
        let events = self
            .events_of_incident(incident_id)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?;
        let summaries = self
            .evidence_summaries(incident_id)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?;

        // 护栏：LLM 不可用 → rejected 决策 + 回退规则摘要
        let provider = match self.provider_repo.get_active_provider().await {
            Ok(Some(p)) if p.enabled => p,
            _ => {
                let fallback = llm_diagnosis::fallback_outcome(incident.impact_summary.as_deref());
                let _ = self
                    .record_decision(
                        incident_id,
                        "llm_hypothesis",
                        "rejected",
                        Some(json!({ "input": "evidence_summary" })),
                        Some(fallback.clone()),
                        Some("LLM Provider 未配置或不可用".to_string()),
                    )
                    .await;
                return Ok(fallback);
            },
        };

        let messages = llm_diagnosis::build_messages(
            &incident.title,
            incident.impact_summary.as_deref(),
            &events,
            &summaries,
        );
        let client = crate::services::ai::llm::ChatClient::new(provider);
        let completion = match client.chat(&messages, None).await {
            Ok(c) => c,
            Err(e) => {
                let fallback = llm_diagnosis::fallback_outcome(incident.impact_summary.as_deref());
                let _ = self
                    .record_decision(
                        incident_id,
                        "llm_hypothesis",
                        "rejected",
                        None,
                        Some(fallback.clone()),
                        Some(format!("LLM 调用失败: {}", e)),
                    )
                    .await;
                return Ok(fallback);
            },
        };

        // 解析 + 校验（证据 ID 必须存在且属于本 Incident）
        let raw = llm_diagnosis::parse_hypotheses(completion.content.as_deref().unwrap_or(""));
        let valid_ids: std::collections::HashSet<i64> = summaries.iter().map(|e| e.id).collect();
        let (hypotheses, dropped, errors) = llm_diagnosis::validate_hypotheses(raw, &valid_ids);

        let status = if hypotheses.is_empty() { "rejected" } else { "completed" };
        let output = json!({
            "hypotheses": hypotheses,
            "dropped_count": dropped,
            "validation_errors": errors,
            "rule_diagnosis": incident.impact_summary,
        });
        let _ = self
            .record_decision(
                incident_id,
                "llm_hypothesis",
                status,
                Some(json!({ "evidence_ids": valid_ids.iter().collect::<Vec<_>>() })),
                Some(output.clone()),
                None,
            )
            .await;

        // 校验未全拒时，把假设合并进 incident.output_json
        if !hypotheses.is_empty() {
            let merged = json!({
                "rule_diagnosis": incident.impact_summary,
                "hypotheses": hypotheses,
            });
            let _ = db_query::query("UPDATE agent_incidents SET output_json = ? WHERE id = ?")
                .bind(merged.to_string())
                .bind(incident_id)
                .execute(&self.pool)
                .await;
        }
        Ok(output)
    }

    /// 证据 → 紧凑摘要（payload 截断 300 字符）
    async fn evidence_summaries(&self, incident_id: i64) -> Result<Vec<EvidenceSummary>, String> {
        let rows = db_query::query(
            "SELECT id, stage, collector, payload_json, quality, error FROM agent_evidences              WHERE incident_id = ? ORDER BY id",
        )
        .bind(incident_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| {
                let payload: String = r.get("payload_json");
                let summary = crate::utils::string_ext::truncate(&payload, 300);
                EvidenceSummary {
                    id: r.get("id"),
                    stage: r.get("stage"),
                    collector: r.get("collector"),
                    quality: r.get("quality"),
                    summary: if summary.is_empty() { "empty".to_string() } else { summary },
                }
            })
            .collect())
    }

    /// 手动触发调查（API 用）：按 incident 解析集群后取证 + 诊断。
    pub async fn investigate_by_id(&self, incident_id: i64) -> ApiResult<DiagnosisOutcome> {
        let incident = self
            .get_incident(incident_id)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?
            .ok_or_else(|| crate::utils::ApiError::not_found("Incident 不存在"))?;
        let cluster = self
            .cluster_service
            .get_cluster(incident.cluster_id)
            .await?;
        self.investigate_and_diagnose(&cluster, incident_id)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))
    }

    /// 关闭 Incident（人工确认恢复）。
    pub async fn close_incident(&self, incident_id: i64) -> Result<(), String> {
        let _ = db_query::query(
            "UPDATE agent_incidents SET status = 'closed', resolved_at = ? WHERE id = ? AND status IN ('open','investigating','resolved')",
        )
        .bind(chrono::Utc::now())
        .bind(incident_id)
        .execute(&self.pool)
        .await;
        self.record_decision(incident_id, "close", "completed", Some(json!({})), None, None)
            .await?;
        Ok(())
    }

    // ---- 查询 API ----

    pub async fn list_incidents(
        &self,
        cluster_id: i64,
        status: Option<&str>,
        limit: i64,
    ) -> ApiResult<Vec<IncidentRow>> {
        let mut q = db_query::query(
            "SELECT id, cluster_id, dedupe_key, title, impact_summary, status, output_json, created_at, resolved_at \
             FROM agent_incidents WHERE cluster_id = ? AND (? IS NULL OR status = ?) \
             ORDER BY created_at DESC LIMIT ?",
        )
        .bind(cluster_id);
        // SQLite/MySQL: NULL 参数匹配用 ? IS NULL 技巧需参数化两次
        q = match status {
            Some(s) => q.bind(s).bind(s),
            None => q.bind(None::<String>).bind(None::<String>),
        };
        let rows = q
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?;
        Ok(rows.iter().map(|r| row_to_incident::<DB>(r)).collect())
    }

    pub async fn get_incident_detail(&self, incident_id: i64) -> ApiResult<Value> {
        let incident = self
            .get_incident(incident_id)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?
            .ok_or_else(|| crate::utils::ApiError::not_found("Incident 不存在"))?;

        let events = self
            .events_of_incident(incident_id)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?;
        let evidences = self
            .evidences_of_incident(incident_id)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?;
        let decisions = self
            .decisions_of_incident(incident_id)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?;

        Ok(json!({
            "incident": incident,
            "events": events,
            "evidences": evidences,
            "decisions": decisions,
        }))
    }

    pub async fn list_events(
        &self,
        cluster_id: i64,
        state: Option<&str>,
        limit: i64,
    ) -> ApiResult<Vec<Value>> {
        let mut q = db_query::query(
            "SELECT id, kind, severity, object_type, object_id, title, summary, state, \
                    occurrence_count, first_seen_at, last_seen_at \
             FROM agent_events WHERE cluster_id = ? AND (? IS NULL OR state = ?) \
             ORDER BY last_seen_at DESC LIMIT ?",
        )
        .bind(cluster_id);
        q = match state {
            Some(s) => q.bind(s).bind(s),
            None => q.bind(None::<String>).bind(None::<String>),
        };
        let rows = q
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| crate::utils::ApiError::internal_error(format!("OpsAgent: {}", e)))?;
        Ok(rows
            .iter()
            .map(|r| {
                json!({
                    "id": r.get::<i64,_>("id"),
                    "kind": r.get::<String,_>("kind"),
                    "severity": r.get::<String,_>("severity"),
                    "object_type": r.get::<Option<String>,_>("object_type"),
                    "object_id": r.get::<Option<String>,_>("object_id"),
                    "title": r.get::<String,_>("title"),
                    "summary": r.get::<Option<String>,_>("summary"),
                    "state": r.get::<String,_>("state"),
                    "occurrence_count": r.get::<i64,_>("occurrence_count"),
                    "first_seen_at": r.get::<chrono::DateTime<chrono::Utc>,_>("first_seen_at").to_rfc3339(),
                    "last_seen_at": r.get::<chrono::DateTime<chrono::Utc>,_>("last_seen_at").to_rfc3339(),
                })
            })
            .collect())
    }

    // ---- 内部 helpers ----

    async fn save_evidence(
        &self,
        incident_id: i64,
        stage: &str,
        collector: &str,
        payload: Value,
        error: Option<String>,
    ) -> Result<i64, String> {
        let quality = if error.is_some() { "weak" } else { "strong" };
        db_query::query(
            "INSERT INTO agent_evidences (incident_id, stage, collector, payload_json, quality, error) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(incident_id)
        .bind(stage)
        .bind(collector)
        .bind(payload.to_string())
        .bind(quality)
        .bind(error)
        .insert_id(&self.pool)
        .await
        .map_err(|e| e.to_string())
    }

    async fn record_decision(
        &self,
        incident_id: i64,
        stage: &str,
        status: &str,
        input_json: Option<Value>,
        output_json: Option<Value>,
        error: Option<String>,
    ) -> Result<(), String> {
        db_query::query(
            "INSERT INTO agent_decisions (incident_id, stage, status, input_json, output_json, error) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(incident_id)
        .bind(stage)
        .bind(status)
        .bind(input_json.map(|v| v.to_string()))
        .bind(output_json.map(|v| v.to_string()))
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn events_of_incident(&self, incident_id: i64) -> Result<Vec<EventRow>, String> {
        let rows = db_query::query(
            "SELECT e.id, e.cluster_id, e.fingerprint, e.kind, e.severity, e.object_type, \
                    e.object_id, e.title, e.state, e.occurrence_count, e.first_seen_at, e.last_seen_at \
             FROM agent_events e JOIN agent_incident_events ie ON ie.event_id = e.id \
             WHERE ie.incident_id = ? ORDER BY e.last_seen_at DESC",
        )
        .bind(incident_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| EventRow {
                id: r.get("id"),
                cluster_id: r.get("cluster_id"),
                fingerprint: r.get("fingerprint"),
                kind: EventKind::parse(&r.get::<String, _>("kind"))
                    .unwrap_or(EventKind::MetricBreach),
                severity: if r.get::<String, _>("severity") == "critical" {
                    super::models::EventSeverity::Critical
                } else if r.get::<String, _>("severity") == "warning" {
                    super::models::EventSeverity::Warning
                } else {
                    super::models::EventSeverity::Info
                },
                object_type: r.get("object_type"),
                object_id: r.get("object_id"),
                title: r.get("title"),
                state: EventState::parse(&r.get::<String, _>("state")).unwrap_or(EventState::Open),
                occurrence_count: r.get("occurrence_count"),
                first_seen_at: r.get("first_seen_at"),
                last_seen_at: r.get("last_seen_at"),
            })
            .collect())
    }

    async fn evidences_of_incident(&self, incident_id: i64) -> Result<Vec<Value>, String> {
        let rows = db_query::query(
            "SELECT id, stage, collector, payload_json, quality, error, created_at \
             FROM agent_evidences WHERE incident_id = ? ORDER BY id",
        )
        .bind(incident_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| {
                json!({
                    "id": r.get::<i64,_>("id"),
                    "stage": r.get::<String,_>("stage"),
                    "collector": r.get::<String,_>("collector"),
                    "payload": serde_json::from_str::<Value>(&r.get::<String,_>("payload_json")).unwrap_or(Value::Null),
                    "quality": r.get::<String,_>("quality"),
                    "error": r.get::<Option<String>,_>("error"),
                    "created_at": r.get::<chrono::DateTime<chrono::Utc>,_>("created_at").to_rfc3339(),
                })
            })
            .collect())
    }

    async fn decisions_of_incident(&self, incident_id: i64) -> Result<Vec<Value>, String> {
        let rows = db_query::query(
            "SELECT id, stage, status, input_json, output_json, error, created_at \
             FROM agent_decisions WHERE incident_id = ? ORDER BY id",
        )
        .bind(incident_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| {
                json!({
                    "id": r.get::<i64,_>("id"),
                    "stage": r.get::<String,_>("stage"),
                    "status": r.get::<String,_>("status"),
                    "input": r.get::<Option<String>,_>("input_json").and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "output": r.get::<Option<String>,_>("output_json").and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "error": r.get::<Option<String>,_>("error"),
                    "created_at": r.get::<chrono::DateTime<chrono::Utc>,_>("created_at").to_rfc3339(),
                })
            })
            .collect())
    }
}

#[stellar_macros::app_db]
fn row_to_incident<DB: AppDb>(r: &<DB as sqlx::Database>::Row) -> IncidentRow {
    IncidentRow {
        id: r.get("id"),
        cluster_id: r.get("cluster_id"),
        dedupe_key: r.get("dedupe_key"),
        title: r.get("title"),
        impact_summary: r.get("impact_summary"),
        status: r.get("status"),
        output_json: r.get("output_json"),
        created_at: r.get("created_at"),
        resolved_at: r.get("resolved_at"),
    }
}

/// ScheduledTask 适配：固定间隔执行一轮流水线。
#[app_impl]
impl<DB: AppDb> crate::utils::scheduled_executor::ScheduledTask for AgentRuntimeService<DB> {
    fn run(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), anyhow::Error>> + Send + '_>>
    {
        Box::pin(async move { self.run_once().await })
    }
}
