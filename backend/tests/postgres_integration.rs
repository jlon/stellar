//! PostgreSQL 真实后端验证；默认忽略，需提供可清理的独立数据库。

use chrono::Utc;
use sqlx::{Postgres, Row};
use stellar::{
    db::{SqlDialect, create_pool, query, query_as, query_scalar},
    models::{Cluster, ClusterType, DeploymentMode},
    services::LLMProvider,
};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires TEST_POSTGRES_URL pointing to a disposable PostgreSQL database"]
async fn postgres_backend_applies_migrations_and_preserves_semantics() -> anyhow::Result<()> {
    let url = std::env::var("TEST_POSTGRES_URL")?;
    let pool = create_pool::<Postgres>(&url).await?;
    let suffix = Uuid::new_v4().simple().to_string();
    let organization_code = format!("postgres_test_{suffix}");
    let cache_key = format!("postgres_cache_{suffix}");

    let arithmetic = query::<Postgres>("SELECT ?::BIGINT + ?::BIGINT AS total")
        .bind(20_i64)
        .bind(22_i64)
        .fetch_one(&pool)
        .await?;
    assert_eq!(arithmetic.get::<i64, _>("total"), 42);

    let organization_id = query::<Postgres>(
        "INSERT INTO organizations (code, name, description, is_system) VALUES (?, ?, ?, ?)",
    )
    .bind(&organization_code)
    .bind("PostgreSQL integration test")
    .bind(Option::<String>::None)
    .bind(false)
    .insert_id(&pool)
    .await?;

    let active_cluster_id =
        insert_cluster(&pool, &format!("active_{suffix}"), organization_id, true).await?;
    insert_cluster(&pool, &format!("inactive_{suffix}"), organization_id, false).await?;
    assert!(
        insert_cluster(&pool, &format!("duplicate_{suffix}"), organization_id, true)
            .await
            .is_err()
    );

    let cluster = query_as::<Postgres, Cluster>("SELECT * FROM clusters WHERE id = ?")
        .bind(active_cluster_id)
        .fetch_one(&pool)
        .await?;
    assert_eq!(cluster.cluster_type, ClusterType::Doris);
    assert_eq!(cluster.deployment_mode, DeploymentMode::SharedData);

    let provider = query_as::<Postgres, LLMProvider>("SELECT * FROM llm_providers WHERE name = ?")
        .bind("deepseek")
        .fetch_one(&pool)
        .await?;
    assert_eq!(provider.max_tokens, 4096);

    let replace_sql = <Postgres as SqlDialect>::replace_sql(
        "llm_cache",
        &["cache_key", "scenario", "request_hash", "response_json", "expires_at"],
        &["cache_key"],
    );
    for response in ["first", "second"] {
        query::<Postgres>(&replace_sql)
            .bind(&cache_key)
            .bind("root_cause_analysis")
            .bind("request")
            .bind(response)
            .bind(Utc::now() + chrono::Duration::hours(1))
            .execute(&pool)
            .await?;
    }
    let cached =
        query_scalar::<Postgres, String>("SELECT response_json FROM llm_cache WHERE cache_key = ?")
            .bind(&cache_key)
            .fetch_one(&pool)
            .await?;
    assert_eq!(cached, "second");

    query::<Postgres>("DELETE FROM llm_cache WHERE cache_key = ?")
        .bind(&cache_key)
        .execute(&pool)
        .await?;
    query::<Postgres>("DELETE FROM clusters WHERE organization_id = ?")
        .bind(organization_id)
        .execute(&pool)
        .await?;
    query::<Postgres>("DELETE FROM organizations WHERE id = ?")
        .bind(organization_id)
        .execute(&pool)
        .await?;

    Ok(())
}

async fn insert_cluster(
    pool: &sqlx::Pool<Postgres>,
    name: &str,
    organization_id: i64,
    is_active: bool,
) -> sqlx::Result<i64> {
    query::<Postgres>(
        "INSERT INTO clusters (
            name, fe_host, username, password_encrypted, organization_id,
            is_active, deployment_mode, cluster_type
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(name)
    .bind("127.0.0.1")
    .bind("stellar")
    .bind("encrypted")
    .bind(organization_id)
    .bind(is_active)
    .bind("shared_data")
    .bind("doris")
    .insert_id(pool)
    .await
}
