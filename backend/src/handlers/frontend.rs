use axum::{
    Json,
    body::Body,
    extract::{Query, State},
    http::header,
    response::Response,
};
use base64::Engine as _;
use serde::Deserialize;
use sha2::Digest as _;
use std::{net::IpAddr, sync::Arc, time::Duration};
use stellar_macros::app_db;

use crate::{
    AppState,
    db::AppDb,
    middleware::OrgContext,
    models::{Frontend, FrontendProfile},
    services::StarRocksClient,
    utils::{ApiError, ApiResult},
};

const MAX_PROFILE_LIST_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROFILE_HTML_BYTES: usize = 20 * 1024 * 1024;
const MAX_PROFILE_COUNT: usize = 100;
// async-profiler 4.0 renderer prefix; update only with a reviewed renderer fixture.
const PROFILE_CREDIT_LINK: &str = "<a href='https://github.com/async-profiler/async-profiler'>";
const PROFILE_RENDERER_PREFIX_SHA256: [u8; 32] = [
    0xcd, 0x81, 0xbc, 0x0e, 0xc9, 0x87, 0x06, 0x03, 0xa7, 0xc8, 0xc0, 0x23, 0xbc, 0x08, 0x4c, 0xc1,
    0x3e, 0x32, 0xac, 0xe9, 0x63, 0x0c, 0x33, 0xd0, 0x94, 0x1f, 0x09, 0xa3, 0x5a, 0x6b, 0x03, 0x3d,
];
const PROFILE_VIEWER_STYLE: &str = "\
html, body { min-height: 100%; background: var(--stellar-canvas) !important; color: var(--stellar-text) !important; }
body { margin: 0 !important; padding: 0.75rem 0.75rem 1.5rem !important; font: 12px Verdana, sans-serif !important; }
h1 { margin: 0 0 0.5rem !important; color: var(--stellar-text) !important; font-size: 0.875rem !important; font-weight: 600 !important; letter-spacing: 0 !important; text-align: left !important; }
header { margin: 0 0 0.5rem !important; min-height: 2rem; padding: 0.25rem 0 !important; border-bottom: 1px solid var(--stellar-border); line-height: 1.5rem !important; }
header:last-of-type { display: none !important; }
button { min-width: 1.75rem; min-height: 1.75rem; margin-right: 0.25rem; padding: 0.2rem 0.45rem; border: 1px solid var(--stellar-control-border); border-radius: 4px; background: var(--stellar-surface); color: var(--stellar-text); font: inherit; line-height: 1; }
button:hover { border-color: var(--stellar-primary); background: var(--stellar-button-hover); }
button:focus-visible, #reset:focus-visible { outline: 2px solid var(--stellar-primary); outline-offset: 2px; }
#canvas { display: block; width: 100% !important; background: var(--stellar-canvas); }
#hl { box-sizing: border-box; border-radius: 3px; background: var(--stellar-primary-tint) !important; color: var(--stellar-text); outline: 1px solid var(--stellar-primary) !important; }
p { box-sizing: border-box; padding: 0.25rem 0.5rem !important; border: 1px solid var(--stellar-border); border-radius: 4px 4px 0 0; background: var(--stellar-surface) !important; color: var(--stellar-text); outline: 0 !important; }
a { color: var(--stellar-primary) !important; }
* { scrollbar-width: thin; scrollbar-color: var(--stellar-scrollbar) transparent !important; }
*::-webkit-scrollbar { width: 5px !important; height: 5px !important; }
*::-webkit-scrollbar-track { background: transparent !important; }
*::-webkit-scrollbar-thumb, *::-webkit-scrollbar-thumb:hover, *::-webkit-scrollbar-thumb:active, *::-webkit-scrollbar-thumb:window-inactive { background: var(--stellar-scrollbar) !important; border-radius: 999px !important; cursor: default !important; }
#stellar-profile-search-dialog { position: fixed; z-index: 10; inset: 0; display: grid; place-items: start center; padding: min(12vh, 6rem) 1rem 1rem; background: var(--stellar-modal-backdrop); color: var(--stellar-text); font: 12px Verdana, sans-serif; }
#stellar-profile-search-dialog[hidden] { display: none; }
#stellar-profile-search-panel { width: min(24rem, 100%); box-sizing: border-box; padding: 1rem; border: 1px solid var(--stellar-control-border); border-radius: 6px; background: var(--stellar-surface); box-shadow: 0 1.25rem 3.5rem var(--stellar-modal-shadow); }
#stellar-profile-search-title { margin: 0 0 1rem; font-size: 1rem; font-weight: 600; }
#stellar-profile-search-label { display: block; margin-bottom: 0.4rem; color: var(--stellar-hint); }
#stellar-profile-search-input { width: 100%; box-sizing: border-box; padding: 0.625rem 0.75rem; border: 1px solid var(--stellar-control-border); border-radius: 4px; outline: none; background: var(--stellar-canvas); color: var(--stellar-text); font: inherit; }
#stellar-profile-search-input:focus { border-color: var(--stellar-primary); box-shadow: 0 0 0 2px var(--stellar-primary-tint); }
#stellar-profile-search-error { min-height: 1rem; margin: 0.5rem 0 0; color: var(--stellar-error); }
#stellar-profile-search-actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 1rem; }
#stellar-profile-search-actions button { padding: 0.45rem 0.8rem; margin: 0; }
#stellar-profile-search-submit { border-color: var(--stellar-primary); background: var(--stellar-primary) !important; color: var(--stellar-primary-text) !important; }
#stellar-profile-search-submit:hover { background: var(--stellar-primary-hover) !important; }";
const PROFILE_DEFAULT_THEME_TOKENS: &str = ":root { color-scheme: light; --stellar-canvas: #ffffff; --stellar-surface: #f7f9fc; --stellar-border: #e4e9f2; --stellar-text: #222b45; --stellar-hint: #8f9bb3; --stellar-primary: #3366ff; --stellar-primary-hover: #598bff; --stellar-primary-text: #ffffff; --stellar-primary-tint: rgba(51, 102, 255, 0.16); --stellar-control-border: #c5cee0; --stellar-button-hover: #edf1f7; --stellar-modal-backdrop: rgba(34, 43, 69, 0.28); --stellar-modal-shadow: rgba(34, 43, 69, 0.18); --stellar-error: #d14343; --stellar-scrollbar: rgba(143, 155, 179, 0.62); }";
const PROFILE_DARK_THEME_TOKENS: &str = ":root { color-scheme: dark; --stellar-canvas: #222b45; --stellar-surface: #2a3555; --stellar-border: #151a30; --stellar-text: #ffffff; --stellar-hint: #8f9bb3; --stellar-primary: #3366ff; --stellar-primary-hover: #598bff; --stellar-primary-text: #ffffff; --stellar-primary-tint: rgba(51, 102, 255, 0.28); --stellar-control-border: #3d4a69; --stellar-button-hover: #2f3c5c; --stellar-modal-backdrop: rgba(15, 20, 37, 0.76); --stellar-modal-shadow: rgba(15, 20, 37, 0.68); --stellar-error: #ff708d; --stellar-scrollbar: rgba(180, 180, 219, 0.32); }";
const PROFILE_COSMIC_THEME_TOKENS: &str = ":root { color-scheme: dark; --stellar-canvas: #252547; --stellar-surface: #323259; --stellar-border: #1b1b38; --stellar-text: #ffffff; --stellar-hint: #b4b4db; --stellar-primary: #a16eff; --stellar-primary-hover: #b18aff; --stellar-primary-text: #ffffff; --stellar-primary-tint: rgba(161, 110, 255, 0.24); --stellar-control-border: #6a6a94; --stellar-button-hover: #3e2494; --stellar-modal-backdrop: rgba(19, 19, 43, 0.72); --stellar-modal-shadow: rgba(19, 19, 43, 0.64); --stellar-error: #ff708d; --stellar-scrollbar: rgba(180, 180, 219, 0.32); }";
const PROFILE_SEARCH_DIALOG: &str = "\
<div id=\"stellar-profile-search-dialog\" role=\"dialog\" aria-modal=\"true\" aria-labelledby=\"stellar-profile-search-title\" hidden>
  <div id=\"stellar-profile-search-panel\">
    <h2 id=\"stellar-profile-search-title\">搜索火焰图</h2>
    <label id=\"stellar-profile-search-label\" for=\"stellar-profile-search-input\">正则表达式</label>
    <input id=\"stellar-profile-search-input\" type=\"text\" autocomplete=\"off\" spellcheck=\"false\">
    <p id=\"stellar-profile-search-error\" role=\"alert\"></p>
    <div id=\"stellar-profile-search-actions\">
      <button id=\"stellar-profile-search-cancel\" type=\"button\">取消</button>
      <button id=\"stellar-profile-search-submit\" type=\"button\">搜索</button>
    </div>
  </div>
