use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use tokio::{process::Command, time::timeout};

use crate::utils::{ApiError, ApiResult};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub struct SshTarget {
    pub id: i64,
    pub target: String,
    pub port: i64,
    pub host_key: String,
}

#[derive(Clone)]
pub struct SshContext {
    pub target: SshTarget,
    pub username: String,
    private_key_path: PathBuf,
    known_hosts_path: PathBuf,
}

pub struct SshOutput {
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait SshExecutor: Send + Sync {
    async fn run(&self, context: &SshContext, command: &str) -> ApiResult<SshOutput>;
    async fn copy(
        &self,
        context: &SshContext,
        local_path: &Path,
        remote_path: &str,
    ) -> ApiResult<()>;
}

#[derive(Default)]
pub struct OpenSshExecutor;

impl SshContext {
    pub async fn create(
        task_dir: &Path,
        target: SshTarget,
        username: String,
        private_key: &str,
    ) -> ApiResult<Self> {
        let directory = task_dir.join(format!("host-{}", target.id));
        let private_key_path = directory.join("id_key");
        let known_hosts_path = directory.join("known_hosts");

        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            std::fs::create_dir_all(&directory).map_err(|error| {
                ApiError::internal_error(format!("failed to create SSH work directory: {error}"))
            })?;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).map_err(
                |error| {
                    ApiError::internal_error(format!(
                        "failed to protect SSH work directory: {error}"
                    ))
                },
            )?;
            let mut key_file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&private_key_path)
                .map_err(|error| {
                    ApiError::internal_error(format!("failed to create temporary SSH key: {error}"))
                })?;
            key_file
                .write_all(private_key.as_bytes())
                .map_err(|error| {
                    ApiError::internal_error(format!("failed to write temporary SSH key: {error}"))
                })?;
            std::fs::write(&known_hosts_path, known_host_line(&target)).map_err(|error| {
                ApiError::internal_error(format!("failed to write known_hosts: {error}"))
            })?;
        }

        #[cfg(not(unix))]
        return Err(ApiError::internal_error(
            "physical deployment requires a Unix control-plane host",
        ));

        Ok(Self { target, username, private_key_path, known_hosts_path })
    }
}

#[async_trait]
impl SshExecutor for OpenSshExecutor {
    async fn run(&self, context: &SshContext, command: &str) -> ApiResult<SshOutput> {
        let mut process = Command::new("ssh");
        process.kill_on_drop(true);
        configure_ssh_command(&mut process, context, "-p");
        process.arg(format!("{}@{}", context.username, ssh_destination(&context.target.target)));
        process.arg(command);

        let output = timeout(COMMAND_TIMEOUT, process.output())
            .await
            .map_err(|_| ApiError::cluster_connection_failed("SSH command timed out"))?
            .map_err(|error| {
                ApiError::cluster_connection_failed(format!("failed to execute ssh: {error}"))
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(ApiError::cluster_connection_failed(format!(
                "SSH command failed on {}: {}",
                context.target.target,
                truncate(&stderr)
            )));
        }
        Ok(SshOutput { stdout, stderr })
    }

    async fn copy(
        &self,
        context: &SshContext,
        local_path: &Path,
        remote_path: &str,
    ) -> ApiResult<()> {
        let mut process = Command::new("scp");
        process.kill_on_drop(true);
        configure_ssh_command(&mut process, context, "-P");
        process.arg(local_path);
        process.arg(format!(
            "{}@{}:{}",
            context.username,
            ssh_destination(&context.target.target),
            remote_path
        ));

        let output = timeout(COMMAND_TIMEOUT, process.output())
            .await
            .map_err(|_| ApiError::cluster_connection_failed("SCP command timed out"))?
            .map_err(|error| {
                ApiError::cluster_connection_failed(format!("failed to execute scp: {error}"))
            })?;
        if !output.status.success() {
            return Err(ApiError::cluster_connection_failed(format!(
                "SCP failed for {}: {}",
                context.target.target,
                truncate(&String::from_utf8_lossy(&output.stderr))
            )));
        }
        Ok(())
    }
}

fn configure_ssh_command(command: &mut Command, context: &SshContext, port_flag: &str) {
    command
        .args(["-o", "BatchMode=yes"])
        .args(["-o", "StrictHostKeyChecking=yes"])
        .args(["-o", "GlobalKnownHostsFile=/dev/null"])
        .args(["-o", &format!("UserKnownHostsFile={}", context.known_hosts_path.display())])
        .args(["-o", "IdentitiesOnly=yes"])
        .args(["-o", "ConnectTimeout=10"])
        .args(["-o", "ServerAliveInterval=15"])
        .args(["-o", "ServerAliveCountMax=3"])
        .arg("-i")
        .arg(&context.private_key_path)
        .arg(port_flag)
        .arg(context.target.port.to_string());
}

fn known_host_line(target: &SshTarget) -> String {
    let address = if target.port == 22 {
        target.target.clone()
    } else {
        format!("[{}]:{}", target.target, target.port)
    };
    format!("{address} {}\n", target.host_key)
}

fn ssh_destination(target: &str) -> String {
    if target.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{target}]")
    } else {
        target.to_owned()
    }
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn truncate(value: &str) -> String {
    value.chars().take(2_048).collect()
}

#[cfg(test)]
mod tests {
    use super::{shell_quote, ssh_destination};

    #[test]
    fn shell_quote_preserves_single_quote_boundaries() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn brackets_ipv6_only_in_ssh_destinations() {
        assert_eq!(ssh_destination("2001:db8::1"), "[2001:db8::1]");
        assert_eq!(ssh_destination("fe-01.example.com"), "fe-01.example.com");
    }
}
