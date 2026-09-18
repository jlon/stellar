use std::{
    fs::{self, File},
    io::Cursor,
    path::Path,
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{Extension, State},
    http::{HeaderValue, header},
    response::Response,
};
use chrono::Utc;
use stellar_macros::app_db;
use zip::{CompressionMethod, ZipWriter, write::FileOptions};

use crate::{
    AppState,
    db::AppDb,
    middleware::OrgContext,
    services::op_audit::{OpAuditEntry, log_op_best_effort},
    utils::{ApiError, ApiResult},
};

// The archive is buffered in memory: it is capped at MAX_ARCHIVE_BYTES, and
// streaming instead would leave temp files behind on client disconnects.
const MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;

/// Download a ZIP archive containing Stellar's current and rotated application logs.
#[utoipa::path(
    get,
    path = "/api/system/logs/archive",
    responses(
        (status = 200, description = "Log archive", content_type = "application/zip"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "No readable application logs")
    ),
    security(("bearer_auth" = [])),
    tag = "System"
)]
#[app_db]
pub async fn download<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
) -> ApiResult<Response> {
    let log_file = state.log_file.clone().ok_or_else(|| {
        ApiError::not_found("File logging is disabled; no log archive is available")
    })?;

    let archive = tokio::task::spawn_blocking(move || build_log_archive(&log_file))
        .await
        .map_err(|err| ApiError::internal_error(format!("Log archive task failed: {err}")))??
        .ok_or_else(|| ApiError::not_found("No application logs found; nothing to archive"))?;

    let filename = format!("stellar-logs-{}.zip", Utc::now().format("%Y%m%dT%H%M%SZ"));
    log_op_best_effort(
        &state.db,
        OpAuditEntry {
            user_id: org_ctx.user_id,
            username: &org_ctx.username,
            organization_id: org_ctx.organization_id,
            action: "download",
            target_type: "system_log",
            target_id: None,
            target_name: &filename,
        },
    )
    .await;

    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(|err| ApiError::internal_error(format!("Invalid archive filename: {err}")))?;

    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(archive))
        .map_err(|err| ApiError::internal_error(format!("Failed to build archive response: {err}")))
}

/// Builds the log archive. `Ok(None)` means the log directory holds no
/// application log yet (e.g. the server just started), which is not an error.
pub(crate) fn build_log_archive(log_file: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let log_dir = log_file
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Log file has no parent directory"))?;
    let file_name = log_file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Log file name is not valid UTF-8"))?;
    let prefix = log_file
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(file_name);

    let mut files = Vec::new();
    let mut total_size = 0_u64;
    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !is_application_log_file(name, file_name, prefix) {
            continue;
        }

        let size = entry.metadata()?.len();
        total_size = total_size.saturating_add(size);
        if total_size > MAX_ARCHIVE_BYTES {
            anyhow::bail!(
                "Application logs exceed the {} MiB archive limit",
                MAX_ARCHIVE_BYTES / 1024 / 1024
            );
        }
        files.push(entry.path());
    }

    if files.is_empty() {
        return Ok(None);
    }
    files.sort();

    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
    for path in files {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("Log file name is not valid UTF-8"))?;
        archive.start_file(name, options)?;

        let mut file = File::open(path)?;
        std::io::copy(&mut file, &mut archive)?;
    }

    Ok(Some(archive.finish()?.into_inner()))
}

fn is_application_log_file(name: &str, configured_name: &str, prefix: &str) -> bool {
    name == configured_name
        || name
            .strip_prefix(prefix)
            .and_then(|suffix| suffix.strip_prefix('.'))
            .is_some_and(|suffix| suffix.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
}
