use crate::{
    handlers::frontend::{
        ProfileViewerTheme, fetch_profile_html, frontend_url, isolate_profile_html,
        isolate_profile_html_for_theme, memory_profile_timestamp, parse_memory_profiles,
        validate_selected_cluster,
    },
    middleware::OrgContext,
    middleware::permission_extractor::extract_permission,
    models::{Cluster, ClusterType, DeploymentMode, Frontend},
    services::{
        ClusterService, MySQLPoolManager,
        frontend_profile_audit::{FrontendProfileAccess, record_frontend_profile_access},
    },
};
use base64::Engine as _;
use chrono::Utc;
use sha2::Digest as _;
use sqlx::sqlite::SqlitePoolOptions;
use std::{borrow::Cow, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

fn cluster(enable_ssl: bool) -> Cluster {
    Cluster {
        id: 1,
        name: "test".into(),
        description: None,
        fe_host: "127.0.0.1".into(),
        fe_http_port: 8030,
        fe_query_port: 9030,
        username: "root".into(),
        password_encrypted: String::new(),
        enable_ssl,
        connection_timeout: 5,
        tags: None,
        catalog: "default_catalog".into(),
        is_active: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        created_by: None,
        organization_id: None,
        deployment_mode: DeploymentMode::SharedNothing,
        cluster_type: ClusterType::StarRocks,
        admin_user: None,
        admin_password_encrypted: None,
    }
}

fn frontend(host: &str, port: &str) -> Frontend {
    Frontend {
        name: "fe_1".into(),
        host: host.into(),
        edit_log_port: "9010".into(),
        http_port: port.into(),
        query_port: "9030".into(),
        rpc_port: "9020".into(),
        role: "FOLLOWER".into(),
        is_master: Some("false".into()),
        cluster_id: "1".into(),
        join: "true".into(),
        alive: "true".into(),
        replayed_journal_id: "10".into(),
        last_heartbeat: "now".into(),
        err_msg: String::new(),
        version: "4.1.4".into(),
        is_helper: Some("false".into()),
        start_time: None,
    }
}

#[test]
fn accepts_only_timestamped_memory_profile_files() {
    assert_eq!(
        memory_profile_timestamp("mem-profile-20260918-123456.html.tar.gz"),
        Some("2026-09-18 12:34:56".into())
    );
    assert!(memory_profile_timestamp("cpu-profile-20260918-123456.html.tar.gz").is_none());
    assert!(memory_profile_timestamp("mem-profile-20260918-12345.html.tar.gz").is_none());
    assert!(memory_profile_timestamp("mem-profile-../../secret.html.tar.gz").is_none());
    assert!(memory_profile_timestamp("mem-profile-20260230-123456.html.tar.gz").is_none());
}

#[test]
fn parses_only_unique_valid_memory_profile_links() {
    let html = r#"
      <a href="/proc_profile/file?filename=mem-profile-20260918-123456.html.tar.gz">View</a>
      <a href="/proc_profile/file?filename=cpu-profile-20260918-123456.html.tar.gz">View</a>
      <a href="/proc_profile/file?filename=mem-profile-../../secret.html.tar.gz">View</a>
      <a href="/proc_profile/file?filename=mem-profile-20260918-123456.html.tar.gz">Duplicate</a>
    "#;
    let profiles = parse_memory_profiles(html);
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].filename, "mem-profile-20260918-123456.html.tar.gz");
    assert_eq!(profiles[0].captured_at, "2026-09-18 12:34:56");

    let many_profiles = (0..101)
        .map(|second| {
            format!(
                "/proc_profile/file?filename=mem-profile-20260918-12{:02}{:02}.html.tar.gz",
                second / 60,
                second % 60
            )
        })
        .collect::<String>();
    assert_eq!(parse_memory_profiles(&many_profiles).len(), 100);
}

