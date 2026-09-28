use crate::{
    handlers::schema::{extract_ddl, parse_physical_properties, show_create_sql},
    middleware::{OrgContext, permission_extractor::extract_permission},
    models::{SchemaObjectIdentity, SchemaObjectKind},
    services::SchemaObjectReferenceStore,
};

fn object_identity() -> SchemaObjectIdentity {
    SchemaObjectIdentity {
        cluster_id: 7,
        catalog: "default_catalog".to_string(),
        database: "analytics".to_string(),
        name: "daily_orders".to_string(),
        object_kind: SchemaObjectKind::Table,
    }
}

fn org_context(user_id: i64, organization_id: Option<i64>) -> OrgContext {
    OrgContext { user_id, username: "operator".to_string(), organization_id, is_super_admin: false }
}

#[test]
fn schema_reference_is_opaque_scoped_and_valid_across_replicas() {
    let issuer = SchemaObjectReferenceStore::new("schema-reference-test-secret");
    let resolver = SchemaObjectReferenceStore::new("schema-reference-test-secret");
    let owner = org_context(1, Some(2));
    let identity = object_identity();

    let object_ref = issuer
        .issue(identity.clone(), &owner)
        .expect("issue reference");

    assert!(!object_ref.contains("daily_orders"));
    assert_eq!(resolver.resolve(&object_ref, 7, &owner).unwrap(), identity);
    assert!(resolver.resolve(&object_ref, 8, &owner).is_err());
    assert!(
        resolver
            .resolve(&object_ref, 7, &org_context(3, Some(2)))
            .is_err()
    );
    assert!(
        resolver
            .resolve(&object_ref, 7, &org_context(1, Some(4)))
            .is_err()
    );
    assert!(
        SchemaObjectReferenceStore::new("different-secret")
            .resolve(&object_ref, 7, &owner)
            .is_err()
    );
}

#[test]
fn schema_routes_map_to_dedicated_permissions() {
    assert_eq!(
        extract_permission("GET", "/api/clusters/7/schema/objects"),
        Some(("clusters".to_string(), "schema:list".to_string()))
    );
    assert_eq!(
        extract_permission("GET", "/api/clusters/7/schema/objects/reference"),
        Some(("clusters".to_string(), "schema:get".to_string()))
    );
    assert_eq!(
        extract_permission("POST", "/api/clusters/7/schema/objects/reference/refresh"),
        Some(("clusters".to_string(), "schema:refresh".to_string()))
    );
    assert_eq!(
        extract_permission("GET", "/api/clusters/7/schema/objects/reference/dependencies"),
        Some(("clusters".to_string(), "schema:dependencies".to_string()))
    );
}

#[test]
fn schema_sql_is_quoted_and_physical_properties_are_read_only() {
    let mut identity = object_identity();
    identity.database = "analytics`2026".to_string();
    assert_eq!(
        show_create_sql(&identity).unwrap(),
        "SHOW CREATE TABLE `analytics``2026`.`daily_orders`"
    );
    identity.name = "daily\norders".to_string();
    assert!(show_create_sql(&identity).is_err());
    assert!(extract_ddl(&[], &[]).is_err());

    let properties = parse_physical_properties(
        "CREATE TABLE `daily_orders` (\n`id` bigint\n)\nDUPLICATE KEY(`id`)\nPARTITION BY RANGE(`id`)\nDISTRIBUTED BY HASH(`id`) BUCKETS 8\nPROPERTIES (\n  \"replication_num\" = \"3\"\n)\nENGINE=OLAP",
    )
    .expect("physical properties");
    assert_eq!(properties.key_model.as_deref(), Some("DUPLICATE KEY(`id`)"));
    assert_eq!(properties.engine.as_deref(), Some("OLAP"));
    assert_eq!(properties.properties.get("replication_num"), Some(&"3".to_string()));
}
