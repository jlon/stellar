use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Sqlite, SqlitePool};

use crate::config::RuntimeMode;
use crate::db::bootstrap::{
    ensure_backend_diagnostic_permission, ensure_jwt_secret, ensure_root_user,
};

/// In-memory SQLite with real migrations, like a fresh install.
async fn fresh_pool() -> SqlitePool {
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

async fn insert_legacy_admin(pool: &SqlitePool) {
    const LEGACY_ADMIN_PASSWORD_HASH: &str =
        "$2b$12$LFxvzXbmyBPO9Zp.1MFU4OX3fb8kID8AHYHklokkZvgyzmHuRTc56";

    sqlx::query("INSERT INTO users (username, password_hash) VALUES ('admin', ?)")
        .bind(LEGACY_ADMIN_PASSWORD_HASH)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) \
         SELECT (SELECT id FROM users WHERE username = 'admin'), id FROM roles \
         WHERE code IN ('admin', 'super_admin')",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn fresh_migrations_provision_secure_permission_requests_and_new_feature_access() {
    let pool = fresh_pool().await;

    let max_disk_usage_column: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('metrics_snapshots') \
         WHERE name = 'max_disk_usage_pct'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(max_disk_usage_column, 1);

    let encrypted_password_column: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('permission_requests') \
         WHERE name = 'new_user_password_encrypted'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(encrypted_password_column, 1);

    let permitted_roles: Vec<String> = sqlx::query_scalar(
        "SELECT r.code FROM role_permissions rp \
         JOIN roles r ON r.id = rp.role_id \
         JOIN permissions p ON p.id = rp.permission_id \
         WHERE p.code = 'api:system:logs:archive' \
         ORDER BY r.code",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(permitted_roles, ["admin", "super_admin"]);

    let load_api_parent: String = sqlx::query_scalar(
        "SELECT parent.code FROM permissions child \
         JOIN permissions parent ON parent.id = child.parent_id \
         WHERE child.code = 'api:clusters:loads'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(load_api_parent, "menu:loads");

    for permission in ["menu:loads", "api:clusters:loads"] {
        let permitted_roles: Vec<String> = sqlx::query_scalar(
            "SELECT r.code FROM role_permissions rp \
             JOIN roles r ON r.id = rp.role_id \
             JOIN permissions p ON p.id = rp.permission_id \
             WHERE p.code = ? ORDER BY r.code",
        )
        .bind(permission)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(permitted_roles, ["admin", "super_admin"], "{permission}");
    }
}

#[tokio::test]
async fn startup_backfills_backend_diagnostic_permission_for_existing_databases() {
    let pool = fresh_pool().await;
    sqlx::query(
        "DELETE FROM role_permissions WHERE permission_id = ( \
         SELECT id FROM permissions WHERE code = 'api:clusters:backends:diagnose')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM permissions WHERE code = 'api:clusters:backends:diagnose'")
        .execute(&pool)
        .await
        .unwrap();

    ensure_backend_diagnostic_permission::<Sqlite>(&pool)
        .await
        .unwrap();
    ensure_backend_diagnostic_permission::<Sqlite>(&pool)
        .await
        .unwrap();

    let parent: String = sqlx::query_scalar(
        "SELECT parent.code FROM permissions child \
         JOIN permissions parent ON parent.id = child.parent_id \
         WHERE child.code = 'api:clusters:backends:diagnose'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(parent, "menu:nodes:backends");

    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT r.code FROM role_permissions rp \
         JOIN roles r ON r.id = rp.role_id \
         JOIN permissions p ON p.id = rp.permission_id \
         WHERE p.code = 'api:clusters:backends:diagnose' \
         ORDER BY r.code",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(roles, ["admin", "super_admin"]);
}

#[tokio::test]
async fn empty_database_gets_random_root_password() {
    let pool = fresh_pool().await;
    // Simulate a truly empty database.
    sqlx::query("DELETE FROM users")
        .execute(&pool)
        .await
        .unwrap();

    let password = ensure_root_user::<Sqlite>(&pool, RuntimeMode::Production, None)
        .await
        .unwrap()
        .expect("created");

    assert_eq!(password.len(), 16);
    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(bcrypt::verify(&password, &hash).unwrap(), "hash must match the printed password");

    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT r.code FROM user_roles ur JOIN roles r ON r.id = ur.role_id \
         WHERE ur.user_id = (SELECT id FROM users WHERE username = 'admin')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(roles, ["super_admin"]);

    // Second call on a non-empty database is a no-op.
    assert!(
        ensure_root_user::<Sqlite>(&pool, RuntimeMode::Production, None)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn stellar_root_password_env_is_used_verbatim() {
    let pool = fresh_pool().await;

    let created = ensure_root_user::<Sqlite>(
        &pool,
        RuntimeMode::Production,
        Some("s3cret-手工密码".to_string()),
    )
    .await
    .unwrap()
    .expect("created");
    assert_eq!(created, "s3cret-手工密码");

    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(bcrypt::verify("s3cret-手工密码", &hash).unwrap());
}

#[tokio::test]
async fn development_empty_database_uses_local_default_password() {
    let pool = fresh_pool().await;

    let password = ensure_root_user::<Sqlite>(&pool, RuntimeMode::Development, None)
        .await
        .unwrap()
        .expect("created");
    assert_eq!(password, "admin");

    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(bcrypt::verify("admin", &hash).unwrap());

    assert!(
        ensure_root_user::<Sqlite>(
            &pool,
            RuntimeMode::Development,
            Some("must-not-reset-existing-account".to_string()),
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn development_empty_database_honors_explicit_root_password() {
    let pool = fresh_pool().await;

    let password = ensure_root_user::<Sqlite>(
        &pool,
        RuntimeMode::Development,
        Some("local-only-password".to_string()),
    )
    .await
    .unwrap()
    .expect("created");
    assert_eq!(password, "local-only-password");
}

/// Existing databases can retain the historical `admin`/`admin` account; the
/// first startup must replace that known password without changing its roles.
#[tokio::test]
async fn legacy_seed_admin_password_is_rotated_on_first_startup() {
    let pool = fresh_pool().await;
    insert_legacy_admin(&pool).await;

    let password = ensure_root_user::<Sqlite>(&pool, RuntimeMode::Production, None)
        .await
        .unwrap()
        .expect("rotated");

    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(bcrypt::verify(&password, &hash).unwrap(), "hash must match the printed password");

    // The account keeps its identity and roles; only the credential changes.
    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT r.code FROM user_roles ur JOIN roles r ON r.id = ur.role_id \
         WHERE ur.user_id = (SELECT id FROM users WHERE username = 'admin') \
         ORDER BY r.code",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(roles, ["admin", "super_admin"]);

    // Already initialized databases (any other password) are left untouched.
    assert!(
        ensure_root_user::<Sqlite>(&pool, RuntimeMode::Production, None)
            .await
            .unwrap()
            .is_none()
    );
    let hash_after: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hash_after, hash);
}

#[tokio::test]
async fn development_keeps_legacy_admin_seed_unchanged() {
    let pool = fresh_pool().await;
    insert_legacy_admin(&pool).await;

    assert!(
        ensure_root_user::<Sqlite>(
            &pool,
            RuntimeMode::Development,
            Some("must-not-rotate-legacy-seed".to_string()),
        )
        .await
        .unwrap()
        .is_none()
    );

    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(bcrypt::verify("admin", &hash).unwrap());
}

#[tokio::test]
async fn jwt_secret_is_persisted_and_reused() {
    let dir = std::env::temp_dir().join(format!("stellar-bootstrap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Explicit user-managed secret: bootstrap must not touch the filesystem.
    assert!(
        ensure_jwt_secret(Some(&dir), "my-own-secret-value-at-least-32-chars!")
            .unwrap()
            .is_none()
    );

    let first = ensure_jwt_secret(Some(&dir), "dev-secret-key-change-in-production")
        .unwrap()
        .expect("generated");
    let second = ensure_jwt_secret(Some(&dir), "dev-secret-key-change-in-production")
        .unwrap()
        .expect("persisted");
    assert_eq!(first, second, "restart must reuse the stored secret");
    assert_eq!(first.len(), 64);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join(".jwt-secret"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "secret file must be 0600");
    }

    std::fs::remove_dir_all(dir).unwrap();
}
