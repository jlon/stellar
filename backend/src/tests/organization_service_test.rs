// Organization service multi-tenant tests

use crate::models::{CreateOrganizationRequest, UpdateOrganizationRequest};
use crate::services::organization_service::OrganizationService;
use crate::tests::common::{
    assign_user_to_organization, create_test_db, create_test_user_with_org,
    setup_multi_tenant_test_data,
};
use sqlx::SqlitePool;

#[tokio::test]
async fn test_organization_crud_operations() {
    let pool = create_test_db().await;
    let org_service = OrganizationService::new(pool.clone());


    let create_req = CreateOrganizationRequest {
        code: "test_org".to_string(),
        name: "Test Organization".to_string(),
        description: Some("Test organization description".to_string()),
        admin_username: Some("test_org_admin".to_string()),
        admin_password: Some("password123".to_string()),
        admin_email: Some("test_org_admin@example.com".to_string()),
        admin_user_id: None,
    };

    let created_org = org_service
        .create_organization(create_req)
        .await
        .expect("Failed to create organization");

    assert_eq!(created_org.code, "test_org");
    assert_eq!(created_org.name, "Test Organization");
    assert!(!created_org.is_system);


    let retrieved_org = org_service
        .get_organization(created_org.id, None, true)
        .await
        .expect("Failed to get organization");

    assert_eq!(retrieved_org.id, created_org.id);
    assert_eq!(retrieved_org.code, "test_org");


    let organizations = org_service
        .list_organizations(None, true)
        .await
        .expect("Failed to list organizations");

    assert!(!organizations.is_empty());
    assert!(organizations.iter().any(|org| org.code == "test_org"));


    let update_req = UpdateOrganizationRequest {
        name: Some("Updated Test Organization".to_string()),
        description: Some("Updated description".to_string()),
        admin_user_id: None,
    };

    let updated_org = org_service
        .update_organization(created_org.id, update_req, None, true)
        .await
        .expect("Failed to update organization");

    assert_eq!(updated_org.name, "Updated Test Organization");
    assert_eq!(updated_org.description, Some("Updated description".to_string()));


    cleanup_organization_data(&pool, created_org.id).await;

    org_service
        .delete_organization(created_org.id, None, true)
        .await
        .expect("Failed to delete organization");


    let result = org_service
        .get_organization(created_org.id, None, true)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_organization_with_admin_creation() {
    let pool = create_test_db().await;
    let org_service = OrganizationService::new(pool.clone());


    let create_req = CreateOrganizationRequest {
        code: "org_with_admin".to_string(),
        name: "Organization With Admin".to_string(),
        description: Some("Test organization with admin".to_string()),
        admin_username: Some("org_admin".to_string()),
        admin_password: Some("password123".to_string()),
        admin_email: Some("admin@test.com".to_string()),
        admin_user_id: None,
    };

    let created_org = org_service
        .create_organization(create_req)
        .await
        .expect("Failed to create organization with admin");

    assert_eq!(created_org.code, "org_with_admin");


    let users =
        sqlx::query_as::<_, (i64, String)>("SELECT id, username FROM users WHERE username = ?")
            .bind("org_admin")
            .fetch_all(&pool)
            .await
            .expect("Failed to fetch admin user");

    assert!(!users.is_empty());
    let (admin_user_id, _) = users[0];


    let user_orgs: Vec<(i64,)> =
        sqlx::query_as("SELECT organization_id FROM user_organizations WHERE user_id = ?")
            .bind(admin_user_id)
            .fetch_all(&pool)
            .await
            .expect("Failed to fetch user organizations");

    assert!(!user_orgs.is_empty());
    assert_eq!(user_orgs[0].0, created_org.id);
}

#[tokio::test]
async fn test_organization_creation_without_admin() {
    let pool = create_test_db().await;
    let org_service = OrganizationService::new(pool.clone());

    let create_req = CreateOrganizationRequest {
        code: "org_without_admin".to_string(),
        name: "Organization Without Admin".to_string(),
        description: Some("Org created without admin".to_string()),
        admin_username: None,
        admin_password: None,
        admin_email: None,
        admin_user_id: None,
    };

    let created_org = org_service
        .create_organization(create_req)
        .await
        .expect("Failed to create organization without admin");


    let (role_id,): (i64,) = sqlx::query_as(
        "SELECT id FROM roles WHERE organization_id = ? AND code LIKE 'org_admin_%' LIMIT 1",
    )
    .bind(created_org.id)
    .fetch_one(&pool)
    .await
    .expect("org_admin role should exist");

    let assignments: Vec<(i64,)> =
        sqlx::query_as("SELECT user_id FROM user_roles WHERE role_id = ?")
            .bind(role_id)
            .fetch_all(&pool)
            .await
            .expect("Failed to fetch assignments");
    assert!(assignments.is_empty(), "No admin should be assigned automatically");
}

#[tokio::test]
async fn test_assign_admin_during_update() {
    let pool = create_test_db().await;
    let org_service = OrganizationService::new(pool.clone());


    let create_req = CreateOrganizationRequest {
        code: "org_update_admin".to_string(),
        name: "Org Update Admin".to_string(),
        description: None,
        admin_username: None,
        admin_password: None,
        admin_email: None,
        admin_user_id: None,
    };
    let org = org_service
        .create_organization(create_req)
        .await
        .expect("Failed to create organization");


    let user_id = create_test_user_with_org(&pool, "deferred_admin", org.id).await;
    assign_user_to_organization(&pool, user_id, org.id).await;


    let update_req =
        UpdateOrganizationRequest { name: None, description: None, admin_user_id: Some(user_id) };

    org_service
        .update_organization(org.id, update_req, None, true)
        .await
        .expect("Failed to update organization admin");

    let (role_id,): (i64,) = sqlx::query_as(
        "SELECT id FROM roles WHERE organization_id = ? AND code LIKE 'org_admin_%' LIMIT 1",
    )
    .bind(org.id)
    .fetch_one(&pool)
    .await
    .expect("org_admin role missing");

    let assignments: Vec<(i64,)> =
        sqlx::query_as("SELECT user_id FROM user_roles WHERE role_id = ?")
            .bind(role_id)
            .fetch_all(&pool)
            .await
            .expect("Failed to fetch user roles");

    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].0, user_id);
}

