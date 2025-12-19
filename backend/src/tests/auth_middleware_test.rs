// Integration tests for Auth Middleware with Permission Checking
// Tests different users with different roles and permissions

use crate::middleware::{
    AuthState, auth::auth_middleware, permission_extractor::extract_permission,
};
use crate::services::casbin_service::CasbinService;
use crate::tests::common::{
    assign_role_to_user, create_role, create_test_casbin_service, create_test_db, create_test_user,
    grant_permissions, setup_test_data,
};
use crate::utils::JwtUtil;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
    routing::get,
};
use sqlx::SqlitePool;
use std::sync::Arc;
use tower::util::ServiceExt;

/// Mock handler that returns 200 OK
async fn mock_handler() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap()
}

/// Create test router with auth middleware
fn create_test_router(
    jwt_util: Arc<JwtUtil>,
    casbin_service: Arc<CasbinService>,
    db: SqlitePool,
) -> Router {
    let auth_state = AuthState { jwt_util, casbin_service, db };

    Router::new()
        .route("/api/roles", get(mock_handler))
        .route("/api/roles/1", get(mock_handler))
        .route("/api/roles", axum::routing::post(mock_handler))
        .route("/api/clusters", axum::routing::post(mock_handler))
        .route("/api/clusters/1", axum::routing::put(mock_handler))
        .route("/api/clusters/1", axum::routing::delete(mock_handler))
        .route("/api/auth/permissions", get(mock_handler))
        .route_layer(axum::middleware::from_fn_with_state(auth_state, auth_middleware))
}

/// Generate JWT token for a user
fn generate_token(jwt_util: &JwtUtil, user_id: i64, username: &str) -> String {
    jwt_util
        .generate_token(user_id, username)
        .expect("Failed to generate token")
}

async fn create_menu_only_role(pool: &sqlx::SqlitePool) -> i64 {
    let role_id = create_role(pool, "ops", "Operator", "Operator role", false).await;

    let menu_permission_ids: Vec<i64> =
        sqlx::query_as::<_, (i64,)>("SELECT id FROM permissions WHERE code LIKE 'menu:%'")
            .fetch_all(pool)
            .await
            .expect("Failed to fetch menu permissions")
            .into_iter()
            .map(|(id,)| id)
            .collect();

    grant_permissions(pool, role_id, &menu_permission_ids).await;

    role_id
}

#[tokio::test]
async fn test_admin_user_has_all_permissions() {
    let pool = create_test_db().await;
    let casbin_service = create_test_casbin_service().await;
    let jwt_util = Arc::new(JwtUtil::new("test-secret-key-for-admin-test", "24h"));


    let data = setup_test_data(&pool).await;
    let admin_role_id = data.admin_role_id;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let admin_user_id = create_test_user(&pool, "admin_user").await;
    assign_role_to_user(&pool, admin_user_id, admin_role_id).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let token = generate_token(&jwt_util, admin_user_id, "admin_user");


    let app = create_test_router(jwt_util.clone(), casbin_service.clone(), pool.clone());


    let test_cases = vec![
        ("GET", "/api/roles", true),
        ("POST", "/api/roles", true),
        ("GET", "/api/roles/1", true),
        ("POST", "/api/clusters", true),
        ("PUT", "/api/clusters/1", true),
        ("DELETE", "/api/clusters/1", true),
    ];

    for (method, path, should_allow) in test_cases {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();

        if should_allow {
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Admin should have access to {} {}",
                method,
                path
            );
        } else {
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "Admin should be denied access to {} {}",
                method,
                path
            );
        }
    }
}

