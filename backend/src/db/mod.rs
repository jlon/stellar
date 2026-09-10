use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

pub async fn create_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    tracing::info!("Initializing database connection: {}", database_url);

    if let Some(dir) = std::path::Path::new(database_url.trim_start_matches("sqlite://")).parent()
        && !dir.exists()
    {
        tracing::debug!("Creating database directory: {:?}", dir);
        std::fs::create_dir_all(dir).map_err(|e| {
            tracing::error!("Failed to create database directory {:?}: {}", dir, e);
            sqlx::Error::Io(e)
        })?;
    }

    let db_path = database_url.trim_start_matches("sqlite://");
    if !std::path::Path::new(db_path).exists() {
        tracing::debug!("Creating database file: {}", db_path);
        std::fs::File::create(db_path).map_err(|e| {
            tracing::error!("Failed to create database file {}: {}", db_path, e);
            sqlx::Error::Io(e)
        })?;
    }

    tracing::debug!("Creating database pool with max_connections=10, busy_timeout=60s");
    // Deployment tasks issue long writes; SQLite's 5s default busy timeout
    // surfaces as spurious "database is locked" API errors under load.
    let options = SqliteConnectOptions::from_str(database_url)
        .map_err(|e| {
            tracing::error!("Invalid database URL: {}", e);
            sqlx::Error::Configuration(Box::new(e))
        })?
        .busy_timeout(Duration::from_secs(60));
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await
        .map_err(|e| {
            tracing::error!("Database connection failed: {}", e);
            e
        })?;

    let migrations_path = find_migrations_dir();
    tracing::info!("Using migrations from: {}", migrations_path);

    tracing::debug!("Running database migrations...");

    sqlx::migrate::Migrator::new(Path::new(&migrations_path))
        .await
        .map_err(|e| {
            tracing::error!("Migration setup failed: {}", e);
            e
        })?
        .run(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Migration execution failed: {}", e);
            e
        })?;

    tracing::info!("Database pool created and migrations applied successfully");

    Ok(pool)
}

fn find_migrations_dir() -> String {
    let possible_paths = ["./migrations", "../migrations", "migrations"];

    for path in &possible_paths {
        if Path::new(path).exists() {
            tracing::debug!("Found migrations directory at: {}", path);
            return path.to_string();
        }
    }

    tracing::warn!("No migrations directory found, using default: ./migrations");
    "./migrations".to_string()
}