#[test]
fn profile_html_applies_hash_csp_and_rejects_unsupported_markup() {
    let html = include_str!("fixtures/async-profiler-4.0.html");
    let secured = isolate_profile_html(html.to_string()).expect("supported async-profiler HTML");
    let policy = secured
        .split("Content-Security-Policy\" content=\"")
        .nth(1)
        .and_then(|value| value.split_once('"').map(|(policy, _)| policy))
        .expect("CSP meta element");
    let script_start = secured.find("<script>").unwrap() + "<script>".len();
    let script_end = secured[script_start..].find("</script>").unwrap() + script_start;
    let expected_hash = base64::engine::general_purpose::STANDARD
        .encode(sha2::Sha256::digest(&secured.as_bytes()[script_start..script_end]));
    assert!(secured.contains("let inverted = true;"));
    assert!(secured.contains("window.addEventListener('resize', function()"));
    assert!(secured.contains("canvas.style.width = '100%';"));
    assert!(secured.contains("c.fillStyle = '#252547';"));
    assert!(secured.contains("[0x6d5bd0, 16, 16, 16]"));
    assert!(secured.contains("c.fillStyle = '#ffffff';"));
    assert!(secured.contains("rgba(255, 255, 255, 0.12)"));
    assert!(secured.contains("'#ff6ee7'"));
    assert!(!secured.contains("[0x50e150, 30, 30, 30]"));
    assert!(!secured.contains("c.fillStyle = '#000000';"));
    assert!(!secured.contains("rgba(255, 255, 255, 0.5)"));
    assert!(!secured.contains("'#ee00ee'"));
    assert!(secured.contains("id=\"stellar-profile-search-dialog\""));
    assert!(secured.contains("#stellar-profile-search-dialog[hidden] { display: none; }"));
    assert!(secured.contains("function openProfileSearch()"));
    assert!(secured.contains("window.addEventListener('keydown', function(event)"));
    assert!(!secured.contains("prompt("));

    assert!(
        secured
            .find("http-equiv=\"Content-Security-Policy\"")
            .unwrap()
            < secured.find("<style>").unwrap()
    );
    assert!(policy.contains(&format!("script-src 'sha256-{expected_hash}'")));
    assert!(policy.contains("connect-src 'none'"));
    assert!(policy.contains("img-src 'none'"));
    assert!(secured.contains("--stellar-canvas: #252547;"));
    assert!(secured.contains("header:last-of-type { display: none !important; }"));
    assert!(
        secured.contains("*::-webkit-scrollbar { width: 5px !important; height: 5px !important; }")
    );
    assert!(!secured.contains("href="));
    let script_policy = policy
        .split(';')
        .find(|directive| directive.trim_start().starts_with("script-src"))
        .expect("script-src directive");
    assert!(!script_policy.contains("unsafe-inline"));

    let light = isolate_profile_html_for_theme(html.to_string(), ProfileViewerTheme::Default)
        .expect("default theme renderer");
    assert!(light.contains("color-scheme: light"));
    assert!(light.contains("--stellar-canvas: #ffffff"));
    assert!(light.contains("[0x50e150, 30, 30, 30]"));
    assert!(light.contains("c.fillStyle = '#000000';"));
    assert!(!light.contains("c.fillStyle = '#252547';"));

    let corporate = isolate_profile_html_for_theme(html.to_string(), ProfileViewerTheme::Corporate)
        .expect("corporate theme renderer");
    assert!(corporate.contains("color-scheme: light"));
    assert!(corporate.contains("[0x50e150, 30, 30, 30]"));

    let dark = isolate_profile_html_for_theme(html.to_string(), ProfileViewerTheme::Dark)
        .expect("dark theme renderer");
    assert!(dark.contains("--stellar-canvas: #222b45"));
    assert!(dark.contains("[0x6868c9, 16, 16, 16]"));
    assert!(dark.contains("c.fillStyle = '#ffffff';"));

    assert!(secured.contains("--stellar-canvas: #252547;"));
    assert!(secured.contains("[0x6d5bd0, 16, 16, 16]"));

    let deep_profile = html
        .replace("Array(3)", "Array(51)")
        .replace("height: 48px", "height: 816px");
    assert!(isolate_profile_html(deep_profile.replace("n(3,435)", "f(3,1,2,3,4,5,6)")).is_ok());
    assert!(isolate_profile_html(html.replace("n(3,435)", "f(3,1,2,alert(1))")).is_err());
    assert!(isolate_profile_html(html.replace("Array(3)", "Array(0)")).is_err());
    assert!(isolate_profile_html(html.replace("Array(3)", "Array(1025)")).is_err());
    assert!(
        isolate_profile_html(html.replace("Array(3)", "Array(3);location='https://example.com'"))
            .is_err()
    );

    let injected_renderer = html.replace("search();", "search();location='https://example.com';");
    assert!(isolate_profile_html(injected_renderer).is_err());
    let injected_link = html.replace(
        "<h1>Allocation profile</h1>",
        "<h1>Allocation profile</h1><a href='https://example.com'>open</a>",
    );
    assert!(isolate_profile_html(injected_link).is_err());
    let malformed_close = html.replace("</script>", "</script data-x='>'>");
    assert!(isolate_profile_html(malformed_close).is_err());
    assert!(
        isolate_profile_html(
            "<!DOCTYPE html><html lang='en'><head data-x='>'><script>safe()</script></head></html>"
                .into()
        )
        .is_err()
    );
    assert!(isolate_profile_html(
        "<!DOCTYPE html><html lang='en'><head><meta http-equiv='refresh' content='0;url=https://example.com'></head><body><script>safe()</script></body></html>".into()
    )
    .is_err());
    assert!(isolate_profile_html(
        "<!DOCTYPE html><html lang='en'><head></head><body><script>safe()</script><meta http-equiv='refresh'></body></html>".into()
    )
    .is_err());
}

