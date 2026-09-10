use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use utoipa::ToSchema;

use crate::utils::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct PhysicalHost {
    pub id: i64,
    pub organization_id: i64,
    pub hostname: String,
    pub ssh_target: String,
    pub ssh_port: i64,
    pub host_key_fingerprint: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreatePhysicalHostRequest {
    pub organization_id: Option<i64>,
    pub hostname: String,
    pub ssh_target: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    /// OpenSSH host public key in "algorithm base64" format, without comments.
    pub host_key: String,
    /// OpenSSH SHA-256 fingerprint confirmed through a trusted out-of-band channel.
    pub host_key_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct SrPackage {
    pub id: i64,
    pub organization_id: i64,
    pub version: String,
    pub package_url: String,
    pub sha256: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSrPackageRequest {
    pub organization_id: Option<i64>,
    pub version: String,
    pub package_url: String,
    pub sha256: String,
}

impl CreateSrPackageRequest {
    pub fn normalize(mut self) -> ApiResult<Self> {
        self.version = self.version.trim().to_owned();
        self.package_url = self.package_url.trim().to_owned();
        self.sha256 = self.sha256.trim().to_ascii_lowercase();

        if self.version.is_empty()
            || self.version.len() > 64
            || self.version.chars().any(char::is_control)
        {
            return Err(ApiError::validation_error(
                "version must contain 1 to 64 printable characters",
            ));
        }
        if !is_valid_https_url(&self.package_url) {
            return Err(ApiError::validation_error("package_url must be a valid HTTPS URL"));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ApiError::validation_error(
                "sha256 must be a 64-character hexadecimal digest",
            ));
        }

        Ok(self)
    }
}

impl CreatePhysicalHostRequest {
    pub fn normalize(mut self) -> ApiResult<Self> {
        self.hostname = self.hostname.trim().to_owned();
        self.ssh_target = self.ssh_target.trim().to_owned();
        self.host_key = self
            .host_key
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        self.host_key_fingerprint = self.host_key_fingerprint.trim().to_owned();

        if self.hostname.is_empty() || self.hostname.len() > 255 {
            return Err(ApiError::validation_error("hostname must contain 1 to 255 characters"));
        }
        if self.hostname.chars().any(char::is_control) {
            return Err(ApiError::validation_error("hostname must not contain control characters"));
        }
        if !is_valid_ssh_target(&self.ssh_target) {
            return Err(ApiError::validation_error(
                "ssh_target must be an IP address or FQDN without spaces",
            ));
        }
        if self.ssh_port == 0 {
            return Err(ApiError::validation_error("ssh_port must be between 1 and 65535"));
        }
        if !is_valid_host_key(&self.host_key) {
            return Err(ApiError::validation_error(
                "host_key must contain a supported OpenSSH algorithm and Base64 public key",
            ));
        }
        if !is_valid_fingerprint(&self.host_key_fingerprint) {
            return Err(ApiError::validation_error(
                "host_key_fingerprint must use OpenSSH SHA256 format",
            ));
        }
        if host_key_fingerprint(&self.host_key).as_deref() != Some(&self.host_key_fingerprint) {
            return Err(ApiError::validation_error("host_key_fingerprint does not match host_key"));
        }

        Ok(self)
    }
}

fn default_ssh_port() -> u16 {
    22
}

fn is_valid_ssh_target(value: &str) -> bool {
    if value.is_empty() || value.len() > 255 {
        return false;
    }
    if value.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn is_valid_host_key(value: &str) -> bool {
    let mut parts = value.split(' ');
    let Some(algorithm) = parts.next() else {
        return false;
    };
    let Some(key) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }

    matches!(
        algorithm,
        "ssh-ed25519"
            | "ssh-rsa"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ecdsa-sha2-nistp521"
    ) && key.len() >= 20
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

fn is_valid_fingerprint(value: &str) -> bool {
    let Some(fingerprint) = value.strip_prefix("SHA256:") else {
        return false;
    };

    fingerprint.len() == 43
        && fingerprint
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
}

pub(crate) fn host_key_fingerprint(host_key: &str) -> Option<String> {
    let encoded_key = host_key.split_once(' ')?.1;
    let key = STANDARD.decode(encoded_key).ok()?;
    let digest = Sha256::digest(key);
    Some(format!("SHA256:{}", STANDARD.encode(digest).trim_end_matches('=')))
}

fn is_valid_https_url(value: &str) -> bool {
    let Some(host_and_path) = value.strip_prefix("https://") else {
        return false;
    };

    !host_and_path.is_empty()
        && host_and_path.len() <= 2_048
        && !host_and_path.starts_with('/')
        && host_and_path
            .chars()
            .all(|character| !character.is_control() && !character.is_whitespace())
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct SshCredential {
    pub id: i64,
    pub organization_id: i64,
    pub name: String,
    pub username: String,
    pub auth_type: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSshCredentialRequest {
    pub organization_id: Option<i64>,
    pub name: String,
    pub username: String,
    /// Write-only OpenSSH or PEM private key. It is encrypted before persistence.
    pub private_key: String,
}

impl CreateSshCredentialRequest {
    pub fn normalize(mut self) -> ApiResult<Self> {
        self.name = self.name.trim().to_owned();
        self.username = self.username.trim().to_owned();
        self.private_key = self.private_key.trim().to_owned();

        if !is_valid_name(&self.name, 100) {
            return Err(ApiError::validation_error(
                "credential name must contain 1 to 100 printable characters",
            ));
        }
        if !is_valid_identifier(&self.username, 64) {
            return Err(ApiError::validation_error("SSH username contains unsupported characters"));
        }
        if self.private_key.len() > 65_536
            || !self.private_key.starts_with("-----BEGIN ")
            || !self.private_key.contains("PRIVATE KEY-----")
            || !self.private_key.contains("-----END ")
            || self.private_key.contains("ENCRYPTED")
            || self
                .private_key
                .chars()
                .any(|character| character.is_control() && character != '\n' && character != '\r')
        {
            return Err(ApiError::validation_error(
                "private_key must be an unencrypted OpenSSH or PEM private key no larger than 64 KiB",
            ));
        }

        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct SrDatabaseCredential {
    pub id: i64,
    pub organization_id: i64,
    pub name: String,
    pub username: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSrDatabaseCredentialRequest {
    pub organization_id: Option<i64>,
    pub name: String,
    pub username: String,
    /// Write-only password. P0 restricts its character set before it can enter a fixed SQL template.
    pub password: String,
}

impl CreateSrDatabaseCredentialRequest {
    pub fn normalize(mut self) -> ApiResult<Self> {
        self.name = self.name.trim().to_owned();
        self.username = self.username.trim().to_owned();

        if !is_valid_name(&self.name, 100) {
            return Err(ApiError::validation_error(
                "credential name must contain 1 to 100 printable characters",
            ));
        }
        if !is_valid_identifier(&self.username, 64) {
            return Err(ApiError::validation_error(
                "database username contains unsupported characters",
            ));
        }
        if self.password.len() < 8
            || self.password.len() > 64
            || !self.password.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'!' | b'@'
                            | b'#'
                            | b'$'
                            | b'%'
                            | b'^'
                            | b'&'
                            | b'*'
                            | b'_'
                            | b'+'
                            | b'='
                            | b'-'
                            | b'.'
                    )
            })
        {
            return Err(ApiError::validation_error(
                "database password must contain 8 to 64 safe printable characters",
            ));
        }

        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct SrManagedCluster {
    pub id: i64,
    pub organization_id: i64,
    pub name: String,
    pub deployment_mode: String,
    pub sr_version: String,
    pub cluster_id: Option<i64>,
    pub install_dir: Option<String>,
    pub status: String,
    pub created_by: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct SrClusterNode {
    pub id: i64,
    pub managed_cluster_id: i64,
    pub host_id: i64,
    pub role: String,
    pub fe_role: Option<String>,
    pub advertise_host: String,
    pub service_port: i64,
    pub http_port: Option<i64>,
    pub query_port: Option<i64>,
    pub rpc_port: Option<i64>,
    pub brpc_port: Option<i64>,
    pub webserver_port: Option<i64>,
    pub meta_dir: Option<String>,
    pub storage_dir: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SrManagedClusterDetail {
    #[serde(flatten)]
    pub cluster: SrManagedCluster,
    pub nodes: Vec<SrClusterNode>,
    pub observed_nodes: Vec<SrObservedNode>,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct SrObservedNode {
    pub id: i64,
    pub managed_cluster_id: i64,
    pub node_type: String,
    pub address: String,
    pub raw_json: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct SrOperationTask {
    pub id: i64,
    pub organization_id: i64,
    pub managed_cluster_id: i64,
    pub task_type: String,
    pub status: String,
    pub current_step: Option<String>,
    pub error_message: Option<String>,
    pub result_json: Option<String>,
    pub created_by: i64,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct SrOperationEvent {
    pub id: i64,
    pub task_id: i64,
    pub step: String,
    pub status: String,
    pub node_id: Option<i64>,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SrOperationTaskDetail {
    #[serde(flatten)]
    pub task: SrOperationTask,
    pub events: Vec<SrOperationEvent>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct FrontendDeploymentNode {
    pub host_id: i64,
    pub advertise_host: String,
    #[serde(default = "default_edit_log_port")]
    pub edit_log_port: u16,
    #[serde(default = "default_fe_http_port")]
    pub http_port: u16,
    #[serde(default = "default_fe_query_port")]
    pub query_port: u16,
    #[serde(default = "default_fe_rpc_port")]
    pub rpc_port: u16,
    pub meta_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct BackendDeploymentNode {
    pub host_id: i64,
    pub advertise_host: String,
    #[serde(default = "default_be_heartbeat_port")]
    pub heartbeat_port: u16,
    #[serde(default = "default_be_port")]
    pub be_port: u16,
    #[serde(default = "default_be_webserver_port")]
    pub webserver_port: u16,
    #[serde(default = "default_be_brpc_port")]
    pub brpc_port: u16,
    pub storage_dir: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDeploymentRequest {
    pub organization_id: Option<i64>,
    pub name: String,
    pub package_id: i64,
    pub ssh_credential_id: i64,
    pub operator_credential_id: i64,
    pub install_dir: String,
    #[serde(default)]
    pub confirm_non_ha: bool,
    pub frontends: Vec<FrontendDeploymentNode>,
    pub backends: Vec<BackendDeploymentNode>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdoptClusterRequest {
    pub organization_id: Option<i64>,
    pub name: String,
    pub fe_host: String,
    #[serde(default = "default_fe_http_port")]
    pub fe_http_port: u16,
    #[serde(default = "default_fe_query_port")]
    pub fe_query_port: u16,
    pub operator_credential_id: i64,
}

impl AdoptClusterRequest {
    pub fn normalize(mut self) -> ApiResult<Self> {
        self.name = self.name.trim().to_owned();
        self.fe_host = self.fe_host.trim().to_owned();
        if !is_valid_identifier(&self.name, 100) {
            return Err(ApiError::validation_error("cluster name contains unsupported characters"));
        }
        if !is_valid_ssh_target(&self.fe_host) || self.fe_query_port == 0 || self.fe_http_port == 0
        {
            return Err(ApiError::validation_error("FE host and ports must be valid"));
        }
        if self.operator_credential_id <= 0 {
            return Err(ApiError::validation_error("operator credential ID must be positive"));
        }
        Ok(self)
    }
}

impl CreateDeploymentRequest {
    pub fn normalize(mut self) -> ApiResult<Self> {
        self.name = self.name.trim().to_owned();
        self.install_dir = normalize_absolute_path(&self.install_dir)?;

        if !is_valid_identifier(&self.name, 100) {
            return Err(ApiError::validation_error("cluster name contains unsupported characters"));
        }
        if self.package_id <= 0 || self.ssh_credential_id <= 0 || self.operator_credential_id <= 0 {
            return Err(ApiError::validation_error("package and credential IDs must be positive"));
        }
        if self.frontends.is_empty() || self.backends.is_empty() {
            return Err(ApiError::validation_error(
                "a deployment requires at least one FE and one BE",
            ));
        }
        if (self.frontends.len() < 3 || self.backends.len() < 3) && !self.confirm_non_ha {
            return Err(ApiError::validation_error("non-HA topology requires confirm_non_ha=true"));
        }

        let mut node_roles = std::collections::HashSet::new();
        for frontend in &mut self.frontends {
            validate_advertise_host(&frontend.advertise_host)?;
            if frontend.host_id <= 0
                || frontend.edit_log_port == 0
                || frontend.http_port == 0
                || frontend.query_port == 0
                || frontend.rpc_port == 0
            {
                return Err(ApiError::validation_error("FE node host and ports must be valid"));
            }
            frontend.meta_dir = Some(normalize_absolute_path(
                frontend
                    .meta_dir
                    .as_deref()
                    .unwrap_or(&format!("{}/fe/meta", self.install_dir)),
            )?);
            if !node_roles.insert((frontend.host_id, "fe")) {
                return Err(ApiError::validation_error("duplicate FE host in deployment topology"));
            }
        }
        for backend in &mut self.backends {
            validate_advertise_host(&backend.advertise_host)?;
            if backend.host_id <= 0
                || backend.heartbeat_port == 0
                || backend.be_port == 0
                || backend.webserver_port == 0
                || backend.brpc_port == 0
            {
                return Err(ApiError::validation_error("BE node host and ports must be valid"));
            }
            backend.storage_dir = Some(normalize_absolute_path(
                backend
                    .storage_dir
                    .as_deref()
                    .unwrap_or(&format!("{}/be/storage", self.install_dir)),
            )?);
            if !node_roles.insert((backend.host_id, "be")) {
                return Err(ApiError::validation_error("duplicate BE host in deployment topology"));
            }
        }

        let mut host_ports =
            std::collections::HashMap::<i64, std::collections::HashSet<u16>>::new();
        for (host_id, ports) in self
            .frontends
            .iter()
            .map(|node| {
                (node.host_id, [node.edit_log_port, node.http_port, node.query_port, node.rpc_port])
            })
            .chain(self.backends.iter().map(|node| {
                (
                    node.host_id,
                    [node.heartbeat_port, node.be_port, node.webserver_port, node.brpc_port],
                )
            }))
        {
            let allocated = host_ports.entry(host_id).or_default();
            if ports.into_iter().any(|port| !allocated.insert(port)) {
                return Err(ApiError::validation_error(
                    "deployment ports must be unique on each physical host",
                ));
            }
        }

        Ok(self)
    }
}

fn is_valid_name(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value.chars().all(|character| !character.is_control())
}

fn is_valid_identifier(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn normalize_absolute_path(value: &str) -> ApiResult<String> {
    let value = value.trim();
    if value == "/"
        || !value.starts_with('/')
        || value.len() > 1_024
        || value.split('/').any(|part| part == "." || part == "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
    {
        return Err(ApiError::validation_error(
            "paths must be absolute and contain only letters, digits, slash, dot, underscore, or hyphen",
        ));
    }
    Ok(value.to_owned())
}

fn validate_advertise_host(value: &str) -> ApiResult<()> {
    if value.parse::<std::net::Ipv4Addr>().is_err() {
        return Err(ApiError::validation_error("P0 requires advertise_host to be an IPv4 address"));
    }
    Ok(())
}

fn default_edit_log_port() -> u16 {
    9010
}
fn default_fe_http_port() -> u16 {
    8030
}
fn default_fe_query_port() -> u16 {
    9030
}
fn default_fe_rpc_port() -> u16 {
    9020
}
fn default_be_heartbeat_port() -> u16 {
    9050
}
fn default_be_port() -> u16 {
    9060
}
fn default_be_webserver_port() -> u16 {
    8040
}
fn default_be_brpc_port() -> u16 {
    8060
}

#[cfg(test)]
mod tests {
    use super::{
        BackendDeploymentNode, CreateDeploymentRequest, CreatePhysicalHostRequest,
        CreateSrPackageRequest, FrontendDeploymentNode, host_key_fingerprint,
    };

    fn valid_request() -> CreatePhysicalHostRequest {
        CreatePhysicalHostRequest {
            organization_id: Some(1),
            hostname: "be-01".to_string(),
            ssh_target: "10.10.0.11".to_string(),
            ssh_port: 22,
            host_key: format!("ssh-ed25519 {}", "A".repeat(40)),
            host_key_fingerprint: host_key_fingerprint(&format!("ssh-ed25519 {}", "A".repeat(40)))
                .unwrap(),
        }
    }

    #[test]
    fn normalizes_a_valid_host_request() {
        let mut request = valid_request();
        request.hostname = "  be-01  ".to_string();
        request.host_key = format!("  ssh-ed25519\n{}  ", "A".repeat(40));

        let request = request.normalize().expect("request should be valid");

        assert_eq!(request.hostname, "be-01");
        assert_eq!(request.host_key, format!("ssh-ed25519 {}", "A".repeat(40)));
    }

    #[test]
    fn rejects_an_unsafe_ssh_target() {
        let mut request = valid_request();
        request.ssh_target = "host; rm -rf /".to_string();

        assert!(request.normalize().is_err());
    }

    #[test]
    fn accepts_an_ipv6_ssh_target() {
        let mut request = valid_request();
        request.ssh_target = "2001:db8::11".to_string();

        assert!(request.normalize().is_ok());
    }

    #[test]
    fn rejects_root_as_an_installation_path() {
        let request = CreateDeploymentRequest {
            organization_id: Some(1),
            name: "sr".to_string(),
            package_id: 1,
            ssh_credential_id: 1,
            operator_credential_id: 1,
            install_dir: "/".to_string(),
            confirm_non_ha: true,
            frontends: vec![],
            backends: vec![],
        };

        assert!(request.normalize().is_err());
    }

    #[test]
    fn rejects_duplicate_ports_on_a_co_located_host() {
        let request = CreateDeploymentRequest {
            organization_id: Some(1),
            name: "sr".to_string(),
            package_id: 1,
            ssh_credential_id: 1,
            operator_credential_id: 1,
            install_dir: "/opt/starrocks".to_string(),
            confirm_non_ha: true,
            frontends: vec![FrontendDeploymentNode {
                host_id: 1,
                advertise_host: "10.0.0.1".to_string(),
                edit_log_port: 9010,
                http_port: 8030,
                query_port: 9030,
                rpc_port: 9020,
                meta_dir: None,
            }],
            backends: vec![BackendDeploymentNode {
                host_id: 1,
                advertise_host: "10.0.0.1".to_string(),
                heartbeat_port: 9050,
                be_port: 9060,
                webserver_port: 8030,
                brpc_port: 8060,
                storage_dir: None,
            }],
        };

        assert!(request.normalize().is_err());
    }

    #[test]
    fn rejects_a_host_key_with_unmatched_fingerprint() {
        let mut request = valid_request();
        request.host_key_fingerprint = format!("SHA256:{}", "B".repeat(43));

        assert!(request.normalize().is_err());
    }

    #[test]
    fn normalizes_a_package_digest() {
        let request = CreateSrPackageRequest {
            organization_id: Some(1),
            version: " 3.3.9 ".to_string(),
            package_url: " https://packages.example.com/starrocks-3.3.9.tar.gz ".to_string(),
            sha256: "A".repeat(64),
        }
        .normalize()
        .expect("package request should be valid");

        assert_eq!(request.version, "3.3.9");
        assert_eq!(request.sha256, "a".repeat(64));
    }

    #[test]
    fn rejects_non_https_package_urls() {
        let request = CreateSrPackageRequest {
            organization_id: Some(1),
            version: "3.3.9".to_string(),
            package_url: "http://packages.example.com/starrocks.tar.gz".to_string(),
            sha256: "a".repeat(64),
        };

        assert!(request.normalize().is_err());
    }
}
