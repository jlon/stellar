use chrono::Utc;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Sqlite, SqlitePool};

use crate::{
    middleware::permission_extractor::extract_permission,
    models::{Cluster, ClusterType, DeploymentMode, RequestDetails},
    services::PermissionRequestService,
};

const APPLICANT: i64 = 9001;
const PEER: i64 = 9002;
const ORG_ADMIN: i64 = 9003;
const OTHER_ORG_ADMIN: i64 = 9004;

/// In-memory SQLite with real migrations, used for object-level authorization tests.
async fn auth_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("test db");
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .expect("migrations");
    pool
}

async fn seed_pending_request(pool: &SqlitePool, request_details: &str) -> i64 {
    for statement in [
        "INSERT INTO organizations (code, name, description, is_system) VALUES ('e2e_other', 'Other', '', 0)",
        "INSERT INTO users (id, username, password_hash, email, organization_id) VALUES (9001, 'e2e_applicant', 'x', 'a@t.com', 1), (9002, 'e2e_peer', 'x', 'p@t.com', 1), (9003, 'e2e_org_admin', 'x', 'oa@t.com', 1), (9004, 'e2e_other_admin', 'x', 'oaa@t.com', 2)",
        "INSERT INTO roles (id, code, name, description, is_system) VALUES (9010, 'org_admin', 'Org Admin', '', 0)",
        "INSERT INTO user_roles (user_id, role_id) VALUES (9003, 9010), (9004, 9010)",
        "INSERT INTO clusters (id, name, fe_host, fe_http_port, fe_query_port, username, password_encrypted, catalog, deployment_mode, cluster_type, is_active, organization_id) VALUES (10, 'e2e-cluster', '127.0.0.1', 8030, 9030, 'root', 'p', 'default_catalog', 'shared_nothing', 'starrocks', 1, 1)",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .expect("seed fixture");
    }
    sqlx::query(
        "INSERT INTO permission_requests (id, cluster_id, applicant_id, applicant_org_id, request_type, request_details, reason, status)
         VALUES (1, 10, 9001, 1, 'grant_role', ?, 'e2e', 'pending')",
    )
    .bind(request_details)
    .execute(pool)
    .await
    .expect("seed request");
    1
}

fn service(pool: &SqlitePool) -> PermissionRequestService<Sqlite> {
    PermissionRequestService::new(pool.clone(), "e2e-encryption-key")
}

fn cluster() -> Cluster {
    Cluster {
        id: 1,
        name: "test".to_string(),
        description: None,
        fe_host: "127.0.0.1".to_string(),
        fe_http_port: 8030,
        fe_query_port: 9030,
        username: "stellar".to_string(),
        password_encrypted: "unused".to_string(),
        enable_ssl: false,
        connection_timeout: 10,
        tags: None,
        catalog: "internal".to_string(),
        is_active: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        created_by: Some(1),
        organization_id: Some(1),
        deployment_mode: DeploymentMode::SharedNothing,
        cluster_type: ClusterType::StarRocks,
        admin_user: Some("cluster_admin".to_string()),
        admin_password_encrypted: Some("unused".to_string()),
    }
}

fn table_permission_details() -> RequestDetails {
    RequestDetails {
        action: Some("grant_permission".to_string()),
        target_user: Some("analyst".to_string()),
        target_account: None,
        target_role: None,
        scope: None,
        resource_type: Some("table".to_string()),
        catalog: Some("internal".to_string()),
        database: Some("sales".to_string()),
        table: Some("orders".to_string()),
        permissions: Some(vec!["SELECT".to_string()]),
        with_grant_option: None,
        new_user_name: None,
        new_user_password: None,
        new_role_name: None,
    }
}

#[test]
fn request_shape_rejects_protected_user_and_injected_permission() {
    let mut root = table_permission_details();
    root.target_user = Some("root".to_string());
    assert!(
        PermissionRequestService::<Sqlite>::validate_request_details(
            "grant_permission",
            &root,
            &cluster(),
        )
        .is_err()
    );

    let mut injected = table_permission_details();
    injected.permissions = Some(vec!["SELECT; DROP USER root".to_string()]);
    assert!(
        PermissionRequestService::<Sqlite>::validate_request_details(
            "grant_permission",
            &injected,
            &cluster(),
        )
        .is_err()
    );
}

#[test]
fn request_shape_requires_matching_action_and_accepts_safe_request() {
    let mut mismatched = table_permission_details();
    mismatched.action = Some("revoke_permission".to_string());
    assert!(
        PermissionRequestService::<Sqlite>::validate_request_details(
            "grant_permission",
            &mismatched,
            &cluster(),
        )
        .is_err()
    );

    assert!(
        PermissionRequestService::<Sqlite>::validate_request_details(
            "grant_permission",
            &table_permission_details(),
            &cluster(),
        )
        .is_ok()
    );
}