#[tokio::test]
async fn test_operator_user_has_limited_permissions() {
    let pool = create_test_db().await;
    let casbin_service = create_test_casbin_service().await;
    let jwt_util = Arc::new(JwtUtil::new("test-secret-key-for-operator-test", "24h"));


    setup_test_data(&pool).await;
    let operator_role_id = create_menu_only_role(&pool).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let operator_user_id = create_test_user(&pool, "operator_user").await;
    assign_role_to_user(&pool, operator_user_id, operator_role_id).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let token = generate_token(&jwt_util, operator_user_id, "operator_user");


    let app = create_test_router(jwt_util.clone(), casbin_service.clone(), pool.clone());



    let test_cases = vec![
        ("GET", "/api/roles", false),
        ("POST", "/api/roles", false),
        ("GET", "/api/roles/1", false),
        ("POST", "/api/clusters", false),
        ("PUT", "/api/clusters/1", false),
        ("DELETE", "/api/clusters/1", false),
        ("GET", "/api/auth/permissions", true),
    ];

    for (method, path, should_allow) in test_cases {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();

        if should_allow {
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Operator should have access to {} {}",
                method,
                path
            );
        } else {
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "Operator should be denied access to {} {}",
                method,
                path
            );
        }
    }
}

#[tokio::test]
async fn test_user_with_no_role_has_no_permissions() {
    let pool = create_test_db().await;
    let casbin_service = create_test_casbin_service().await;
    let jwt_util = Arc::new(JwtUtil::new("test-secret-key-for-no-role-test", "24h"));


    setup_test_data(&pool).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let user_id = create_test_user(&pool, "no_role_user").await;



    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let token = generate_token(&jwt_util, user_id, "no_role_user");


    let app = create_test_router(jwt_util.clone(), casbin_service.clone(), pool.clone());


    let test_cases = vec![
        ("GET", "/api/roles", false),
        ("POST", "/api/roles", false),
        ("GET", "/api/roles/1", false),
        ("POST", "/api/clusters", false),
        ("PUT", "/api/clusters/1", false),
        ("DELETE", "/api/clusters/1", false),
        ("GET", "/api/auth/permissions", true),
    ];

    for (method, path, should_allow) in test_cases {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();

        if should_allow {
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "User without role should have access to {} {}",
                method,
                path
            );
        } else {
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "User without role should be denied access to {} {}",
                method,
                path
            );
        }
    }
}

#[tokio::test]
async fn test_custom_role_with_specific_permissions() {
    let pool = create_test_db().await;
    let casbin_service = create_test_casbin_service().await;
    let jwt_util = Arc::new(JwtUtil::new("test-secret-key-for-custom-role-test", "24h"));


    setup_test_data(&pool).await;


    let custom_role_id: (i64,) = sqlx::query_as(
        "INSERT INTO roles (code, name, description, is_system) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind("custom_viewer")
    .bind("Custom Viewer")
    .bind("Custom viewer role with limited permissions")
    .bind(0)
    .fetch_one(&pool)
    .await
    .expect("Failed to create custom role");

    let custom_role_id = custom_role_id.0;


    let permission_id: (i64,) =
        sqlx::query_as("SELECT id FROM permissions WHERE code = 'api:clusters:create'")
            .fetch_one(&pool)
            .await
            .expect("Failed to find permission");

    let permission_id = permission_id.0;


    sqlx::query("INSERT INTO role_permissions (role_id, permission_id) VALUES (?, ?)")
        .bind(custom_role_id)
        .bind(permission_id)
        .execute(&pool)
        .await
        .expect("Failed to assign permission to custom role");


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let user_id = create_test_user(&pool, "custom_user").await;
    assign_role_to_user(&pool, user_id, custom_role_id).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let token = generate_token(&jwt_util, user_id, "custom_user");


    let app = create_test_router(jwt_util.clone(), casbin_service.clone(), pool.clone());


    let test_cases = vec![
        ("POST", "/api/clusters", true),
        ("GET", "/api/roles", false),
        ("POST", "/api/roles", false),
        ("GET", "/api/roles/1", false),
        ("PUT", "/api/clusters/1", false),
        ("DELETE", "/api/clusters/1", false),
    ];

    for (method, path, should_allow) in test_cases {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();

        if should_allow {
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Custom user should have access to {} {}",
                method,
                path
            );
        } else {
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "Custom user should be denied access to {} {}",
                method,
                path
            );
        }
    }
}