</div>";
const PROFILE_SEARCH_PROMPT: &str = "\
\t\tif (r === true && (r = prompt('Enter regexp to search:', '')) === null) {
\t\t\treturn;
\t\t}";
const PROFILE_SEARCH_PROMPT_REPLACEMENT: &str = "\
\t\tif (r === true) {
\t\t\topenProfileSearch();
\t\t\treturn;
\t\t}";
const PROFILE_SEARCH_BEHAVIOR: &str = "\
\n\tconst profileSearchDialog = document.getElementById('stellar-profile-search-dialog');
\tconst profileSearchInput = document.getElementById('stellar-profile-search-input');
\tconst profileSearchError = document.getElementById('stellar-profile-search-error');
\tfunction closeProfileSearch() {
\t\tprofileSearchDialog.hidden = true;
\t\tprofileSearchError.textContent = '';
\t}
\tfunction submitProfileSearch() {
\t\tconst expression = profileSearchInput.value;
\t\ttry {
\t\t\tif (expression) RegExp(expression);
\t\t} catch (_) {
\t\t\tprofileSearchError.textContent = '正则表达式无效，请检查后重试';
\t\t\treturn;
\t\t}
\t\tcloseProfileSearch();
\t\tsearch(expression);
\t}
\tfunction openProfileSearch() {
\t\tprofileSearchInput.value = pattern ? pattern.source : '';
\t\tprofileSearchError.textContent = '';
\t\tprofileSearchDialog.hidden = false;
\t\tprofileSearchInput.focus();
\t}
\tprofileSearchDialog.onclick = function(event) {
\t\tif (event.target === profileSearchDialog) closeProfileSearch();
\t};
\twindow.addEventListener('keydown', function(event) {
\t\tif (profileSearchDialog.hidden) return;
\t\tif (event.key === 'Escape') {
\t\t\tevent.preventDefault();
\t\t\tevent.stopPropagation();
\t\t\tcloseProfileSearch();
\t\t} else if (event.key === 'Enter') {
\t\t\tevent.preventDefault();
\t\t\tevent.stopPropagation();
\t\t\tsubmitProfileSearch();
\t\t}
\t}, true);
\tdocument.getElementById('stellar-profile-search-cancel').onclick = closeProfileSearch;
\tdocument.getElementById('stellar-profile-search-submit').onclick = submitProfileSearch;";
const PROFILE_CANVAS_SETUP: &str = "\
\tconst canvasWidth = canvas.offsetWidth;
\tconst canvasHeight = canvas.offsetHeight;
\tcanvas.style.width = canvasWidth + 'px';
\tcanvas.width = canvasWidth * (devicePixelRatio || 1);
\tcanvas.height = canvasHeight * (devicePixelRatio || 1);
\tif (devicePixelRatio) c.scale(devicePixelRatio, devicePixelRatio);
\tc.font = document.body.style.font;";
const PROFILE_CANVAS_SETUP_RESIZABLE: &str = "\
\tlet canvasWidth = 0;
\tlet canvasHeight = 0;
\tlet resizeFrame;
\tfunction resizeCanvas() {
\t\tcanvas.style.width = '100%';
\t\tconst width = canvas.offsetWidth;
\t\tconst height = canvas.offsetHeight;
\t\tif (!width || !height) return;
\t\tcanvasWidth = width;
\t\tcanvasHeight = height;
\t\tconst scale = devicePixelRatio || 1;
\t\tcanvas.width = width * scale;
\t\tcanvas.height = height * scale;
\t\tc.setTransform(scale, 0, 0, scale, 0, 0);
\t\tc.font = document.body.style.font;
\t\tif (root) render(root);
\t}
\tresizeCanvas();
\twindow.addEventListener('resize', function() {
\t\tcancelAnimationFrame(resizeFrame);
\t\tresizeFrame = requestAnimationFrame(resizeCanvas);
\t});";
const PROFILE_CANVAS_BACKGROUND: &str = "\
\t\t\tc.fillStyle = '#ffffff';";
const PROFILE_CANVAS_STELLAR_BACKGROUND: &str = "\
\t\t\tc.fillStyle = '#252547';";
const PROFILE_FLAME_PALETTE: &str = "\
\tconst palette = [
\t\t[0xb2e1b2, 20, 20, 20],
\t\t[0x50e150, 30, 30, 30],
\t\t[0x50cccc, 30, 30, 30],
\t\t[0xe15a5a, 30, 40, 40],
\t\t[0xc8c83c, 30, 30, 10],
\t\t[0xe17d00, 30, 30,  0],
\t\t[0xcce880, 20, 20, 20],
\t];";
const PROFILE_STELLAR_FLAME_PALETTE: &str = "\
\tconst palette = [
\t\t[0x5d5b95, 16, 16, 16],
\t\t[0x3164a8, 18, 18, 18],
\t\t[0x317d8c, 16, 16, 16],
\t\t[0xa14e81, 16, 16, 16],
\t\t[0x735628, 18, 18, 10],
\t\t[0x6d5bd0, 16, 16, 16],
\t\t[0x3e7f6d, 16, 16, 16],
\t];";
const PROFILE_DARK_FLAME_PALETTE: &str = "\
	const palette = [
		[0x536bb3, 16, 16, 16],
		[0x3372ae, 18, 18, 18],
		[0x3d8790, 16, 16, 16],
		[0x9b5383, 16, 16, 16],
		[0x7c632d, 18, 18, 10],
		[0x6868c9, 16, 16, 16],
		[0x3d7a6d, 16, 16, 16],
	];";
