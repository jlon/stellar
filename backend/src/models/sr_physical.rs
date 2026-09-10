use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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

        if self.version.is_empty() || self.version.len() > 64 || self.version.chars().any(char::is_control) {
            return Err(ApiError::validation_error("version must contain 1 to 64 printable characters"));
        }
        if !is_valid_https_url(&self.package_url) {
            return Err(ApiError::validation_error("package_url must be a valid HTTPS URL"));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ApiError::validation_error("sha256 must be a 64-character hexadecimal digest"));
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

        Ok(self)
    }
}

fn default_ssh_port() -> u16 {
    22
}

fn is_valid_ssh_target(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
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

fn is_valid_https_url(value: &str) -> bool {
    let Some(host_and_path) = value.strip_prefix("https://") else {
        return false;
    };

    !host_and_path.is_empty()
        && host_and_path.len() <= 2_048
        && !host_and_path.starts_with('/')
        && host_and_path.chars().all(|character| !character.is_control() && !character.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::{CreatePhysicalHostRequest, CreateSrPackageRequest};

    fn valid_request() -> CreatePhysicalHostRequest {
        CreatePhysicalHostRequest {
            organization_id: Some(1),
            hostname: "be-01".to_string(),
            ssh_target: "10.10.0.11".to_string(),
            ssh_port: 22,
            host_key: format!("ssh-ed25519 {}", "A".repeat(40)),
            host_key_fingerprint: format!("SHA256:{}", "B".repeat(43)),
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
