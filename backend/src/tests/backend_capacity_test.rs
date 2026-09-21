use crate::models::Backend;
use crate::services::metrics_collector_service::{
    disk_capacity_sample, fill_backend_capacity, should_alert_disk_usage,
};

#[test]
fn fills_shared_data_compute_node_cache_capacity_when_storage_columns_are_missing() {
    let mut node: Backend = serde_json::from_value(serde_json::json!({
        "ComputeNodeId": "1",
        "IP": "cn-0",
        "DataCacheMetrics": "Status: Normal, DiskUsage: 7.5TB/12.6TB, MemUsage: 0B/0B"
    }))
    .expect("compute node");

    fill_backend_capacity(&mut node, true);

    assert_eq!(node.data_used_capacity, "7.5 TB");
    assert_eq!(node.total_capacity, "12.6 TB");
    assert_eq!(node.used_pct, "59.5%");
}

#[test]
fn repairs_zero_used_capacity_from_storage_usage_rate() {
    let mut node: Backend = serde_json::from_value(serde_json::json!({
        "BackendId": "1",
        "DataUsedCapacity": "0.000 B",
        "TotalCapacity": "13.860 TB",
        "UsedPct": "5.04%",
        "DataCacheMetrics": "Status: Normal, DiskUsage: 9.0TB/9.0TB, MemUsage: 0B/0B"
    }))
    .expect("backend");

    fill_backend_capacity(&mut node, false);

    assert_eq!(node.data_used_capacity, "715.3 GB");
    assert_eq!(node.total_capacity, "13.860 TB");
    assert_eq!(node.used_pct, "5.04%");
}

#[test]
fn preserves_reported_data_capacity() {
    let mut node: Backend = serde_json::from_value(serde_json::json!({
        "BackendId": "1",
        "DataUsedCapacity": "1.0 TB",
        "TotalCapacity": "10 TB",
        "UsedPct": "50%"
    }))
    .expect("backend");

    fill_backend_capacity(&mut node, false);

    assert_eq!(node.data_used_capacity, "1.0 TB");
}

#[test]
fn shared_nothing_cache_does_not_fill_data_disk_capacity() {
    let mut node: Backend = serde_json::from_value(serde_json::json!({
        "BackendId": "1",
        "DataCacheMetrics": "Status: Normal, DiskUsage: 7.5TB/12.6TB, MemUsage: 0B/0B"
    }))
    .expect("backend");

    fill_backend_capacity(&mut node, false);

    assert!(node.data_used_capacity.is_empty());
    assert!(node.total_capacity.is_empty());
    assert!(node.used_pct.is_empty());
}

#[test]
fn capacity_sample_prefers_used_pct_over_hottest_disk() {
    let node: Backend = serde_json::from_value(serde_json::json!({
        "BackendId": "1",
        "TotalCapacity": "10 TB",
        "UsedPct": "5%",
        "MaxDiskUsedPct": "90%"
    }))
    .expect("backend");

    let sample = disk_capacity_sample(&node).expect("capacity sample");

    assert_eq!(sample.0, 5.0);
    assert_eq!(sample.2, sample.1 / 20);
}

#[test]
fn shared_data_cache_usage_never_triggers_a_disk_alert() {
    assert!(!should_alert_disk_usage(true, 1, 100.0));
    assert!(!should_alert_disk_usage(false, 1, 80.0));
    assert!(should_alert_disk_usage(false, 1, 80.1));
}