const PROFILE_FLAME_TEXT_COLOR: &str = "c.fillStyle = '#000000';";
const PROFILE_STELLAR_FLAME_TEXT_COLOR: &str = "c.fillStyle = '#ffffff';";
const PROFILE_FLAME_PARENT_OVERLAY: &str = "c.fillStyle = 'rgba(255, 255, 255, 0.5)';";
const PROFILE_STELLAR_FLAME_PARENT_OVERLAY: &str = "c.fillStyle = 'rgba(255, 255, 255, 0.12)';";
const PROFILE_FLAME_SEARCH_HIGHLIGHT: &str = "'#ee00ee'";
const PROFILE_STELLAR_FLAME_SEARCH_HIGHLIGHT: &str = "'#ff6ee7'";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProfileViewerTheme {
    Default,
    Dark,
    Cosmic,
    Corporate,
}

impl ProfileViewerTheme {
    fn style_tokens(self) -> &'static str {
        match self {
            Self::Default | Self::Corporate => PROFILE_DEFAULT_THEME_TOKENS,
            Self::Dark => PROFILE_DARK_THEME_TOKENS,
            Self::Cosmic => PROFILE_COSMIC_THEME_TOKENS,
        }
    }

    fn uses_dark_flame_palette(self) -> bool {
        matches!(self, Self::Dark | Self::Cosmic)
    }

    fn flame_palette(self) -> &'static str {
        match self {
            Self::Dark => PROFILE_DARK_FLAME_PALETTE,
            Self::Cosmic => PROFILE_STELLAR_FLAME_PALETTE,
            Self::Default | Self::Corporate => PROFILE_FLAME_PALETTE,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FrontendListQuery {
    cluster_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct FrontendProfileQuery {
    cluster_id: i64,
    name: String,
    host: String,
    http_port: String,
}

// Get all frontends for a cluster
#[utoipa::path(
    get,
    path = "/api/clusters/frontends",
    params(("cluster_id" = Option<i64>, Query, description = "Selected active cluster ID")),
    responses(
        (status = 200, description = "List of frontend nodes", body = Vec<Frontend>),
        (status = 404, description = "No active cluster found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Frontends"
)]
#[app_db]
pub async fn list_frontends(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    Query(query): Query<FrontendListQuery>,
) -> ApiResult<Json<Vec<Frontend>>> {
    let cluster = match query.cluster_id {
        Some(cluster_id) => selected_active_cluster(&state, &org_ctx, cluster_id).await?,
        None => active_cluster(&state, &org_ctx).await?,
    };
    let cluster_id = cluster.id;
    if let Some(frontends) = state
        .metrics_collector_service
        .cached_frontends(cluster_id, Duration::from_secs(90))
        .or_else(|| state.metrics_collector_service.stale_frontends(cluster_id))
    {
        return Ok(Json(frontends));
    }
    let adapter = crate::services::create_adapter(cluster, state.mysql_pool_manager.clone());
    let frontends = adapter.get_frontends().await?;
    state
        .metrics_collector_service
        .store_frontends(cluster_id, frontends.clone());
    Ok(Json(frontends))
}

#[utoipa::path(
    get,
    path = "/api/clusters/frontends/profiles",
    params(
        ("cluster_id" = i64, Query, description = "Active cluster ID"),
        ("name" = String, Query, description = "Discovered frontend name"),
        ("host" = String, Query, description = "Discovered frontend host"),
        ("http_port" = String, Query, description = "Discovered frontend HTTP port")
    ),
    responses(
        (status = 200, description = "Available memory profiles", body = Vec<FrontendProfile>),
        (status = 404, description = "Cluster, frontend, or profile endpoint not found"),
        (status = 502, description = "Frontend request failed")
    ),
    security(("bearer_auth" = [])),
    tag = "Frontends"
)]
#[app_db]
pub async fn list_frontend_profiles<DB: crate::db::AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    Query(query): Query<FrontendProfileQuery>,
) -> ApiResult<Json<Vec<FrontendProfile>>> {
    let cluster = selected_active_cluster(&state, &org_ctx, query.cluster_id).await?;
    let cluster = ensure_starrocks_cluster(cluster)?;
    let frontend = resolve_frontend(&cluster, &state, &query).await?;
    let mut url = frontend_url(&cluster, &frontend, "/proc_profile")?;
    url.query_pairs_mut().append_pair("node", "FE");

    let response = fetch_profile_html(&cluster, url, MAX_PROFILE_LIST_BYTES).await;
    audit_profile_access(&state, &org_ctx, &cluster, &frontend, "list", None, response.is_ok())
        .await?;
    let html = response?;
    Ok(Json(parse_memory_profiles(&html)))
}

