use chrono::Utc;

use crate::{
    handlers::backend::{
        BackendDiagnosticEndpoint, backend_url, parse_blocking_drivers_summary,
        parse_compaction_summary, parse_data_cache_summary, parse_memory_summary,
    },
    middleware::permission_extractor::extract_permission,
    models::{Backend, Cluster, ClusterType, DeploymentMode},
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

fn backend(host: &str, port: &str) -> Backend {
    serde_json::from_value(serde_json::json!({
        "BackendId": "42",
        "IP": host,
        "HttpPort": port
    }))
    .expect("backend")
}

#[test]
fn backend_diagnostic_urls_use_only_discovered_authority_and_fixed_paths() {
    let url = backend_url(
        &cluster(false),
        &backend("127.0.0.1", "8040"),
        BackendDiagnosticEndpoint::Memory,
    )
    .expect("IPv4 backend URL");
    assert_eq!(url.as_str(), "http://127.0.0.1:8040/metrics/memory");

    let url = backend_url(
        &cluster(true),
        &backend("2001:db8::1", "8040"),
        BackendDiagnosticEndpoint::BlockingDrivers,
    )
    .expect("IPv6 backend URL");
    assert_eq!(url.as_str(), "https://[2001:db8::1]:8040/api/pipeline_blocking_drivers/stat");
    assert!(
        backend_url(
            &cluster(false),
            &backend("127.0.0.1@evil", "8040"),
            BackendDiagnosticEndpoint::DataCache,
        )
        .is_err()
    );
}

#[test]
fn backend_diagnostic_parsers_preserve_engine_percent_units_and_limit_summary_shape() {
    let memory = parse_memory_summary(serde_json::json!([
        {
            "name": "process",
            "size": "1000",
            "child": [
                { "name": "small", "size": "20", "child": [] },
                { "name": "large", "size": "400", "child": [
                    { "name": "nested", "size": "500", "child": [] }
                ] }
            ]
        },
        { "name": "metadata", "size": "100", "child": [] },
        { "name": "update", "size": "50", "child": [] }
    ]))
    .expect("memory summary");
    assert_eq!(memory.process_bytes, 1000);
    assert_eq!(memory.metadata_bytes, Some(100));
    assert_eq!(memory.top_trackers[0].name, "nested");
    assert_eq!(memory.top_trackers[0].percent_of_process, 50.0);

    let cache = parse_data_cache_summary(serde_json::json!({
        "block_cache_hit_rate": 88.5,
        "block_cache_hit_rate_last_minute": 90.25,
        "page_cache_hit_rate": 72.0,
        "page_cache_hit_rate_last_minute": 73.5
    }))
    .expect("cache summary");
    assert_eq!(cache.block_hit_rate_last_minute, Some(90.25));
    assert_eq!(cache.page_hit_rate, Some(72.0));

    let blocking = parse_blocking_drivers_summary(serde_json::json!({
        "queries_in_workgroup": [{
            "query_id": "q1",
            "fragments": [{
                "fragment_id": "f1",
                "fragment_status": "OK",
                "drivers": [{ "driver_id": 7, "state": "BLOCKED", "driver_desc": "ignored" }]
            }]
        }]
    }))
    .expect("blocking summary");
    assert_eq!(blocking.query_count, 1);
    assert_eq!(blocking.driver_count, 1);
    assert_eq!(blocking.drivers[0].state, "BLOCKED");

    let compaction = parse_compaction_summary(serde_json::json!({
        "max_task_num": 8,
        "running_task_num": 2,
        "base_task_num": 1,
        "cumulative_task_num": 1,
        "candidate_num": 9,
        "tablet_num": 2
    }))
    .expect("compaction summary");
    assert_eq!(compaction.running_task_num, 2);
    assert_eq!(compaction.candidate_num, 9);
}

#[test]
fn backend_diagnostic_route_requires_dedicated_permission() {
    assert_eq!(
        extract_permission("GET", "/api/clusters/backends/diagnostics"),
        Some(("clusters".into(), "backends:diagnose".into()))
    );
    assert_eq!(extract_permission("POST", "/api/clusters/backends/diagnostics"), None);
}