#[test]
fn profile_file_query_accepts_only_supported_viewer_themes() {
    let uri: axum::http::Uri = "/api/clusters/frontends/profiles/file?cluster_id=2&name=fe-0&host=10.0.0.8&http_port=8030&filename=mem-profile-20260918-123456.html.tar.gz&theme=corporate"
        .parse()
        .unwrap();
    let query: Result<axum::extract::Query<crate::handlers::frontend::ProfileFileQuery>, _> =
        axum::extract::Query::try_from_uri(&uri);
    assert_eq!(query.unwrap().theme, Some(ProfileViewerTheme::Corporate));

    let invalid: axum::http::Uri = "/api/clusters/frontends/profiles/file?cluster_id=2&name=fe-0&host=10.0.0.8&http_port=8030&filename=mem-profile-20260918-123456.html.tar.gz&theme=solar"
        .parse()
        .unwrap();
    let query: Result<axum::extract::Query<crate::handlers::frontend::ProfileFileQuery>, _> =
        axum::extract::Query::try_from_uri(&invalid);
    assert!(query.is_err());
}

#[tokio::test]
async fn org_scoped_cluster_lookups_do_not_change_the_global_selection() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create test database");
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .expect("run fresh sqlite migrations");
    let insert = |id, name: &'static str, org_id, is_active| {
        sqlx::query(
            "INSERT INTO clusters (id, name, fe_host, fe_http_port, fe_query_port, username, password_encrypted, catalog, deployment_mode, cluster_type, is_active, organization_id) VALUES (?, ?, '127.0.0.1', 8030, 9030, 'root', '', 'default_catalog', 'shared_nothing', 'starrocks', ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(is_active)
        .bind(org_id)
        .execute(&pool)
    };
    insert(1_i64, "org-one-active", 1_i64, true)
        .await
        .expect("insert org one cluster");

    let service = ClusterService::new(pool.clone(), Arc::new(MySQLPoolManager::new()));
    assert_eq!(service.get_active_cluster().await.unwrap().id, 1);

    insert(2_i64, "org-two-active", 2_i64, true)
        .await
        .expect("insert org two active cluster");
    insert(3_i64, "org-two-next", 2_i64, false)
        .await
        .expect("insert org two standby cluster");
    assert_eq!(service.get_active_cluster_by_org(Some(2)).await.unwrap().id, 2);
    assert_eq!(service.cached_active_cluster().unwrap().id, 1);

    service.set_active_cluster(3, false).await.unwrap();
    assert_eq!(service.get_active_cluster_by_org(Some(2)).await.unwrap().id, 3);
    assert_eq!(service.cached_active_cluster().unwrap().id, 1);

    service.set_active_cluster(3, true).await.unwrap();
    assert_eq!(service.get_active_cluster().await.unwrap().id, 3);
}