#[utoipa::path(
    get,
    path = "/api/clusters/frontends/profiles/file",
    params(
        ("cluster_id" = i64, Query, description = "Active cluster ID"),
        ("name" = String, Query, description = "Discovered frontend name"),
        ("host" = String, Query, description = "Discovered frontend host"),
        ("http_port" = String, Query, description = "Discovered frontend HTTP port"),
        ("filename" = String, Query, description = "Memory profile filename"),
        ("theme" = Option<String>, Query, description = "Validated Stellar viewer theme")
    ),
    responses(
        (status = 200, description = "Untrusted profile HTML returned as plain text", content_type = "text/plain"),
        (status = 400, description = "Invalid profile filename"),
        (status = 404, description = "Profile not found"),
        (status = 502, description = "Frontend request failed")
    ),
    security(("bearer_auth" = [])),
    tag = "Frontends"
)]
#[app_db]
pub async fn get_frontend_profile<DB: crate::db::AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    Query(query): Query<ProfileFileQuery>,
) -> ApiResult<Response> {
    let ProfileFileQuery { cluster_id, name, host, http_port, filename, theme } = query;
    let query = FrontendProfileQuery { cluster_id, name, host, http_port };
    let theme = theme.unwrap_or(ProfileViewerTheme::Cosmic);
    let cluster = selected_active_cluster(&state, &org_ctx, query.cluster_id).await?;
    let cluster = ensure_starrocks_cluster(cluster)?;
    let frontend = resolve_frontend(&cluster, &state, &query).await?;
    if memory_profile_timestamp(&filename).is_none() {
        audit_profile_access(&state, &org_ctx, &cluster, &frontend, "view", Some(&filename), false)
            .await?;
        return Err(ApiError::invalid_data("Invalid memory profile filename"));
    }
    let mut url = frontend_url(&cluster, &frontend, "/proc_profile/file")?;
    url.query_pairs_mut().append_pair("filename", &filename);

    let response = fetch_profile_html(&cluster, url, MAX_PROFILE_HTML_BYTES)
        .await
        .and_then(|html| isolate_profile_html_for_theme(html, theme));
    audit_profile_access(
        &state,
        &org_ctx,
        &cluster,
        &frontend,
        "view",
        Some(&filename),
        response.is_ok(),
    )
    .await?;
    let html = response?;
    Response::builder()
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(html))
        .map_err(|_| ApiError::internal_error("Failed to build profile response"))
}

