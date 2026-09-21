use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{AuthConfig, Config, RuntimeMode};

#[test]
fn runtime_mode_is_explicit_and_rejects_unknown_values() {
    assert_eq!(RuntimeMode::from_environment(None).unwrap(), RuntimeMode::Production);
    assert_eq!(
        RuntimeMode::from_environment(Some("development")).unwrap(),
        RuntimeMode::Development
    );
    assert_eq!(RuntimeMode::from_environment(Some("production")).unwrap(), RuntimeMode::Production);
    assert!(RuntimeMode::from_environment(Some("dev")).is_err());
}

#[test]
fn config_file_mode_rejects_an_empty_jwt_secret() {
    let config = Config {
        auth: AuthConfig { jwt_secret: String::new(), ..Default::default() },
        data_dir: None,
        ..Default::default()
    };

    let error = config
        .validate()
        .expect_err("config-file mode must require a JWT secret");
    assert!(error.to_string().contains("auth.jwt_secret is required"));
}

#[test]
fn zero_config_mode_accepts_the_bootstrap_jwt_placeholder() {
    let config = Config { data_dir: Some("data".into()), ..Default::default() };

    config
        .validate()
        .expect("zero-config mode generates and persists its JWT secret");
}

#[test]
fn config_file_reads_custom_audit_table() {
    let path = std::env::temp_dir().join(format!(
        "stellar-config-{}.toml",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));
    std::fs::write(
        &path,
        r#"
[auth]
jwt_secret = "test-secret"

[audit]
database = "custom_audit"
table = "query_events"
"#,
    )
    .expect("write test config");

    let config = Config::from_toml(path.to_str().expect("utf-8 path")).expect("read config");
    std::fs::remove_file(path).expect("remove test config");

    assert_eq!(config.audit.full_table_name(), "custom_audit.query_events");
}
