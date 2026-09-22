use crate::models::DeploymentMode;
use crate::services::system_function_service::function_supports_mode;

#[test]
fn system_function_mode_gate_matches_node_and_metadata_semantics() {
    for name in ["compactions", "replications", "historical_nodes", "compute_nodes"] {
        assert!(function_supports_mode(name, DeploymentMode::SharedData), "{name}");
        assert!(!function_supports_mode(name, DeploymentMode::SharedNothing), "{name}");
    }

    assert!(function_supports_mode("cluster_balance", DeploymentMode::SharedNothing));
    assert!(!function_supports_mode("cluster_balance", DeploymentMode::SharedData));
    assert!(function_supports_mode("backends", DeploymentMode::SharedNothing));
    assert!(!function_supports_mode("backends", DeploymentMode::SharedData));

    assert!(function_supports_mode("warehouses", DeploymentMode::SharedNothing));
    assert!(function_supports_mode("warehouses", DeploymentMode::SharedData));
    assert!(function_supports_mode("colocation_group", DeploymentMode::SharedNothing));
    assert!(function_supports_mode("colocation_group", DeploymentMode::SharedData));
}