#[derive(Debug, Deserialize)]
pub struct ProfileFileQuery {
    cluster_id: i64,
    name: String,
    host: String,
    http_port: String,
    filename: String,
    pub(crate) theme: Option<ProfileViewerTheme>,
}

#[app_db]
async fn active_cluster<DB: crate::db::AppDb>(
    state: &AppState<DB>,
    org_ctx: &OrgContext,
) -> ApiResult<crate::models::Cluster> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    Ok(cluster)
}

#[app_db]
async fn selected_active_cluster<DB: crate::db::AppDb>(
    state: &AppState<DB>,
    org_ctx: &OrgContext,
    cluster_id: i64,
) -> ApiResult<crate::models::Cluster> {
    let cluster = state.cluster_service.get_cluster(cluster_id).await?;
    validate_selected_cluster(&cluster, org_ctx)?;
    Ok(cluster)
}

pub(crate) fn validate_selected_cluster(
    cluster: &crate::models::Cluster,
    org_ctx: &OrgContext,
) -> ApiResult<()> {
    if !cluster.is_active
        || (!org_ctx.is_super_admin
            && (org_ctx.organization_id.is_none()
                || cluster.organization_id != org_ctx.organization_id))
    {
        return Err(ApiError::not_found("Active cluster not found"));
    }
    Ok(())
}

fn ensure_starrocks_cluster(cluster: crate::models::Cluster) -> ApiResult<crate::models::Cluster> {
    if cluster.is_doris() {
        return Err(ApiError::not_implemented(
            "FE memory profiles are supported for StarRocks clusters only",
        ));
    }
    Ok(cluster)
}

async fn resolve_frontend<DB: crate::db::AppDb>(
    cluster: &crate::models::Cluster,
    state: &AppState<DB>,
    query: &FrontendProfileQuery,
) -> ApiResult<Frontend> {
    if query.cluster_id != cluster.id {
        return Err(ApiError::not_found("Active cluster changed; refresh the frontend list"));
    }
    let requested_port = query
        .http_port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| ApiError::invalid_data("Invalid frontend HTTP port"))?;
    let client = StarRocksClient::new(cluster.clone(), state.mysql_pool_manager.clone());
    client
        .get_frontends()
        .await?
        .into_iter()
        .find(|frontend| {
            frontend.name == query.name
                && frontend.host == query.host
                && frontend.http_port.parse::<u16>().ok() == Some(requested_port)
        })
        .ok_or_else(|| ApiError::not_found("Frontend is no longer present in the active cluster"))
}