#[tokio::test]
async fn test_multiple_users_different_permissions() {
    let pool = create_test_db().await;
    let casbin_service = create_test_casbin_service().await;
    let jwt_util = Arc::new(JwtUtil::new("test-secret-key-for-multiple-users-test", "24h"));


    let data = setup_test_data(&pool).await;
    let admin_role_id = data.admin_role_id;
    let operator_role_id = create_menu_only_role(&pool).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let admin_user_id = create_test_user(&pool, "admin1").await;
    assign_role_to_user(&pool, admin_user_id, admin_role_id).await;

    let operator_user_id = create_test_user(&pool, "operator1").await;
    assign_role_to_user(&pool, operator_user_id, operator_role_id).await;

    let no_role_user_id = create_test_user(&pool, "norole1").await;



    let existing_assignments: Vec<(i64,)> =
        sqlx::query_as("SELECT role_id FROM user_roles WHERE user_id = ?")
            .bind(no_role_user_id)
            .fetch_all(&pool)
            .await
            .expect("Failed to check user roles");
    assert!(existing_assignments.is_empty(), "No-role user should have no role assignments");



    eprintln!("[TEST] Admin role ID: {}, Operator role ID: {}", admin_role_id, operator_role_id);
    eprintln!(
        "[TEST] Admin user ID: {}, Operator user ID: {}, No-role user ID: {}",
        admin_user_id, operator_user_id, no_role_user_id
    );


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let admin_token = generate_token(&jwt_util, admin_user_id, "admin1");
    let operator_token = generate_token(&jwt_util, operator_user_id, "operator1");
    let no_role_token = generate_token(&jwt_util, no_role_user_id, "norole1");


    let app = create_test_router(jwt_util.clone(), casbin_service.clone(), pool.clone());


    let test_endpoint = "/api/roles";
    let test_method = "POST";


    let admin_req = Request::builder()
        .method(test_method)
        .uri(test_endpoint)
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let admin_response = app.clone().oneshot(admin_req).await.unwrap();
    assert_eq!(
        admin_response.status(),
        StatusCode::OK,
        "Admin user should have access to create roles"
    );


    let operator_req = Request::builder()
        .method(test_method)
        .uri(test_endpoint)
        .header(header::AUTHORIZATION, format!("Bearer {}", operator_token))
        .body(Body::empty())
        .unwrap();

    let operator_response = app.clone().oneshot(operator_req).await.unwrap();
    assert_eq!(
        operator_response.status(),
        StatusCode::UNAUTHORIZED,
        "Operator user should not have access to create roles"
    );


    let no_role_req = Request::builder()
        .method(test_method)
        .uri(test_endpoint)
        .header(header::AUTHORIZATION, format!("Bearer {}", no_role_token))
        .body(Body::empty())
        .unwrap();


    let perm = extract_permission("POST", "/api/roles");
    assert!(
        perm.is_some(),
        "extract_permission should return Some for POST /api/roles, got: {:?}",
        perm
    );
    let (res, act) = perm.as_ref().unwrap();
    assert_eq!(res, "roles", "Resource should be 'roles'");
    assert_eq!(act, "create", "Action should be 'create'");



    let casbin_result_before = casbin_service
        .enforce(no_role_user_id, "roles", "create")
        .await;
    eprintln!(
        "[DEBUG] CRITICAL: Casbin enforce result for user {} (no roles, roles, create): {:?}",
        no_role_user_id, casbin_result_before
    );


    if casbin_result_before.is_ok() && casbin_result_before.as_ref().unwrap() == &true {
        eprintln!(
            "[SECURITY BUG DETECTED] User {} has no roles but Casbin returned true for (roles, create)!",
            no_role_user_id
        );
        eprintln!("This indicates a CRITICAL security vulnerability in the RBAC implementation.");
    }


    let no_role_response = app.clone().oneshot(no_role_req).await.unwrap();
    let status = no_role_response.status();


    let casbin_result_after = casbin_service
        .enforce(no_role_user_id, "roles", "create")
        .await;


    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "SECURITY VULNERABILITY: User without role was granted access (status 200) to POST /api/roles. \
        extract_permission returned: {:?}, \
        Casbin enforce (before): {:?}, \
        Casbin enforce (after): {:?}. \
        This indicates the permission middleware is not properly checking permissions.",
        perm,
        casbin_result_before,
        casbin_result_after
    );
}