#[test]
fn selected_active_cluster_is_bound_to_the_user_organization() {
    let mut cluster = cluster(false);
    cluster.organization_id = Some(7);
    let org_admin = OrgContext {
        user_id: 1,
        username: "org-admin".into(),
        organization_id: Some(7),
        is_super_admin: false,
    };
    let other_org_admin = OrgContext { organization_id: Some(8), ..org_admin.clone() };
    let super_admin =
        OrgContext { organization_id: None, is_super_admin: true, ..org_admin.clone() };

    assert!(validate_selected_cluster(&cluster, &org_admin).is_ok());
    assert!(validate_selected_cluster(&cluster, &other_org_admin).is_err());
    assert!(validate_selected_cluster(&cluster, &super_admin).is_ok());

    cluster.is_active = false;
    assert!(validate_selected_cluster(&cluster, &org_admin).is_err());
}

#[test]
fn profile_proxy_url_uses_only_the_discovered_fe_authority() {
    let mut cluster = cluster(false);
    let url = frontend_url(&cluster, &frontend("127.0.0.1", "8030"), "/proc_profile")
        .expect("valid IPv4 FE");
    assert_eq!(url.as_str(), "http://127.0.0.1:8030/proc_profile");

    cluster.enable_ssl = true;
    let url = frontend_url(&cluster, &frontend("2001:db8::1", "8030"), "/proc_profile")
        .expect("valid IPv6 FE");
    assert_eq!(url.as_str(), "https://[2001:db8::1]:8030/proc_profile");
    assert!(frontend_url(&cluster, &frontend("127.0.0.1@evil", "8030"), "/proc_profile").is_err());
    assert!(frontend_url(&cluster, &frontend("127.0.0.1", "0"), "/proc_profile").is_err());
}

#[tokio::test]
async fn profile_audit_migration_seeds_access_permission_and_persists_reads() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create test database");
    let all_migrations = sqlx::migrate!("./migrations/sqlite");
    let profile_migration = all_migrations
        .iter()
        .find(|migration| migration.version == 20260918000000)
        .cloned()
        .expect("frontend profile migration");
    let prior_migrations = sqlx::migrate::Migrator {
        migrations: Cow::Owned(
            all_migrations
                .iter()
                .filter(|migration| migration.version < profile_migration.version)
                .cloned()
                .collect(),
        ),
        ignore_missing: true,
        locking: false,
        no_tx: false,
    };
    prior_migrations
        .run(&pool)
        .await
        .expect("run migrations before frontend profiles");
    let organization_id: i64 =
        sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
            .fetch_one(&pool)
            .await
            .expect("default organization");
    sqlx::query(
        "INSERT INTO roles (code, name, description, is_system, organization_id) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("org_admin_existing")
    .bind("Existing organization admin")
    .bind("")
    .bind(false)
    .bind(organization_id)
    .execute(&pool)
    .await
    .expect("create existing organization admin role");
    let profile_migrations = sqlx::migrate::Migrator {
        migrations: Cow::Owned(vec![profile_migration]),
        ignore_missing: true,
        locking: false,
        no_tx: false,
    };
    profile_migrations
        .run(&pool)
        .await
        .expect("run frontend profile migration");

    let permission_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM permissions WHERE code = 'api:clusters:frontends:diagnose'",
    )
    .fetch_one(&pool)
    .await
    .expect("query diagnostic permission");
    assert_eq!(permission_exists, 1);
    let permitted_roles: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM role_permissions rp \
         JOIN roles r ON r.id = rp.role_id \
         JOIN permissions p ON p.id = rp.permission_id \
         WHERE p.code = 'api:clusters:frontends:diagnose' \
           AND r.code IN ('admin', 'super_admin', 'org_admin_existing')",
    )
    .fetch_one(&pool)
    .await
    .expect("query diagnostic role grants");
    assert_eq!(permitted_roles, 3);

    record_frontend_profile_access(
        &pool,
        FrontendProfileAccess {
            user_id: 3,
            username: "operator",
            organization_id: Some(9),
            cluster_id: 1,
            cluster_name: "test-cluster",
            frontend_name: "fe_1",
            frontend_host: "127.0.0.1",
            http_port: 8030,
            action: "view",
            profile_filename: Some("mem-profile-20260918-123456.html.tar.gz"),
            outcome: "success",
        },
    )
    .await
    .expect("persist profile access audit");

    let audit: (String, String, String) = sqlx::query_as(
        "SELECT username, profile_filename, outcome FROM frontend_profile_access_logs",
    )
    .fetch_one(&pool)
    .await
    .expect("query profile access audit");
    assert_eq!(
        audit,
        ("operator".into(), "mem-profile-20260918-123456.html.tar.gz".into(), "success".into())
    );
}

