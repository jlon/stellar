use crate::models::{ClassifierRequest, CreateResourceGroupRequest, UpdateResourceGroupRequest};
use crate::services::ResourceGroupService;

fn create_request(
    mem_limit: Option<&str>,
    cpu_weight: Option<i32>,
    exclusive_cpu_cores: Option<i32>,
) -> CreateResourceGroupRequest {
    CreateResourceGroupRequest {
        name: "analytics".to_string(),
        cpu_weight,
        exclusive_cpu_cores,
        mem_limit: mem_limit.map(str::to_string),
        big_query_cpu_second_limit: None,
        big_query_scan_rows_limit: None,
        big_query_mem_limit: None,
        concurrency_limit: None,
        spill_mem_limit_threshold: None,
        classifiers: Vec::new(),
    }
}

#[test]
fn resource_group_creation_requires_mem_limit() {
    let request = create_request(None, Some(1), None);

    assert_eq!(
        ResourceGroupService::validate_create_request(&request),
        Err("mem_limit is required")
    );
}

#[test]
fn resource_group_creation_requires_one_cpu_mode() {
    let mut request = create_request(Some("50%"), None, None);
    request.classifiers.push(classifier_request());

    assert_eq!(
        ResourceGroupService::validate_create_request(&request),
        Err("exactly one CPU mode must be positive")
    );
}

#[test]
fn resource_group_creation_rejects_mixed_cpu_modes() {
    let mut request = create_request(Some("50%"), Some(1), Some(1));
    request.classifiers.push(classifier_request());

    assert_eq!(
        ResourceGroupService::validate_create_request(&request),
        Err("cpu_weight and exclusive_cpu_cores cannot both be positive")
    );
}

#[test]
fn resource_group_creation_requires_a_classifier() {
    let request = create_request(Some("50%"), Some(1), None);

    assert_eq!(
        ResourceGroupService::validate_create_request(&request),
        Err("at least one classifier is required")
    );
}

#[test]
fn resource_group_creation_rejects_an_empty_classifier() {
    let mut request = create_request(Some("50%"), Some(1), None);
    request.classifiers.push(ClassifierRequest {
        user: None,
        role: None,
        query_type: None,
        source_ip: None,
        db: None,
        weight: 1,
    });

    assert_eq!(
        ResourceGroupService::validate_create_request(&request),
        Err("each classifier must define at least one condition")
    );
}

#[test]
fn resource_group_creation_accepts_valid_configuration() {
    let mut request = create_request(Some("50%"), Some(1), None);
    request.classifiers.push(classifier_request());

    assert_eq!(ResourceGroupService::validate_create_request(&request), Ok(()));
}

#[test]
fn resource_group_creation_rejects_unsupported_query_types() {
    let mut request = create_request(Some("50%"), Some(1), None);
    request.classifiers.push(ClassifierRequest {
        query_type: Some(vec!["DELETE".to_string()]),
        ..classifier_request()
    });

    assert_eq!(
        ResourceGroupService::validate_create_request(&request),
        Err("query_type must contain only SELECT or INSERT")
    );
}

#[test]
fn resource_group_update_clears_inactive_cpu_mode_with_with_clause() {
    let request = UpdateResourceGroupRequest {
        cpu_weight: Some(0),
        exclusive_cpu_cores: Some(2),
        ..empty_update_request()
    };

    assert_eq!(
        ResourceGroupService::build_alter_sqls("analytics", &request).unwrap(),
        vec![
            "ALTER RESOURCE GROUP `analytics` WITH ('cpu_weight' = '0', 'exclusive_cpu_cores' = '2')"
        ]
    );
}

#[test]
fn resource_group_update_splits_classifier_changes_into_valid_statements() {
    let request = UpdateResourceGroupRequest {
        mem_limit: Some("50%".to_string()),
        add_classifiers: Some(vec![classifier_request()]),
        drop_classifier_ids: Some(vec![7]),
        ..empty_update_request()
    };

    assert_eq!(
        ResourceGroupService::build_alter_sqls("analytics", &request).unwrap(),
        vec![
            "ALTER RESOURCE GROUP `analytics` ADD (user='analytics')",
            "ALTER RESOURCE GROUP `analytics` WITH ('mem_limit' = '50%')",
            "ALTER RESOURCE GROUP `analytics` DROP (7)",
        ]
    );
}

#[test]
fn resource_group_update_rejects_empty_requests_and_invalid_cpu_modes() {
    assert_eq!(
        ResourceGroupService::validate_update_request(&empty_update_request()),
        Err("at least one resource group update is required")
    );

    let request = UpdateResourceGroupRequest {
        cpu_weight: Some(1),
        exclusive_cpu_cores: Some(1),
        ..empty_update_request()
    };
    assert_eq!(
        ResourceGroupService::validate_update_request(&request),
        Err("exactly one CPU mode must be positive when both modes are updated")
    );

    let request = UpdateResourceGroupRequest {
        add_classifiers: Some(vec![ClassifierRequest {
            user: None,
            role: None,
            query_type: None,
            source_ip: None,
            db: None,
            weight: 1,
        }]),
        ..empty_update_request()
    };
    assert_eq!(
        ResourceGroupService::validate_update_request(&request),
        Err("each classifier must define at least one condition")
    );

    let request =
        UpdateResourceGroupRequest { drop_classifier_ids: Some(vec![7]), ..empty_update_request() };
    assert_eq!(ResourceGroupService::validate_update_request(&request), Ok(()));
}

#[test]
fn resource_group_sql_escapes_backslashes_and_quotes() {
    let request = UpdateResourceGroupRequest {
        mem_limit: Some(r"50%\'".to_string()),
        add_classifiers: Some(vec![ClassifierRequest {
            user: Some(r"ops\'user".to_string()),
            ..classifier_request()
        }]),
        ..empty_update_request()
    };

    assert_eq!(
        ResourceGroupService::build_alter_sqls("analytics", &request).unwrap(),
        vec![
            r"ALTER RESOURCE GROUP `analytics` ADD (user='ops\\''user')",
            r"ALTER RESOURCE GROUP `analytics` WITH ('mem_limit' = '50%\\''')",
        ]
    );
}

fn classifier_request() -> ClassifierRequest {
    ClassifierRequest {
        user: Some("analytics".to_string()),
        role: None,
        query_type: None,
        source_ip: None,
        db: None,
        weight: 1,
    }
}

fn empty_update_request() -> UpdateResourceGroupRequest {
    UpdateResourceGroupRequest {
        cpu_weight: None,
        exclusive_cpu_cores: None,
        mem_limit: None,
        big_query_cpu_second_limit: None,
        big_query_scan_rows_limit: None,
        big_query_mem_limit: None,
        concurrency_limit: None,
        spill_mem_limit_threshold: None,
        add_classifiers: None,
        drop_classifier_ids: None,
    }
}