#[tokio::test]
async fn test_permission_check_skipped_for_public_endpoint() {
    let pool = create_test_db().await;
    let casbin_service = create_test_casbin_service().await;
    let jwt_util = Arc::new(JwtUtil::new("test-secret-key-for-skip-test", "24h"));


    setup_test_data(&pool).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let user_id = create_test_user(&pool, "test_user").await;



    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let token = generate_token(&jwt_util, user_id, "test_user");


    let app = create_test_router(jwt_util.clone(), casbin_service.clone(), pool.clone());


    let req = Request::builder()
        .method("GET")
        .uri("/api/auth/permissions")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();


    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Permission check should be skipped for /api/auth/permissions"
    );
}

#[tokio::test]
async fn test_invalid_token_returns_unauthorized() {
    let pool = create_test_db().await;
    let casbin_service = create_test_casbin_service().await;
    let jwt_util = Arc::new(JwtUtil::new("test-secret-key-for-invalid-token-test", "24h"));


    setup_test_data(&pool).await;


    let app = create_test_router(jwt_util.clone(), casbin_service.clone(), pool.clone());


    let req = Request::builder()
        .method("GET")
        .uri("/api/roles")
        .header(header::AUTHORIZATION, "Bearer invalid_token")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Invalid token should return unauthorized"
    );
}

#[tokio::test]
async fn test_missing_token_returns_unauthorized() {
    let pool = create_test_db().await;
    let casbin_service = create_test_casbin_service().await;
    let jwt_util = Arc::new(JwtUtil::new("test-secret-key-for-missing-token-test", "24h"));


    setup_test_data(&pool).await;


    let app = create_test_router(jwt_util.clone(), casbin_service.clone(), pool.clone());


    let req = Request::builder()
        .method("GET")
        .uri("/api/roles")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Missing token should return unauthorized"
    );
}

#[tokio::test]
async fn test_user_permissions_updated_after_role_change() {
    let pool = create_test_db().await;
    let casbin_service = create_test_casbin_service().await;
    let jwt_util = Arc::new(JwtUtil::new("test-secret-key-for-update-test", "24h"));


    setup_test_data(&pool).await;
    let operator_role_id = create_menu_only_role(&pool).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let user_id = create_test_user(&pool, "dynamic_user").await;
    assign_role_to_user(&pool, user_id, operator_role_id).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let token = generate_token(&jwt_util, user_id, "dynamic_user");


    let app = create_test_router(jwt_util.clone(), casbin_service.clone(), pool.clone());


    let initial_req = Request::builder()
        .method("POST")
        .uri("/api/roles")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let initial_response = app.clone().oneshot(initial_req).await.unwrap();
    assert_eq!(
        initial_response.status(),
        StatusCode::UNAUTHORIZED,
        "User with operator role should not have access initially"
    );


    let admin_role_id = sqlx::query_as::<_, (i64,)>("SELECT id FROM roles WHERE code = 'admin'")
        .fetch_one(&pool)
        .await
        .expect("Failed to find admin role")
        .0;

    assign_role_to_user(&pool, user_id, admin_role_id).await;


    casbin_service.reload_policies_from_db(&pool).await.unwrap();


    let updated_req = Request::builder()
        .method("POST")
        .uri("/api/roles")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let updated_response = app.clone().oneshot(updated_req).await.unwrap();
    assert_eq!(
        updated_response.status(),
        StatusCode::OK,
        "User with admin role should have access after role update"
    );
}
