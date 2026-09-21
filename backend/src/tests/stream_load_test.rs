use crate::{
    middleware::permission_extractor::extract_permission,
    services::stream_load::{self, StreamLoadFormat},
};

#[test]
fn stream_load_uses_existing_execute_permission() {
    assert_eq!(
        extract_permission("POST", "/api/clusters/queries/stream-load"),
        Some(("clusters".to_string(), "queries:execute".to_string()))
    );
}

#[test]
fn stream_load_accepts_only_supported_file_options() {
    let spec = stream_load::validate_stream_load_spec(
        "analytics",
        "events",
        "csv",
        Some("daily_events"),
        Some("\\t"),
    )
    .expect("valid stream load spec");

    assert_eq!(spec.format, StreamLoadFormat::Csv);
    assert_eq!(spec.column_separator.as_deref(), Some("\\t"));
    assert!(
        stream_load::validate_stream_load_spec("analytics", "events", "parquet", None, None)
            .is_err()
    );
    assert!(
        stream_load::validate_stream_load_spec("analytics/unsafe", "events", "csv", None, None)
            .is_err()
    );
    assert!(
        stream_load::validate_stream_load_spec("analytics", "events", "csv", None, Some(";"))
            .is_err()
    );
}

#[test]
fn stream_load_response_is_limited_to_known_engine_fields() {
    let response = stream_load::parse_stream_load_response(
        r#"{"Status":"Success","Label":"daily_events","NumberTotalRows":3,"NumberLoadedRows":2,"NumberFilteredRows":1,"LoadBytes":128,"Secret":"ignored"}"#,
    );

    assert!(response.success);
    assert_eq!(response.label.as_deref(), Some("daily_events"));
    assert_eq!(response.number_loaded_rows, Some(2));
    assert_eq!(response.load_bytes, Some(128));
}