#[tokio::test]
async fn test_organization_access_control() {
    let pool = create_test_db().await;
    let org_service = OrganizationService::new(pool.clone());

    let test_data = setup_multi_tenant_test_data(&pool).await;


    let organizations = org_service
        .list_organizations(None, true)
        .await
        .expect("Super admin should list all organizations");

    assert!(organizations.len() >= 2);


    let org1_organizations = org_service
        .list_organizations(Some(test_data.org1_id), false)
        .await
        .expect("Org admin should list their organization");

    assert!(!org1_organizations.is_empty());


    let create_req = CreateOrganizationRequest {
        code: "unauthorized_org".to_string(),
        name: "Unauthorized Organization".to_string(),
        description: None,
        admin_username: Some("unauth_admin".to_string()),
        admin_password: Some("password123".to_string()),
        admin_email: Some("unauth_admin@example.com".to_string()),
        admin_user_id: None,
    };

    let result = org_service.create_organization(create_req).await;
    assert!(result.is_ok(), "Service-level creation does not enforce RBAC; handler is responsible");


    let result = org_service
        .delete_organization(test_data.org2_id, Some(test_data.org1_id), false)
        .await;

    assert!(result.is_err(), "Org admin should not be able to delete other organizations");
}

#[tokio::test]
async fn test_system_organization_protection() {
    let pool = create_test_db().await;
    let org_service = OrganizationService::new(pool.clone());


    let create_req = CreateOrganizationRequest {
        code: "system_org".to_string(),
        name: "System Organization".to_string(),
        description: Some("System organization".to_string()),
        admin_username: Some("system_org_admin".to_string()),
        admin_password: Some("password123".to_string()),
        admin_email: Some("system_org_admin@example.com".to_string()),
        admin_user_id: None,
    };

    let system_org = org_service
        .create_organization(create_req)
        .await
        .expect("Failed to create system organization");


    sqlx::query("UPDATE organizations SET is_system = 1 WHERE id = ?")
        .bind(system_org.id)
        .execute(&pool)
        .await
        .expect("Failed to mark organization as system");


    let result = org_service
        .delete_organization(system_org.id, None, true)
        .await;

    assert!(result.is_err(), "System organization should not be deletable");


    let update_req = UpdateOrganizationRequest {
        name: Some("Updated System Organization".to_string()),
        description: Some("Updated system description".to_string()),
        admin_user_id: None,
    };

    let updated_org = org_service
        .update_organization(system_org.id, update_req, None, true)
        .await
        .expect("System organization should be updatable");

    assert_eq!(updated_org.name, "Updated System Organization");
}

