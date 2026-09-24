use chrono::Utc;

use crate::{
    handlers::agent_chat::validate_chat_cluster_access,
    middleware::OrgContext,
    models::{Cluster, ClusterType, DeploymentMode},
};

fn cluster(organization_id: Option<i64>) -> Cluster {
    Cluster {
        id: 1,
        name: "test".into(),
        description: None,
        fe_host: "127.0.0.1".into(),
        fe_http_port: 8030,
        fe_query_port: 9030,
        username: "root".into(),
        password_encrypted: String::new(),
        enable_ssl: false,
        connection_timeout: 30,
        tags: None,
        catalog: "default_catalog".into(),
        is_active: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        created_by: None,
        organization_id,
        deployment_mode: DeploymentMode::default(),
        cluster_type: ClusterType::StarRocks,
        admin_user: None,
        admin_password_encrypted: None,
    }
}

fn org_context(organization_id: Option<i64>, is_super_admin: bool) -> OrgContext {
    OrgContext { user_id: 1, username: "operator".into(), organization_id, is_super_admin }
}

#[test]
fn explicit_chat_cluster_is_scoped_to_the_requesting_organization() {
    assert!(validate_chat_cluster_access(&cluster(Some(7)), &org_context(Some(7), false)).is_ok());
    assert!(validate_chat_cluster_access(&cluster(Some(8)), &org_context(Some(7), false)).is_err());
    assert!(validate_chat_cluster_access(&cluster(Some(8)), &org_context(Some(7), true)).is_ok());
}
