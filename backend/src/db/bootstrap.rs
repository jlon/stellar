//! First-run bootstrap, MinIO-style: an empty data directory yields a working
//! server with a one-time admin password; the JWT secret is persisted so
//! restarts do not invalidate sessions.

use std::io::Write;
use std::path::Path;

use rand::RngCore;
use stellar_macros::app_db;

use crate::config::RuntimeMode;
use crate::db::AppDb;
use crate::db::query as db_query;
use sqlx::Pool;

/// Initial admin username, mirroring the historical seed account name.
const ROOT_USERNAME: &str = "admin";

/// Reads the persisted JWT secret from `<data_dir>/.jwt-secret`, creating a
/// random one (0600) on first use. Returns `None` when `data_dir` is unknown
/// (config-file mode manages secrets explicitly).
pub fn ensure_jwt_secret(
    data_dir: Option<&Path>,
    explicit: &str,
) -> anyhow::Result<Option<String>> {
    let Some(dir) = data_dir else {
        return Ok(None);
    };
    if !explicit.is_empty() && explicit != "dev-secret-key-change-in-production" {
        return Ok(None); // user-managed secret wins; nothing to persist
    }

    let file = dir.join(".jwt-secret");
    if let Ok(existing) = fs_secret(&file) {
        return Ok(Some(existing));
    }

    let secret = random_string(64);
    fs_write_private(&file, &secret)?;
    eprintln!("[stellar] Generated JWT secret at {}", file.display());
    Ok(Some(secret))
}

/// Unambiguous random string (no 0/O/1/l) for passwords and secrets.
fn random_string(len: usize) -> String {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz";
    let mut bytes = vec![0_u8; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

/// Creates the initial admin user on an empty `users` table.
///
/// A database created by the historical initial migration contains only the
/// known `admin` seed account. Replace that password once, but never modify a
/// database that has been used to create another user.
///
/// In production, the password comes from `STELLAR_ROOT_PASSWORD` or is
/// generated and printed once. In development, an empty database gets the
/// local default `admin`/`admin`; a historical `admin`/`admin` seed is kept
/// intact. Existing initialized databases are never modified in either mode.
///
/// Returns `Some(password)` when a password was initialized or a production
/// legacy seed was rotated.
#[app_db]
pub async fn ensure_root_user<DB: AppDb>(
    pool: &Pool<DB>,
    runtime_mode: RuntimeMode,
    env_password: Option<String>,
) -> anyhow::Result<Option<String>> {
    const LEGACY_ADMIN_PASSWORD_HASH: &str =
        "$2b$12$LFxvzXbmyBPO9Zp.1MFU4OX3fb8kID8AHYHklokkZvgyzmHuRTc56";

    let user_count: i64 = db_query::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    let has_legacy_seed: bool = user_count == 1
        && db_query::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM users WHERE username = ? AND password_hash = ?)",
        )
        .bind(ROOT_USERNAME)
        .bind(LEGACY_ADMIN_PASSWORD_HASH)
        .fetch_one(pool)
        .await?;
    if user_count != 0 && !has_legacy_seed {
        return Ok(None);
    }

    // Development keeps the historical local seed usable. An explicit reset is
    // required for every other already-initialized database.
    if has_legacy_seed && runtime_mode == RuntimeMode::Development {
        return Ok(None);
    }

    let password = match env_password
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
    {
        Some(p) => {
            eprintln!(
                "[stellar] Created initial user '{}' from STELLAR_ROOT_PASSWORD",
                ROOT_USERNAME
            );
            p
        },
        None if runtime_mode == RuntimeMode::Development => {
            eprintln!(
                "[stellar] Created initial development user '{}' with password 'admin'",
                ROOT_USERNAME
            );
            "admin".to_string()
        },
        None => {
            let generated = random_string(16);
            eprintln!(
                "\n  Created initial user '{}' with password: {}\n\n  This is shown once. Change it after first login.\n",
                ROOT_USERNAME, generated
            );
            generated
        },
    };

    let password_hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST)?;
    let role_id: i64 = db_query::query_scalar("SELECT id FROM roles WHERE code = 'super_admin'")
        .fetch_one(pool)
        .await?;

    if has_legacy_seed {
        db_query::query("UPDATE users SET password_hash = ? WHERE username = ?")
            .bind(&password_hash)
            .bind(ROOT_USERNAME)
            .execute(pool)
            .await?;
    } else {
        db_query::query("INSERT INTO users (username, password_hash) VALUES (?, ?)")
            .bind(ROOT_USERNAME)
            .bind(&password_hash)
            .execute(pool)
            .await?;
        let user_id: i64 = db_query::query_scalar("SELECT id FROM users WHERE username = ?")
            .bind(ROOT_USERNAME)
            .fetch_one(pool)
            .await?;
        db_query::query("INSERT INTO user_roles (user_id, role_id) VALUES (?, ?)")
            .bind(user_id)
            .bind(role_id)
            .execute(pool)
            .await?;
    }

    Ok(Some(password))
}

fn fs_secret(file: &Path) -> anyhow::Result<String> {
    let content = std::fs::read_to_string(file)?;
    let secret = content.trim().to_string();
    if secret.len() < 32 {
        anyhow::bail!("secret file too short");
    }
    Ok(secret)
}

fn fs_write_private(file: &Path, secret: &str) -> anyhow::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(file)?;
    f.write_all(secret.as_bytes())?;
    Ok(())
}
