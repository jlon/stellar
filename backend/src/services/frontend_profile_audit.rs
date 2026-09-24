use sqlx::Pool;
use stellar_macros::app_db;

use crate::{db::AppDb, db::query as db_query};

pub struct FrontendProfileAccess<'a> {
    pub user_id: i64,
    pub username: &'a str,
    pub organization_id: Option<i64>,
    pub cluster_id: i64,
    pub cluster_name: &'a str,
    pub frontend_name: &'a str,
    pub frontend_host: &'a str,
    pub http_port: i32,
    pub action: &'a str,
    pub profile_filename: Option<&'a str>,
    pub outcome: &'a str,
}

#[app_db]
pub async fn record_frontend_profile_access<DB: AppDb>(
    pool: &Pool<DB>,
    record: FrontendProfileAccess<'_>,
) -> anyhow::Result<()> {
    db_query::query(
        "INSERT INTO frontend_profile_access_logs \
         (user_id, username, organization_id, cluster_id, cluster_name, frontend_name, frontend_host, http_port, action, profile_filename, outcome) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(record.user_id)
    .bind(record.username)
    .bind(record.organization_id)
    .bind(record.cluster_id)
    .bind(record.cluster_name)
    .bind(record.frontend_name)
    .bind(record.frontend_host)
    .bind(record.http_port)
    .bind(record.action)
    .bind(record.profile_filename)
    .bind(record.outcome)
    .execute(pool)
    .await?;
    Ok(())
}
