use std::time::Duration;

use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

use crate::services::op_audit::{OpAuditEntry, log_op_best_effort};

#[tokio::test]
async fn best_effort_audit_does_not_wait_for_an_unavailable_connection() {
    let pool: SqlitePool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create SQLite test pool");
    sqlx::query(
        "CREATE TABLE op_audit_logs (user_id INTEGER, username TEXT, organization_id INTEGER, action TEXT, target_type TEXT, target_id INTEGER, target_name TEXT)",
    )
    .execute(&pool)
    .await
    .expect("create audit table");

    let _held_connection = pool.acquire().await.expect("hold only connection");
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        log_op_best_effort(
            &pool,
            OpAuditEntry {
                user_id: 1,
                username: "admin",
                organization_id: None,
                action: "test",
                target_type: "provider",
                target_id: Some(1),
                target_name: "test provider",
            },
        ),
    )
    .await;

    assert!(result.is_ok(), "best-effort audit must return within its time limit");
}
