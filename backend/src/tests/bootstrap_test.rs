use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Sqlite, SqlitePool};

use crate::db::bootstrap::{ensure_jwt_secret, ensure_root_user};

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

#[tokio::test]
async fn fresh_migrations_provision_secure_permission_requests_and_log_archive_access() {
    let pool = fresh_pool().await;

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
}

#[tokio::test]
async fn empty_database_gets_random_root_password() {
    let pool = fresh_pool().await;
    // Simulate a truly empty database (migrations seed the legacy admin).
    sqlx::query("DELETE FROM users").execute(&pool).await.unwrap();

    let password = ensure_root_user::<Sqlite>(&pool, None)
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
        ensure_root_user::<Sqlite>(&pool, None)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn stellar_root_password_env_is_used_verbatim() {
    let pool = fresh_pool().await;

    let created = ensure_root_user::<Sqlite>(&pool, Some("s3cret-手工密码".to_string()))
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

/// Migrations still seed the historical `admin`/`admin` account on existing
/// databases; the first startup must replace that known password instead of
/// leaving a public default credential in place.
#[tokio::test]
async fn legacy_seed_admin_password_is_rotated_on_first_startup() {
    let pool = fresh_pool().await;

    let password = ensure_root_user::<Sqlite>(&pool, None)
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
    assert!(ensure_root_user::<Sqlite>(&pool, None).await.unwrap().is_none());
    let hash_after: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hash_after, hash);
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
