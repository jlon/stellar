use crate::services::metrics_collector_service::{DISK_CRITICAL_PCT, DISK_WARNING_PCT};
use crate::services::overview_service::{HealthStatus, node_disk_pressure};

/// 平均正常但某节点将写满：必须报出。否则平台会在节点要炸时说"一切正常"。
#[test]
fn reports_hot_node_even_when_cluster_average_is_low() {
    let (level, message) = node_disk_pressure(true, 95.2, 54.5).expect("hot node must alert");

    assert_eq!(level, HealthStatus::Critical);
    // 两个数都要给，判断依据不隐藏
    assert!(message.contains("95.2"), "missing hot node value: {message}");
    assert!(message.contains("54.5"), "missing cluster average: {message}");
}

/// 均匀高水位是整体容量问题，不重复报成单点分布问题。
#[test]
fn uniform_high_watermark_is_not_single_node_pressure() {
    assert_eq!(node_disk_pressure(true, 85.0, 85.0), None);
    assert_eq!(node_disk_pressure(true, DISK_CRITICAL_PCT + 5.0, DISK_CRITICAL_PCT), None);
}

/// 低于阈值不告警；恰好偏差不足 15 个百分点也不判为倾斜。
#[test]
fn stays_quiet_below_thresholds_or_without_skew() {
    assert_eq!(node_disk_pressure(true, DISK_WARNING_PCT - 1.0, 40.0), None);
    assert_eq!(node_disk_pressure(true, 88.0, 80.0), None); // 差距 8 > 但不足 15
}

/// shared-data 的本地盘是 Data Cache 配额（写满为 LRU 稳态），不做该判定。
#[test]
fn skips_shared_data_clusters() {
    assert_eq!(node_disk_pressure(false, 99.0, 20.0), None);
}

/// 预警档：单点超预警阈值且明显不均时给 Warning，不越级到 Critical。
#[test]
fn warning_tier_below_critical_threshold() {
    let (level, _) = node_disk_pressure(true, DISK_WARNING_PCT + 3.0, 10.0).expect("alert");
    assert_eq!(level, HealthStatus::Warning);
}
