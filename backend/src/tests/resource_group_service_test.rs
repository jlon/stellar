use crate::config::AuditLogConfig;
use crate::services::ResourceGroupService;

#[test]
fn resource_usage_uses_configured_audit_table() {
    let audit_config =
        AuditLogConfig { database: "custom_audit".to_string(), table: "query_events".to_string() };

    assert_eq!(ResourceGroupService::audit_table(&audit_config), "custom_audit.query_events");
}
