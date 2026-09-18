use crate::config::{AuthConfig, Config};

#[test]
fn config_file_mode_rejects_an_empty_jwt_secret() {
    let config = Config {
        auth: AuthConfig { jwt_secret: String::new(), ..Default::default() },
        data_dir: None,
        ..Default::default()
    };

    let error = config.validate().expect_err("config-file mode must require a JWT secret");
    assert!(error.to_string().contains("auth.jwt_secret is required"));
}

#[test]
fn zero_config_mode_accepts_the_bootstrap_jwt_placeholder() {
    let config = Config { data_dir: Some("data".into()), ..Default::default() };

    config.validate().expect("zero-config mode generates and persists its JWT secret");
}
