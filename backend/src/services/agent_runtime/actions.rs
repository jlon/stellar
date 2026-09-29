//! 动作闭环执行器：两阶段确认的写动作（kill_query / update_variable）。
//!
//! 对齐 Flink platform write actions 教训：
//! 1. 动作凭高熵 UUID 确认，端点本身不可被猜测（ba2d615a0e5）
//! 2. 确认单次执行（confirmIsSingleShot），重复确认幂等拒绝
//! 3. TTL 过期后不可执行
//!
//! 场景：变量和查询动作走 MySQLClient，物化视图通过受控集群适配器执行；
//! 统一只接受结构化参数，禁止调用方拼接任意 SQL。

use serde::Deserialize;
use serde_json::{Value, json};

use crate::models::CreateMaterializedViewRequest;
use crate::models::cluster::Cluster;
use crate::services::cluster_timeout;
use crate::services::create_adapter;
use crate::services::mysql_client::MySQLClient;
use crate::services::mysql_pool_manager::MySQLPoolManager;
use sqlx::Row;

/// 动作类型白名单
pub const ACTION_KINDS: [&str; 3] = ["kill_query", "update_variable", "create_materialized_view"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateMaterializedViewAction {
    request: CreateMaterializedViewRequest,
    confirmed_ddl: String,
}

fn materialized_view_action(params: &Value) -> Result<CreateMaterializedViewAction, String> {
    let action: CreateMaterializedViewAction =
        serde_json::from_value(params.clone()).map_err(|_| "物化视图创建参数不合法".to_string())?;
    action
        .request
        .validate()
        .map_err(|error| error.to_string())?;
    if action.request.query_sql.is_none() {
        return Err("智能运维只能提交自定义 SELECT 物化视图草案".to_string());
    }
    if action.confirmed_ddl.is_empty() || action.confirmed_ddl.len() > 30_000 {
        return Err("物化视图 DDL 审阅内容不合法".to_string());
    }
    Ok(action)
}

/// 校验动作参数；通过则返回动作标题（用于确认卡片展示）。
pub fn validate_params(kind: &str, params: &Value) -> Result<String, String> {
    match kind {
        "kill_query" => {
            let query_id = params
                .get("query_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let chars_ok = !query_id.is_empty()
                && query_id.len() <= 64
                && query_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
            // UUID 形态：含 '-' 分隔，或恰 32 位纯十六进制
            let shape_ok = query_id.contains('-') || query_id.len() == 32;
            if !chars_ok || !shape_ok {
                return Err("query_id 必须是 UUID 形态（十六进制与 -，≤64 字符）".to_string());
            }
            Ok(format!("KILL QUERY {}", query_id))
        },
        "update_variable" => {
            let key = params
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let value = params
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let scope = params
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("global")
                .to_lowercase();
            let valid_key = !key.is_empty()
                && key.len() <= 64
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            // value 只允许字母数字与少量安全标点，防止拼接注入
            let valid_value = !value.is_empty()
                && value.len() <= 64
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-. ".contains(c));
            if !valid_key || !valid_value {
                return Err("变量名（字母/数字/_/.，≤64）与值（字母数字与 _-. 空格，≤64）不合法"
                    .to_string());
            }
            if scope != "global" && scope != "session" {
                return Err("scope 只能是 global 或 session".to_string());
            }
            Ok(format!("SET {} {} = {}", scope.to_uppercase(), key, value))
        },
        "create_materialized_view" => {
            let action = materialized_view_action(params)?;
            Ok(format!("创建物化视图 {}.{}", action.request.database, action.request.name))
        },
        other => Err(format!("未知动作类型: {}（白名单: {}）", other, ACTION_KINDS.join(", "))),
    }
}

/// 执行已确认的动作。返回执行结果文本（成功或失败原因），一律留档。
pub async fn execute_action(
    cluster: &Cluster,
    mysql_pool_manager: &MySQLPoolManager,
    kind: &str,
    params: &Value,
) -> Result<String, String> {
    match kind {
        "kill_query" => {
            let query_id = params
                .get("query_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let sql = format!("KILL QUERY '{}'", query_id);
            execute_sql(cluster, mysql_pool_manager, &sql).await
        },
        "update_variable" => {
            let key = params
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let value = params
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let scope = params
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("global")
                .to_uppercase();
            let sql = format!("SET {} {} = {}", scope, key, value);
            execute_sql(cluster, mysql_pool_manager, &sql).await
        },
        "create_materialized_view" => {
            let action = materialized_view_action(params)?;
            if action.request.cluster_id != Some(cluster.id) {
                return Err("集群已变化，需重新生成并确认物化视图 DDL".to_string());
            }
            let adapter =
                create_adapter(cluster.clone(), std::sync::Arc::new(mysql_pool_manager.clone()));
            let preview = adapter
                .preview_materialized_view(&action.request)
                .await
                .map_err(|error| error.to_string())?;
            if preview != action.confirmed_ddl {
                return Err("物化视图 DDL 已变化，需重新审阅".to_string());
            }
            adapter
                .create_materialized_view(&action.request)
                .await
                .map_err(|error| error.to_string())?;
            Ok(format!("物化视图已创建：{}.{}", action.request.database, action.request.name))
        },
        other => Err(format!("未知动作类型: {}", other)),
    }
}

async fn execute_sql(
    cluster: &Cluster,
    mysql_pool_manager: &MySQLPoolManager,
    sql: &str,
) -> Result<String, String> {
    let pool = mysql_pool_manager
        .get_pool(cluster)
        .await
        .map_err(|e| format!("获取集群连接失败: {}", e))?;
    let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(cluster));
    client
        .execute(sql)
        .await
        .map(|_| format!("执行成功: {}", sql))
        .map_err(|e| format!("执行失败: {}（{}）", sql, e))
}

/// 动作行的序列化视图（API 输出）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActionView {
    pub id: i64,
    pub incident_id: i64,
    pub kind: String,
    pub title: String,
    pub params: Value,
    pub status: String,
    pub action_uuid: String,
    pub created_by: String,
    pub created_at: String,
    pub expires_at: String,
    pub confirmed_at: Option<String>,
    pub confirmed_by: Option<String>,
    pub executed_at: Option<String>,
    pub result_json: Option<String>,
}

#[stellar_macros::app_db]
pub fn row_to_action<DB: crate::db::AppDb>(r: &<DB as sqlx::Database>::Row) -> ActionView {
    ActionView {
        id: r.get("id"),
        incident_id: r.get("incident_id"),
        kind: r.get("kind"),
        title: r.get("title"),
        params: serde_json::from_str(&r.get::<String, _>("params_json")).unwrap_or(json!({})),
        status: r.get("status"),
        action_uuid: r.get("action_uuid"),
        created_by: r.get("created_by"),
        created_at: r.get("created_at"),
        expires_at: r.get("expires_at"),
        confirmed_at: r.get("confirmed_at"),
        confirmed_by: r.get("confirmed_by"),
        executed_at: r.get("executed_at"),
        result_json: r.get("result_json"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_query_valid() {
        let p = json!({"query_id": "8b1d0e5a-1234-4abc-9def-0123456789ab"});
        assert_eq!(
            validate_params("kill_query", &p).unwrap(),
            "KILL QUERY 8b1d0e5a-1234-4abc-9def-0123456789ab"
        );
    }

    #[test]
    fn kill_query_rejects_injection() {
        for bad in [
            "x; DROP TABLE agent_actions; --",
            "8b1d0e5a-1234-4abc-9def-0123456789ab' OR '1'='1",
            "",              // empty
            "abc",           // 非 UUID 形态（无 '-' 且非 32 位）
            &"a".repeat(65), // too long -- must be hex/dash; 'a' is hex so length check catches it
        ] {
            let p = json!({ "query_id": bad });
            assert!(validate_params("kill_query", &p).is_err(), "should reject {:?}", bad);
        }
    }

    #[test]
    fn update_variable_valid_and_rejects() {
        let ok = json!({"scope": "GLOBAL", "key": "query_queue_concurrency_limit", "value": "10"});
        assert_eq!(
            validate_params("update_variable", &ok).unwrap(),
            "SET GLOBAL query_queue_concurrency_limit = 10"
        );
        // value 带引号/分号注入必须拒绝
        for bad in [
            json!({"key": "k", "value": "10; DROP TABLE x;"}),
            json!({"key": "k'=1--", "value": "1"}),
            json!({"key": "k", "value": "''"}),
            json!({"scope": "evil", "key": "k", "value": "1"}),
            json!({"key": "", "value": "1"}),
        ] {
            assert!(validate_params("update_variable", &bad).is_err(), "should reject {:?}", bad);
        }
    }

    #[test]
    fn unknown_kind_rejected() {
        assert!(validate_params("drop_table", &json!({})).is_err());
    }

    #[test]
    fn materialized_view_action_requires_valid_server_review_package() {
        let action = json!({
            "request": {
                "cluster_id": 1,
                "database": "analytics",
                "name": "daily_sales",
                "query_sql": "SELECT day, SUM(amount) AS total FROM orders GROUP BY day",
                "schedule": { "kind": "manual" }
            },
            "confirmed_ddl": "CREATE MATERIALIZED VIEW `analytics`.`daily_sales` AS SELECT"
        });
        assert_eq!(
            validate_params("create_materialized_view", &action).unwrap(),
            "创建物化视图 analytics.daily_sales"
        );

        let mut invalid = action;
        invalid["confirmed_ddl"] = json!("");
        assert!(validate_params("create_materialized_view", &invalid).is_err());
    }
}