async fn mock_frontend(response: String) -> (reqwest::Url, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock FE");
    let address = listener.local_addr().expect("mock FE address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept FE request");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stream.read(&mut buffer).await.expect("read FE request");
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write FE response");
        String::from_utf8(request).expect("HTTP request is UTF-8")
    });
    (reqwest::Url::parse(&format!("http://{address}/proc_profile")).unwrap(), task)
}

fn http_response(status: &str, headers: &str, body: &str) -> String {
    format!("HTTP/1.1 {status}\r\n{headers}\r\n{body}")
}

#[tokio::test]
async fn profile_fetch_uses_cluster_credentials_and_accepts_html() {
    let mut cluster = cluster(false);
    cluster.username = "profile-reader".into();
    cluster.password_encrypted = "secret".into();
    let (url, server) = mock_frontend(http_response(
        "200 OK",
        "Content-Type: text/html; charset=utf-8\r\nContent-Length: 15\r\nConnection: close\r\n",
        "<html>ok</html>",
    ))
    .await;

    assert_eq!(fetch_profile_html(&cluster, url, 32).await.unwrap(), "<html>ok</html>");
    let request = server.await.expect("mock FE task");
    let authorization = request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization")
                .then_some(value.trim())
        })
        .expect("FE Basic Auth header");
    assert_eq!(authorization, "Basic cHJvZmlsZS1yZWFkZXI6c2VjcmV0");
}

#[tokio::test]
async fn profile_fetch_rejects_redirects_auth_errors_and_non_html() {
    let cluster = cluster(false);
    let cases = [
        http_response(
            "302 Found",
            "Location: http://example.invalid/\r\nContent-Length: 0\r\n",
            "",
        ),
        http_response("401 Unauthorized", "Content-Length: 0\r\n", ""),
        http_response(
            "200 OK",
            "Content-Type: application/octet-stream\r\nContent-Length: 1\r\n",
            "x",
        ),
    ];
    for response in cases {
        let (url, server) = mock_frontend(response).await;
        assert!(matches!(
            fetch_profile_html(&cluster, url, 8).await,
            Err(crate::utils::ApiError::BadGateway(_))
        ));
        server.await.expect("mock FE task");
    }

    let (url, server) =
        mock_frontend(http_response("400 Bad Request", "Content-Length: 0\r\n", "")).await;
    assert!(matches!(
        fetch_profile_html(&cluster, url, 8).await,
        Err(crate::utils::ApiError::ResourceNotFound(_))
    ));
    server.await.expect("mock FE task");
}

#[tokio::test]
async fn profile_fetch_enforces_declared_and_streamed_size_limits() {
    let cluster = cluster(false);
    let cases = [
        http_response(
            "200 OK",
            "Content-Type: text/html\r\nContent-Length: 6\r\n",
            "abcdef",
        ),
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n".into(),
    ];
    for response in cases {
        let (url, server) = mock_frontend(response).await;
        assert!(matches!(
            fetch_profile_html(&cluster, url, 5).await,
            Err(crate::utils::ApiError::BadGateway(_))
        ));
        server.await.expect("mock FE task");
    }
}

#[test]
fn frontend_profile_routes_require_the_dedicated_diagnostic_permission() {
    let expected = Some(("clusters".to_string(), "frontends:diagnose".to_string()));
    assert_eq!(extract_permission("GET", "/api/clusters/frontends/profiles"), expected);
    assert_eq!(extract_permission("GET", "/api/clusters/frontends/profiles/file"), expected);
    assert_eq!(extract_permission("POST", "/api/clusters/frontends/profiles/file"), None);
}
