use std::{borrow::Cow, time::SystemTime};

use sqlx::{Sqlite, sqlite::SqlitePoolOptions};

use crate::db::{self, AppDb};

#[tokio::test]
async fn known_legacy_initial_checksum_is_reconciled_before_migrations_run() {
    let path = std::env::temp_dir().join(format!(
        "stellar-migration-compatibility-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos(),
    ));
    let url = format!("sqlite://{}", path.display());
    std::fs::File::create(&path).expect("create test database file");

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("open test database");
    let mut legacy_initial = <Sqlite as AppDb>::migrations()
        .iter()
        .find(|migration| migration.version == 0)
        .cloned()
        .expect("initial migration");
    legacy_initial.checksum =
        Cow::Borrowed(<Sqlite as AppDb>::initial_schema_compatibility_checksums()[0]);
    sqlx::migrate::Migrator {
        migrations: Cow::Owned(vec![legacy_initial]),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
    .run(&pool)
    .await
    .expect("apply legacy initial migration");
    drop(pool);

    let pool = db::create_pool::<Sqlite>(&url)
        .await
        .expect("reconcile known legacy migration checksum");
    let checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 0")
            .fetch_one(&pool)
            .await
            .expect("read reconciled checksum");
    let current_checksum = <Sqlite as AppDb>::migrations()
        .iter()
        .find(|migration| migration.version == 0)
        .expect("current initial migration")
        .checksum
        .to_vec();
    assert_eq!(checksum, current_checksum);

    drop(pool);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn unknown_initial_checksum_remains_rejected() {
    let path = std::env::temp_dir().join(format!(
        "stellar-migration-compatibility-unknown-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos(),
    ));
    let url = format!("sqlite://{}", path.display());
    std::fs::File::create(&path).expect("create test database file");

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("open test database");
    let mut unknown_initial = <Sqlite as AppDb>::migrations()
        .iter()
        .find(|migration| migration.version == 0)
        .cloned()
        .expect("initial migration");
    unknown_initial.checksum = Cow::Owned(vec![0; 48]);
    sqlx::migrate::Migrator {
        migrations: Cow::Owned(vec![unknown_initial]),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
    .run(&pool)
    .await
    .expect("apply unknown initial migration");
    drop(pool);

    let error = db::create_pool::<Sqlite>(&url)
        .await
        .expect_err("unknown migration checksum must remain rejected");
    assert!(
        error
            .to_string()
            .contains("migration 0 was previously applied but has been modified")
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn consolidated_migration_records_are_removed_before_validation() {
    let path = std::env::temp_dir().join(format!(
        "stellar-consolidated-migrations-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos(),
    ));
    let url = format!("sqlite://{}", path.display());

    let pool = db::create_pool::<Sqlite>(&url)
        .await
        .expect("apply initial schema");
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 0")
        .bind(<Sqlite as AppDb>::initial_schema_compatibility_checksums()[2])
        .execute(&pool)
        .await
        .expect("restore pre-consolidation checksum");
    for version in [20260918000000_i64, 20260918000001_i64] {
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
             VALUES (?, ?, TRUE, ?, 0)",
        )
        .bind(version)
        .bind("consolidated migration")
        .bind(vec![0_u8; 48])
        .execute(&pool)
        .await
        .expect("insert legacy migration record");
    }
    drop(pool);

    let pool = db::create_pool::<Sqlite>(&url)
        .await
        .expect("reconcile consolidated migration records");
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM _sqlx_migrations \
         WHERE version IN (20260918000000, 20260918000001)",
    )
    .fetch_one(&pool)
    .await
    .expect("count consolidated migration records");
    assert_eq!(remaining, 0);

    drop(pool);
    let _ = std::fs::remove_file(path);
}