pub(crate) fn frontend_url(
    cluster: &crate::models::Cluster,
    frontend: &Frontend,
    path: &str,
) -> ApiResult<reqwest::Url> {
    let host = frontend.host.trim();
    let port = frontend
        .http_port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| ApiError::invalid_data("Invalid discovered frontend HTTP port"))?;
    if host.is_empty() || host != frontend.host {
        return Err(ApiError::invalid_data("Invalid discovered frontend host"));
    }

    let authority_host = match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(address)) => format!("[{address}]"),
        _ => host.to_string(),
    };
    let scheme = if cluster.enable_ssl { "https" } else { "http" };
    let mut url = reqwest::Url::parse(&format!("{scheme}://{authority_host}:{port}/"))
        .map_err(|_| ApiError::invalid_data("Invalid discovered frontend host"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || !url
            .host_str()
            .is_some_and(|value| value.eq_ignore_ascii_case(&authority_host))
        || url.port_or_known_default() != Some(port)
    {
        return Err(ApiError::invalid_data("Invalid discovered frontend authority"));
    }
    url.set_path(path);
    Ok(url)
}

#[cfg(test)]
pub(crate) fn isolate_profile_html(html: String) -> ApiResult<String> {
    isolate_profile_html_for_theme(html, ProfileViewerTheme::Cosmic)
}

pub(crate) fn isolate_profile_html_for_theme(
    mut html: String,
    theme: ProfileViewerTheme,
) -> ApiResult<String> {
    let head_start = find_html_tag_start(&html, "<head", 0)
        .ok_or_else(|| ApiError::bad_gateway("StarRocks profile HTML has no head element"))?;
    let head_end = head_start + "<head".len();
    if html.as_bytes().get(head_end) != Some(&b'>') {
        return Err(ApiError::bad_gateway(
            "StarRocks profile HTML has an unsupported head element",
        ));
    }
    if !has_profile_html_preamble(&html[..head_start]) {
        return Err(ApiError::bad_gateway(
            "StarRocks profile HTML has unsupported content before its head element",
        ));
    }
    if let Some(link_start) = html.find(PROFILE_CREDIT_LINK) {
        html.replace_range(link_start..link_start + PROFILE_CREDIT_LINK.len(), "<a>");
    }
    let script_start = find_html_tag_start(&html, "<script", head_end + 1)
        .ok_or_else(|| ApiError::bad_gateway("StarRocks profile HTML has no inline renderer"))?;
    let markup = &html.as_bytes()[..script_start];
    if contains_ascii_case_insensitive(markup, b"http-equiv")
        || contains_ascii_case_insensitive(markup, b"href")
    {
        return Err(ApiError::bad_gateway(
            "StarRocks profile HTML contains an unsupported navigation directive",
        ));
    }
    let script_open_end = html[script_start..]
        .find('>')
        .map(|offset| script_start + offset)
        .ok_or_else(|| {
            ApiError::bad_gateway("StarRocks profile HTML has an invalid script element")
        })?;
    if !html[script_start..=script_open_end].eq_ignore_ascii_case("<script>") {
        return Err(ApiError::bad_gateway(
            "StarRocks profile HTML uses an unsupported script element",
        ));
    }
    let script_close =
        find_html_tag_start(&html, "</script", script_open_end + 1).ok_or_else(|| {
            ApiError::bad_gateway("StarRocks profile HTML has an unterminated renderer")
        })?;
    let script_close_end = html[script_close..]
        .find('>')
        .map(|offset| script_close + offset)
        .ok_or_else(|| ApiError::bad_gateway("StarRocks profile HTML has an invalid renderer"))?;
    if !html[script_close..=script_close_end].eq_ignore_ascii_case("</script>") {
        return Err(ApiError::bad_gateway("StarRocks profile HTML has an invalid renderer"));
    }
    if find_html_tag_start(&html, "<script", script_close_end + 1).is_some()
        || find_html_tag_start(&html, "</script", script_close_end + 1).is_some()
    {
        return Err(ApiError::bad_gateway(
            "StarRocks profile HTML contains unsupported script elements",
        ));
    }
    if !html[script_close_end + 1..]
        .trim()
        .eq_ignore_ascii_case("</body></html>")
    {
        return Err(ApiError::bad_gateway(
            "StarRocks profile HTML has unsupported trailing content",
        ));
    }

    let script = &html[script_open_end + 1..script_close];
    if !is_supported_profile_renderer(script) {
        return Err(ApiError::bad_gateway(
            "StarRocks profile HTML uses an unsupported or unsafe renderer",
        ));
    }
    let renderer = adapt_profile_renderer(script, theme)?;
    html.replace_range(script_open_end + 1..script_close, &renderer);
    html.insert_str(script_start, PROFILE_SEARCH_DIALOG);
    let script_hash =
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha256::digest(renderer.as_bytes()));
    let policy = format!(
        "default-src 'none'; script-src 'sha256-{script_hash}'; style-src 'unsafe-inline'; img-src 'none'; connect-src 'none'; font-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'; media-src 'none'; worker-src 'none'"
    );
    html.insert_str(
        head_end + 1,
        &format!(
            "<meta http-equiv=\"Content-Security-Policy\" content=\"{policy}\"><style>{}{PROFILE_VIEWER_STYLE}</style>",
            theme.style_tokens(),
        ),
    );
    Ok(html)
}