#[test]
fn permission_request_routes_have_explicit_permission_codes() {
    assert_eq!(
        extract_permission("POST", "/api/permission-requests/42/approve"),
        Some(("permission-requests".to_string(), "approve".to_string()))
    );
    assert_eq!(
        extract_permission("POST", "/api/db-auth/preview-sql"),
        Some(("db-auth".to_string(), "preview-sql".to_string()))
    );
    assert_eq!(
        extract_permission("GET", "/api/clusters/1/db-auth/accounts"),
        Some(("db-auth".to_string(), "accounts:list".to_string()))
    );
    assert_eq!(
        extract_permission("GET", "/api/llm/providers"),
        Some(("llm".to_string(), "providers:list".to_string()))
    );
    assert_eq!(
        extract_permission("POST", "/api/llm/providers/42/activate"),
        Some(("llm".to_string(), "providers:activate".to_string()))
    );
    assert_eq!(
        extract_permission("POST", "/api/llm/providers/42/test"),
        Some(("llm".to_string(), "providers:test".to_string()))
    );
    assert_eq!(
        extract_permission("POST", "/api/llm/analyze/root-cause"),
        Some(("llm".to_string(), "analyze:root-cause".to_string()))
    );
}

#[tokio::test]
async fn new_user_password_is_encrypted_before_persistence() {
    let service = PermissionRequestService::<Sqlite>::new(
        SqlitePool::connect_lazy("sqlite::memory:").expect("lazy test pool"),
        "test-only-jwt-secret",
    );
    let encrypted = service
        .encrypt_credential("p@ss';not-in-sql")
        .expect("encrypt");
    assert!(encrypted.starts_with("v1:"));
    assert!(!encrypted.contains("p@ss"));
    assert_eq!(service.decrypt_credential(&encrypted).expect("decrypt"), "p@ss';not-in-sql");
}

#[tokio::test]
async fn only_applicant_and_reviewers_can_read_a_request() {
    let pool = auth_pool().await;
    let request_id = seed_pending_request(&pool, "{\"action\":\"grant_role\"}").await;
    let service = service(&pool);

    assert!(
        service
            .get_request_detail_for_user(request_id, APPLICANT)
            .await
            .is_ok()
    );
    assert!(
        service
            .get_request_detail_for_user(request_id, PEER)
            .await
            .is_err()
    );
    // Organization reviewers may read; reviewers from another organization may not.
    assert!(
        service
            .get_request_detail_for_user(request_id, ORG_ADMIN)
            .await
            .is_ok()
    );
    assert!(
        service
            .get_request_detail_for_user(request_id, OTHER_ORG_ADMIN)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn only_the_applicant_can_cancel_and_only_while_pending() {
    let pool = auth_pool().await;
    let request_id = seed_pending_request(&pool, "{\"action\":\"grant_role\"}").await;
    let service = service(&pool);

    assert!(service.cancel_request(request_id, PEER).await.is_err());
    assert!(service.cancel_request(request_id, APPLICANT).await.is_ok());
    // The conditional update makes a second transition from `cancelled` impossible.
    assert!(service.cancel_request(request_id, APPLICANT).await.is_err());

    let status: (String,) = sqlx::query_as("SELECT status FROM permission_requests WHERE id = 1")
        .fetch_one(&pool)
        .await
        .expect("status");
    assert_eq!(status.0, "cancelled");
}

#[tokio::test]
async fn review_queue_requires_a_reviewer_role() {
    let pool = auth_pool().await;
    seed_pending_request(&pool, "{\"action\":\"grant_role\"}").await;
    let service = service(&pool);

    assert!(service.ensure_can_review_requests(APPLICANT).await.is_err());
    assert!(service.ensure_can_review_requests(PEER).await.is_err());
    assert!(service.ensure_can_review_requests(ORG_ADMIN).await.is_ok());
}

#[tokio::test]
async fn stored_credentials_never_leave_the_service() {
    let pool = auth_pool().await;
    let legacy = "{\"action\":\"grant_role\",\"target_user\":\"analyst\",\"new_user_password\":\"LegacySecret\"}";
    let request_id = seed_pending_request(&pool, legacy).await;
    let service = service(&pool);

    let detail = service
        .get_request_detail_for_user(request_id, APPLICANT)
        .await
        .expect("applicant can read own request");
    assert!(detail.request_details.new_user_password.is_none());
    assert!(detail.executed_sql.is_none());

    let serialized = serde_json::to_string(&detail).expect("serialize");
    assert!(!serialized.contains("LegacySecret"));
    assert!(!serialized.contains("new_user_password"));
}