#[tokio::test]
async fn test_organization_filtering() {
    let pool = create_test_db().await;
    let org_service = OrganizationService::new(pool.clone());

    let test_data = setup_multi_tenant_test_data(&pool).await;


    let create_req = CreateOrganizationRequest {
        code: "additional_org".to_string(),
        name: "Additional Organization".to_string(),
        description: Some("Additional test organization".to_string()),
        admin_username: Some("additional_org_admin".to_string()),
        admin_password: Some("password123".to_string()),
        admin_email: Some("additional_org_admin@example.com".to_string()),
        admin_user_id: None,
    };

    let additional_org = org_service
        .create_organization(create_req)
        .await
        .expect("Failed to create additional organization");
    assert_eq!(additional_org.code, "additional_org");


    let all_orgs = org_service
        .list_organizations(None, true)
        .await
        .expect("Super admin should see all organizations");

    assert!(all_orgs.len() >= 3);


    let filtered_orgs = org_service
        .list_organizations(None, false)
        .await
        .expect("Non-super-admin without org context should get empty result");

    assert!(
        filtered_orgs.is_empty(),
        "Should return empty for non-super-admin without organization"
    );


    let filtered_orgs = org_service
        .list_organizations(Some(test_data.org1_id), false)
        .await
        .expect("Non-super-admin with org context should get filtered result");

    assert!(
        filtered_orgs
            .iter()
            .all(|org| org.id == test_data.org1_id || org.code == "org1"),
        "Org-scoped listing should only include the requestor's organization"
    );
}

#[tokio::test]
async fn test_organization_update_validation() {
    let pool = create_test_db().await;
    let org_service = OrganizationService::new(pool.clone());

    let test_data = setup_multi_tenant_test_data(&pool).await;


    let update_req = UpdateOrganizationRequest {
        name: Some("Hijacked Organization".to_string()),
        description: Some("This should not work".to_string()),
        admin_user_id: None,
    };

    let result = org_service
        .update_organization(test_data.org2_id, update_req, Some(test_data.org1_id), false)
        .await;

    assert!(result.is_err(), "Non-super-admin should not update other organizations");


    let update_req = UpdateOrganizationRequest {
        name: Some("Super Admin Updated".to_string()),
        description: Some("Updated by super admin".to_string()),
        admin_user_id: None,
    };

    let updated_org = org_service
        .update_organization(test_data.org1_id, update_req, None, true)
        .await
        .expect("Super admin should update any organization");

    assert_eq!(updated_org.name, "Super Admin Updated");
}

async fn cleanup_organization_data(pool: &SqlitePool, org_id: i64) {
    sqlx::query(
        "DELETE FROM user_roles WHERE user_id IN (SELECT user_id FROM user_organizations WHERE organization_id = ?)",
    )
    .bind(org_id)
    .execute(pool)
    .await
    .ok();

    sqlx::query("DELETE FROM role_permissions WHERE role_id IN (SELECT id FROM roles WHERE organization_id = ?)")
        .bind(org_id)
        .execute(pool)
        .await
        .ok();

    sqlx::query("DELETE FROM user_organizations WHERE organization_id = ?")
        .bind(org_id)
        .execute(pool)
        .await
        .ok();

    sqlx::query("DELETE FROM users WHERE organization_id = ?")
        .bind(org_id)
        .execute(pool)
        .await
        .ok();

    sqlx::query("DELETE FROM clusters WHERE organization_id = ?")
        .bind(org_id)
        .execute(pool)
        .await
        .ok();

    sqlx::query("DELETE FROM roles WHERE organization_id = ?")
        .bind(org_id)
        .execute(pool)
        .await
        .ok();
}