fn adapt_profile_renderer(script: &str, theme: ProfileViewerTheme) -> ApiResult<String> {
    // The reviewed renderer draws the root at the bottom and sizes its canvas only once.
    // Adapt only its fixed source fragments after the renderer and numeric data were verified.
    let renderer = script.replacen("let inverted = false;", "let inverted = true;", 1);
    if renderer == script {
        return Err(ApiError::bad_gateway("StarRocks profile renderer cannot be oriented"));
    }
    let resized = renderer.replacen(PROFILE_CANVAS_SETUP, PROFILE_CANVAS_SETUP_RESIZABLE, 1);
    if resized == renderer {
        return Err(ApiError::bad_gateway("StarRocks profile renderer cannot be resized"));
    }
    let searched = resized.replacen(PROFILE_SEARCH_PROMPT, PROFILE_SEARCH_PROMPT_REPLACEMENT, 1);
    if searched == resized {
        return Err(ApiError::bad_gateway("StarRocks profile renderer cannot be searched"));
    }
    if !theme.uses_dark_flame_palette() {
        return Ok(format!("{searched}{PROFILE_SEARCH_BEHAVIOR}"));
    }
    let themed = searched.replacen(PROFILE_CANVAS_BACKGROUND, PROFILE_CANVAS_STELLAR_BACKGROUND, 1);
    if themed == searched {
        return Err(ApiError::bad_gateway("StarRocks profile renderer cannot be themed"));
    }
    let palette = themed.replacen(PROFILE_FLAME_PALETTE, theme.flame_palette(), 1);
    if palette == themed {
        return Err(ApiError::bad_gateway("StarRocks profile renderer cannot be recolored"));
    }
    let text = palette.replacen(PROFILE_FLAME_TEXT_COLOR, PROFILE_STELLAR_FLAME_TEXT_COLOR, 1);
    if text == palette {
        return Err(ApiError::bad_gateway("StarRocks profile renderer cannot be recolored"));
    }
    let overlay =
        text.replacen(PROFILE_FLAME_PARENT_OVERLAY, PROFILE_STELLAR_FLAME_PARENT_OVERLAY, 1);
    if overlay == text {
        return Err(ApiError::bad_gateway("StarRocks profile renderer cannot be recolored"));
    }
    let highlighted =
        overlay.replacen(PROFILE_FLAME_SEARCH_HIGHLIGHT, PROFILE_STELLAR_FLAME_SEARCH_HIGHLIGHT, 1);
    if highlighted == overlay {
        return Err(ApiError::bad_gateway("StarRocks profile renderer cannot be recolored"));
    }
    Ok(format!("{highlighted}{PROFILE_SEARCH_BEHAVIOR}"))
}

fn has_profile_html_preamble(preamble: &str) -> bool {
    let preamble = preamble.trim();
    let Some(rest) = preamble.get("<!doctype html>".len()..) else {
        return false;
    };
    if !preamble[.."<!doctype html>".len()].eq_ignore_ascii_case("<!doctype html>") {
        return false;
    }
    let rest = rest.trim();
    let Some(tag_end) = rest.find('>') else {
        return false;
    };
    let tag = &rest[..=tag_end];
    ["<html>", "<html lang='en'>", "<html lang=\"en\">"]
        .iter()
        .any(|allowed| tag.eq_ignore_ascii_case(allowed))
        && rest[tag_end + 1..].trim().is_empty()
}

fn is_supported_profile_renderer(script: &str) -> bool {
    let Some(data_start) = script.find("const cpool = [") else {
        return false;
    };
    // The flame graph depth changes per capture; keep the rest of the renderer byte-exact.
    let mut renderer = script[..data_start].to_string();
    let Some(levels_start) = renderer.find("const levels = Array(") else {
        return false;
    };
    let levels_start = levels_start + "const levels = Array(".len();
    let Some(levels_end) = renderer[levels_start..]
        .find(')')
        .map(|end| levels_start + end)
    else {
        return false;
    };
    let levels = &renderer[levels_start..levels_end];
    if !levels.as_bytes().iter().all(u8::is_ascii_digit)
        || !levels
            .parse::<u16>()
            .is_ok_and(|value| (1..=1024).contains(&value))
    {
        return false;
    }
    renderer.replace_range(levels_start..levels_end, "3");
    let renderer_prefix_hash: [u8; 32] = sha2::Sha256::digest(renderer.as_bytes()).into();
    if renderer_prefix_hash != PROFILE_RENDERER_PREFIX_SHA256 {
        return false;
    }

    static PROFILE_DATA_PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PROFILE_DATA_PATTERN
        .get_or_init(|| {
            regex::Regex::new(
                r"(?s)^const cpool = \[\s*'(?:\\.|[^'\\\r\n])*'(?:\s*,\s*'(?:\\.|[^'\\\r\n])*')*\s*,?\s*\];\s*unpack\(cpool\);\s*(?:[nuf]\(\s*-?\d+(?:\.\d+)?(?:\s*,\s*-?\d+(?:\.\d+)?){0,6}\s*\)\s*;?\s*)*search\(\);\s*$",
            )
            .expect("valid profile data pattern")
        })
        .is_match(&script[data_start..])
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|candidate| candidate.eq_ignore_ascii_case(needle))
}

fn find_html_tag_start(html: &str, tag: &str, from: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let tag = tag.as_bytes();
    bytes
        .get(from..)?
        .windows(tag.len())
        .enumerate()
        .find_map(|(offset, candidate)| {
            if !candidate.eq_ignore_ascii_case(tag) {
                return None;
            }
            let start = from + offset;
            let boundary = *bytes.get(start + tag.len())?;
            (boundary == b'>' || boundary.is_ascii_whitespace()).then_some(start)
        })
}

pub(crate) async fn fetch_profile_html(
    cluster: &crate::models::Cluster,
    url: reqwest::Url,
    max_bytes: usize,
) -> ApiResult<String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ApiError::bad_gateway("Failed to initialize frontend HTTP client"))?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "text/html")
        .basic_auth(&cluster.username, cluster.get_auth_password())
        .send()
        .await
        .map_err(|_| ApiError::bad_gateway("Failed to contact the StarRocks frontend"))?;

    let status = response.status();
    if matches!(status, reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::NOT_FOUND) {
        return Err(ApiError::not_found(
            "Profile endpoint or file is not available on this frontend",
        ));
    }
    if !status.is_success() {
        return Err(ApiError::bad_gateway(format!(
            "StarRocks frontend returned HTTP {}",
            status.as_u16()
        )));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !content_type.is_some_and(|value| value.eq_ignore_ascii_case("text/html")) {
        return Err(ApiError::bad_gateway(
            "StarRocks frontend returned a non-HTML profile response",
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(ApiError::bad_gateway(
            "StarRocks frontend profile response exceeds the size limit",
        ));
    }

    let mut response = response;
    let mut body = Vec::with_capacity(response.content_length().unwrap_or(0) as usize);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::bad_gateway("Failed to read the StarRocks frontend response"))?
    {
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::bad_gateway(
                "StarRocks frontend profile response exceeds the size limit",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body)
        .map_err(|_| ApiError::bad_gateway("StarRocks frontend profile response is not UTF-8"))
}

pub(crate) fn parse_memory_profiles(html: &str) -> Vec<FrontendProfile> {
    let mut profiles = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for link in html.split("/proc_profile/file?filename=").skip(1) {
        let filename = link.split(['"', '&', '<', '>']).next().unwrap_or_default();
        let Some(captured_at) = memory_profile_timestamp(filename) else {
            continue;
        };
        if seen.insert(filename.to_string()) {
            profiles.push(FrontendProfile { filename: filename.to_string(), captured_at });
            if profiles.len() == MAX_PROFILE_COUNT {
                break;
            }
        }
    }
    profiles
}

pub(crate) fn memory_profile_timestamp(filename: &str) -> Option<String> {
    let timestamp = filename
        .strip_prefix("mem-profile-")?
        .strip_suffix(".html.tar.gz")?;
    let bytes = timestamp.as_bytes();
    if bytes.len() != 15
        || bytes[8] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 8 && !byte.is_ascii_digit())
    {
        return None;
    }
    let timestamp = chrono::NaiveDateTime::parse_from_str(timestamp, "%Y%m%d-%H%M%S").ok()?;
    Some(timestamp.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[app_db]
async fn audit_profile_access<DB: AppDb>(
    state: &AppState<DB>,
    org_ctx: &OrgContext,
    cluster: &crate::models::Cluster,
    frontend: &Frontend,
    action: &str,
    filename: Option<&str>,
    succeeded: bool,
) -> ApiResult<()> {
    let outcome = if succeeded { "success" } else { "failure" };
    let port = frontend.http_port.parse::<i32>().unwrap_or_default();
    if let Err(error) = crate::services::frontend_profile_audit::record_frontend_profile_access(
        &state.db,
        crate::services::frontend_profile_audit::FrontendProfileAccess {
            user_id: org_ctx.user_id,
            username: &org_ctx.username,
            organization_id: org_ctx.organization_id,
            cluster_id: cluster.id,
            cluster_name: &cluster.name,
            frontend_name: &frontend.name,
            frontend_host: &frontend.host,
            http_port: port,
            action,
            profile_filename: filename,
            outcome,
        },
    )
    .await
    {
        tracing::warn!("Failed to persist FE profile access audit: {}", error);
        return Err(ApiError::internal_error("Failed to record FE profile access audit"));
    }
    tracing::info!(
        target: "audit",
        audit_event = "frontend_profile_access",
        user_id = org_ctx.user_id,
        username = %org_ctx.username,
        organization_id = ?org_ctx.organization_id,
        cluster_id = cluster.id,
        frontend_name = %frontend.name,
        frontend_host = %frontend.host,
        http_port = %frontend.http_port,
        action,
        profile = filename.unwrap_or(""),
        outcome,
        "StarRocks FE profile access"
    );
    Ok(())
}
