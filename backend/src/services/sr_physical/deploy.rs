use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::read::GzDecoder;
use mysql_async::{OptsBuilder, Pool, prelude::Queryable};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tokio::{
    fs,
    io::AsyncWriteExt,
    time::{sleep, timeout},
};

use crate::{
    models::{
        AdoptClusterRequest, BackendDeploymentNode, ClusterType, CreateClusterRequest,
        CreateDeploymentRequest, DecommissionRequest, DeploymentMode, FrontendDeploymentNode,
        NodeCommandRequest, NodeConfigUpdateRequest, NodeScaleRequest, SrClusterNode,
        SrConfigDiffLine, SrConfigRevision, SrConfigRevisionSummary, SrManagedCluster,
        SrManagedClusterDetail, SrOperationTask, SrOperationTaskDetail,
    },
    services::{ClusterService, MySQLClient},
    utils::{ApiError, ApiResult},
};

use super::{
    BE_MANAGED_KEYS, CredentialService, FE_MANAGED_KEYS, FeConfig, OpenSshExecutor, SshContext,
    SshExecutor, SshTarget, be_config, extract_config_values, fe_config, shell_quote,
};

// A fresh FE may take several minutes to initialize BDB and elect itself leader;
// on hosts with sustained heavy I/O pressure it can take longer.
const READY_RETRIES: usize = 400;
const READY_INTERVAL: Duration = Duration::from_secs(3);
const MIN_FREE_KB: i64 = 20 * 1024 * 1024;
const SQL_TIMEOUT: Duration = Duration::from_secs(30);
const TASK_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);
const NODE_PORT_RETRIES: usize = 60;
const NODE_PORT_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct SrDeploymentService {
    pool: SqlitePool,
    cluster_service: Arc<ClusterService>,
    credential_service: Arc<CredentialService>,
    executor: Arc<dyn SshExecutor>,
    cache_dir: PathBuf,
    work_dir: PathBuf,
    package_allowed_hosts: HashSet<String>,
    supported_versions: HashSet<String>,
    cancel_flags: Arc<std::sync::Mutex<HashMap<i64, Arc<std::sync::atomic::AtomicBool>>>>,
}

impl SrDeploymentService {
    pub fn new(
        pool: SqlitePool,
        cluster_service: Arc<ClusterService>,
        credential_service: Arc<CredentialService>,
        cache_dir: PathBuf,
        package_allowed_hosts: Vec<String>,
        supported_versions: Vec<String>,
    ) -> Self {
        Self {
            pool,
            cluster_service,
            credential_service,
            executor: Arc::new(OpenSshExecutor),
            work_dir: cache_dir.join("tasks"),
            cache_dir,
            package_allowed_hosts: package_allowed_hosts
                .into_iter()
                .map(|host| host.trim().to_ascii_lowercase())
                .filter(|host| !host.is_empty())
                .collect(),
            supported_versions: supported_versions.into_iter().collect(),
            cancel_flags: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    fn cancel_flag(&self, task_id: i64) -> Arc<std::sync::atomic::AtomicBool> {
        let mut flags = self.cancel_flags.lock().expect("cancel flag mutex");
        flags
            .entry(task_id)
            .or_insert_with(|| Arc::new(std::sync::atomic::AtomicBool::new(false)))
            .clone()
    }

    /// Coarse cancellation check, called between runner steps.
    fn ensure_not_cancelled(&self, task_id: i64) -> ApiResult<()> {
        let cancelled = self
            .cancel_flags
            .lock()
            .expect("cancel flag mutex")
            .get(&task_id)
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed));
        if cancelled {
            return Err(ApiError::validation_error("task cancelled"));
        }
        Ok(())
    }

    pub async fn submit(
        self: &Arc<Self>,
        request: CreateDeploymentRequest,
        organization_id: i64,
        user_id: i64,
    ) -> ApiResult<SrOperationTask> {
        let request = request.normalize()?;
        let package: (String, String, Option<String>, String) = sqlx::query_as(
            "SELECT version, package_url, local_path, sha256 FROM sr_packages WHERE id = ? AND organization_id = ?",
        )
        .bind(request.package_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::validation_error("package does not belong to the organization"))?;
        if package.2.is_none() {
            self.ensure_package_source(&package.1)?;
        }
        if self.supported_versions.is_empty() || !self.supported_versions.contains(&package.0) {
            return Err(ApiError::validation_error(
                "package version is not in APP_SR_PHYSICAL_SUPPORTED_VERSIONS",
            ));
        }

        self.ensure_credential_ownership(
            request.ssh_credential_id,
            request.operator_credential_id,
            organization_id,
        )
        .await?;
        self.ensure_hosts_ownership(&request, organization_id)
            .await?;

        // Two shared-nothing clusters may share a physical host only when every
        // install path is isolated.
        let host_ids: HashSet<i64> = request
            .frontends
            .iter()
            .map(|node| node.host_id)
            .chain(request.backends.iter().map(|node| node.host_id))
            .collect();
        let install_dir_conflict: Option<i64> = sqlx::query_scalar(
            "SELECT m.id FROM sr_managed_clusters m JOIN sr_cluster_nodes n ON n.managed_cluster_id = m.id WHERE m.install_dir = ? AND n.host_id IN (SELECT value FROM json_each(?)) LIMIT 1",
        )
        .bind(&request.install_dir)
        .bind(serde_json::to_string(&host_ids)?)
        .fetch_optional(&self.pool)
        .await?;
        if install_dir_conflict.is_some() {
            return Err(ApiError::validation_error(format!(
                "install_dir {} is already used by a managed cluster on these hosts",
                request.install_dir
            )));
        }

        let existing_cluster: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM clusters WHERE name = ? UNION SELECT id FROM sr_managed_clusters WHERE name = ? LIMIT 1",
        )
        .bind(&request.name)
        .bind(&request.name)
        .fetch_optional(&self.pool)
        .await?;
        if existing_cluster.is_some() {
            return Err(ApiError::validation_error("cluster name already exists"));
        }

        let payload = DeploymentPayload::from_request(
            &request,
            package.0.clone(),
            package.1,
            package.2,
            package.3,
        );
        if let Some(bootstrap_credential_id) = request.bootstrap_credential_id {
            let owned: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM sr_database_credentials WHERE id = ? AND organization_id = ?",
            )
            .bind(bootstrap_credential_id)
            .bind(organization_id)
            .fetch_optional(&self.pool)
            .await?;
            if owned.is_none() {
                return Err(ApiError::validation_error(
                    "bootstrap credential must belong to the deployment organization",
                ));
            }
        }
        let payload_json = serde_json::to_string(&payload)?;
        let mut tx = self.pool.begin().await?;
        let cluster_id = sqlx::query(
            "INSERT INTO sr_managed_clusters (organization_id, name, sr_version, install_dir, ssh_credential_id, package_id, operator_credential_id, bootstrap_credential_id, created_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(organization_id)
        .bind(&request.name)
        .bind(&payload.sr_version)
        .bind(&request.install_dir)
        .bind(request.ssh_credential_id)
        .bind(request.package_id)
        .bind(request.operator_credential_id)
        .bind(request.bootstrap_credential_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();

        for (index, frontend) in request.frontends.iter().enumerate() {
            sqlx::query(
                "INSERT INTO sr_cluster_nodes (managed_cluster_id, host_id, role, fe_role, advertise_host, service_port, http_port, query_port, rpc_port, meta_dir) VALUES (?, ?, 'fe', ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(cluster_id)
            .bind(frontend.host_id)
            .bind(if index == 0 { "leader" } else { "follower" })
            .bind(&frontend.advertise_host)
            .bind(i64::from(frontend.edit_log_port))
            .bind(i64::from(frontend.http_port))
            .bind(i64::from(frontend.query_port))
            .bind(i64::from(frontend.rpc_port))
            .bind(frontend.meta_dir.as_deref())
            .execute(&mut *tx)
            .await?;
        }
        for backend in &request.backends {
            sqlx::query(
                "INSERT INTO sr_cluster_nodes (managed_cluster_id, host_id, role, advertise_host, service_port, http_port, brpc_port, webserver_port, starlet_port, storage_dir) VALUES (?, ?, 'be', ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(cluster_id)
            .bind(backend.host_id)
            .bind(&backend.advertise_host)
            .bind(i64::from(backend.heartbeat_port))
            .bind(i64::from(backend.be_port))
            .bind(i64::from(backend.brpc_port))
            .bind(i64::from(backend.webserver_port))
            .bind(i64::from(backend.starlet_port))
            .bind(backend.storage_dir.as_deref())
            .execute(&mut *tx)
            .await?;
        }

        let task_id = sqlx::query(
            "INSERT INTO sr_operation_tasks (organization_id, managed_cluster_id, task_type, payload_json, created_by) VALUES (?, ?, 'deploy', ?, ?)",
        )
        .bind(organization_id)
        .bind(cluster_id)
        .bind(payload_json)
        .bind(user_id)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        tx.commit().await?;

        let task = self
            .get_task_for_org(task_id, Some(organization_id))
            .await?
            .task;
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = service.run_task(task_id).await {
                tracing::error!(task_id, error = %error, "Physical deployment task failed");
            }
        });
        Ok(task)
    }

    pub async fn submit_adoption(
        self: &Arc<Self>,
        request: AdoptClusterRequest,
        organization_id: i64,
        user_id: i64,
    ) -> ApiResult<SrOperationTask> {
        let request = request.normalize()?;
        let credential: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM sr_database_credentials WHERE id = ? AND organization_id = ?",
        )
        .bind(request.operator_credential_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?;
        if credential.is_none() {
            return Err(ApiError::validation_error(
                "operator credential must belong to the adoption organization",
            ));
        }
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM clusters WHERE name = ? UNION SELECT id FROM sr_managed_clusters WHERE name = ? LIMIT 1",
        )
        .bind(&request.name)
        .bind(&request.name)
        .fetch_optional(&self.pool)
        .await?;
        if existing.is_some() {
            return Err(ApiError::validation_error("cluster name already exists"));
        }

        let payload = AdoptionPayload::from_request(&request);
        let payload_json = serde_json::to_string(&payload)?;
        let mut transaction = self.pool.begin().await?;
        let managed_cluster_id = sqlx::query(
            "INSERT INTO sr_managed_clusters (organization_id, name, sr_version, operator_credential_id, status, created_by) VALUES (?, ?, 'unknown', ?, 'planning', ?)",
        )
        .bind(organization_id)
        .bind(&request.name)
        .bind(request.operator_credential_id)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?
        .last_insert_rowid();
        let task_id = sqlx::query(
            "INSERT INTO sr_operation_tasks (organization_id, managed_cluster_id, task_type, payload_json, created_by) VALUES (?, ?, 'adopt_read_only', ?, ?)",
        )
        .bind(organization_id)
        .bind(managed_cluster_id)
        .bind(payload_json)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?
        .last_insert_rowid();
        transaction.commit().await?;

        let task = self
            .get_task_for_org(task_id, Some(organization_id))
            .await?
            .task;
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = service.run_task(task_id).await {
                tracing::error!(task_id, error = %error, "Read-only adoption task failed");
            }
        });
        Ok(task)
    }

    pub async fn submit_node_command(
        self: &Arc<Self>,
        managed_cluster_id: i64,
        node_id: i64,
        request: NodeCommandRequest,
        organization_id: i64,
        user_id: i64,
    ) -> ApiResult<SrOperationTask> {
        let request = request.normalize()?;
        let managed: Option<(String, Option<i64>)> = sqlx::query_as(
            "SELECT status, ssh_credential_id FROM sr_managed_clusters WHERE id = ? AND organization_id = ?",
        )
        .bind(managed_cluster_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((status, ssh_credential_id)) = managed else {
            return Err(ApiError::not_found("managed cluster not found"));
        };
        if ssh_credential_id.is_none() {
            return Err(ApiError::validation_error("node commands require an SSH-managed cluster"));
        }
        if status != "running" {
            return Err(ApiError::validation_error(
                "node commands require a running managed cluster",
            ));
        }
        let node: Option<(i64, String)> = sqlx::query_as(
            "SELECT id, status FROM sr_cluster_nodes WHERE id = ? AND managed_cluster_id = ?",
        )
        .bind(node_id)
        .bind(managed_cluster_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((_, node_status)) = node else {
            return Err(ApiError::validation_error("node does not belong to this managed cluster"));
        };
        match request.action.as_str() {
            "start" if node_status == "running" => {
                return Err(ApiError::validation_error("node is already running"));
            },
            // restart works from either state: it always stops first, which
            // also recovers nodes left inconsistent by an interrupted task.
            "stop" | "restart" if !matches!(node_status.as_str(), "running" | "stopped") => {
                return Err(ApiError::validation_error(
                    "only a running or stopped node can be stopped or restarted",
                ));
            },
            _ => {},
        }
        self.ensure_no_running_task(managed_cluster_id).await?;

        let payload = NodeCommandPayload { node_id, action: request.action };
        let payload_json = serde_json::to_string(&payload)?;
        let task_id = sqlx::query(
            "INSERT INTO sr_operation_tasks (organization_id, managed_cluster_id, task_type, payload_json, created_by) VALUES (?, ?, 'node_command', ?, ?)",
        )
        .bind(organization_id)
        .bind(managed_cluster_id)
        .bind(payload_json)
        .bind(user_id)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        let task = self
            .get_task_for_org(task_id, Some(organization_id))
            .await?
            .task;
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = service.run_task(task_id).await {
                tracing::error!(task_id, error = %error, "Node command task failed");
            }
        });
        Ok(task)
    }

    pub async fn submit_import(
        self: &Arc<Self>,
        managed_cluster_id: i64,
        organization_id: i64,
        user_id: i64,
    ) -> ApiResult<SrOperationTask> {
        let managed: Option<(String, String, Option<i64>)> = sqlx::query_as(
            "SELECT status, name, cluster_id FROM sr_managed_clusters WHERE id = ? AND organization_id = ?",
        )
        .bind(managed_cluster_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((status, name, cluster_id)) = managed else {
            return Err(ApiError::not_found("managed cluster not found"));
        };
        if cluster_id.is_some() {
            return Err(ApiError::validation_error("managed cluster is already imported"));
        }
        if status != "running" {
            return Err(ApiError::validation_error(
                "only a successfully deployed managed cluster can be imported",
            ));
        }
        let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM clusters WHERE name = ?")
            .bind(&name)
            .fetch_optional(&self.pool)
            .await?;
        if existing.is_some() {
            return Err(ApiError::validation_error("cluster name already exists"));
        }
        self.ensure_no_running_task(managed_cluster_id).await?;

        let task_id = sqlx::query(
            "INSERT INTO sr_operation_tasks (organization_id, managed_cluster_id, task_type, payload_json, created_by) VALUES (?, ?, 'import_cluster', '{}', ?)",
        )
        .bind(organization_id)
        .bind(managed_cluster_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        let task = self
            .get_task_for_org(task_id, Some(organization_id))
            .await?
            .task;
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = service.run_task(task_id).await {
                tracing::error!(task_id, error = %error, "Cluster import task failed");
            }
        });
        Ok(task)
    }

    pub async fn list_tasks(
        &self,
        organization_id: Option<i64>,
        limit: i64,
        offset: i64,
    ) -> ApiResult<Vec<SrOperationTask>> {
        let limit = limit.clamp(1, 200);
        let offset = offset.max(0);
        match organization_id {
            Some(organization_id) => sqlx::query_as(
                "SELECT id, organization_id, managed_cluster_id, task_type, status, current_step, error_message, result_json, created_by, created_at, started_at, finished_at FROM sr_operation_tasks WHERE organization_id = ? ORDER BY id DESC LIMIT ? OFFSET ?",
            )
            .bind(organization_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into),
            None => sqlx::query_as(
                "SELECT id, organization_id, managed_cluster_id, task_type, status, current_step, error_message, result_json, created_by, created_at, started_at, finished_at FROM sr_operation_tasks ORDER BY id DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into),
        }
    }

    pub async fn get_task_for_org(
        &self,
        id: i64,
        organization_id: Option<i64>,
    ) -> ApiResult<SrOperationTaskDetail> {
        let task: Option<SrOperationTask> = match organization_id {
            Some(organization_id) => sqlx::query_as(
                "SELECT id, organization_id, managed_cluster_id, task_type, status, current_step, error_message, result_json, created_by, created_at, started_at, finished_at FROM sr_operation_tasks WHERE id = ? AND organization_id = ?",
            )
            .bind(id)
            .bind(organization_id)
            .fetch_optional(&self.pool)
            .await?,
            None => sqlx::query_as(
                "SELECT id, organization_id, managed_cluster_id, task_type, status, current_step, error_message, result_json, created_by, created_at, started_at, finished_at FROM sr_operation_tasks WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await?,
        };
        let task = task.ok_or_else(|| ApiError::not_found("deployment task not found"))?;
        let events = sqlx::query_as(
            "SELECT id, task_id, step, status, node_id, message, created_at FROM sr_operation_events WHERE task_id = ? ORDER BY id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(SrOperationTaskDetail { task, events })
    }

    pub async fn cancel(&self, id: i64, organization_id: Option<i64>) -> ApiResult<()> {
        let mut query = if organization_id.is_some() {
            sqlx::query("UPDATE sr_operation_tasks SET status = 'cancelled', finished_at = CURRENT_TIMESTAMP WHERE id = ? AND organization_id = ? AND status = 'pending'").bind(id)
        } else {
            sqlx::query("UPDATE sr_operation_tasks SET status = 'cancelled', finished_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'pending'").bind(id)
        };
        if let Some(organization_id) = organization_id {
            query = query.bind(organization_id);
        }
        if query.execute(&self.pool).await?.rows_affected() > 0 {
            return Ok(());
        }
        // A running task cannot have its SSH child killed safely from here, but
        // the runner checks the flag between steps and stops at the next one.
        let mut query = if organization_id.is_some() {
            sqlx::query("UPDATE sr_operation_tasks SET status = 'cancelled', finished_at = CURRENT_TIMESTAMP WHERE id = ? AND organization_id = ? AND status = 'running'").bind(id)
        } else {
            sqlx::query("UPDATE sr_operation_tasks SET status = 'cancelled', finished_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'running'").bind(id)
        };
        if let Some(organization_id) = organization_id {
            query = query.bind(organization_id);
        }
        if query.execute(&self.pool).await?.rows_affected() == 0 {
            return Err(ApiError::validation_error(
                "only a pending or running task in the current organization can be cancelled",
            ));
        }
        self.cancel_flag(id)
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // A cancelled deploy would otherwise leave the cluster in 'deploying'.
        sqlx::query(
            "UPDATE sr_managed_clusters SET status = 'failed', updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT managed_cluster_id FROM sr_operation_tasks WHERE id = ?) AND status = 'deploying'",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Which of the given clusters are self-managed by the physical deployment
    /// module. Batched single query; used to annotate cluster list responses.
    /// Self-managed = deployed with SSH/lifecycle control (planning/deploying/
    /// running); adopted-read-only and removed clusters are NOT self-managed.
    pub async fn self_managed_cluster_ids(
        &self,
        cluster_ids: &[i64],
    ) -> ApiResult<std::collections::HashSet<i64>> {
        let mut ids = std::collections::HashSet::new();
        for chunk in cluster_ids.chunks(400) {
            if chunk.is_empty() {
                continue;
            }
            let placeholders =
                std::iter::repeat("?").take(chunk.len()).collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT cluster_id FROM sr_managed_clusters \
                 WHERE cluster_id IN ({placeholders}) \
                 AND status IN ('planning', 'deploying', 'running')"
            );
            let mut query = sqlx::query_as::<sqlx::Sqlite, (Option<i64>,)>(&sql);
            for id in chunk {
                query = query.bind(id);
            }
            let rows = query.fetch_all(&self.pool).await?;
            for (cluster_id,) in rows {
                if let Some(cluster_id) = cluster_id {
                    ids.insert(cluster_id);
                }
            }
        }
        Ok(ids)
    }

    /// Deployment-module ownership of a cluster: `None` = external import only,
    /// `Some(status)` = linked to a managed cluster record (self-managed or
    /// adopted read-only). Authoritative source for delete/edit guards.
    pub async fn managed_cluster_status(&self, cluster_id: i64) -> ApiResult<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT status FROM sr_managed_clusters WHERE cluster_id = ? LIMIT 1",
        )
        .bind(cluster_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn list_clusters(
        &self,
        organization_id: Option<i64>,
    ) -> ApiResult<Vec<SrManagedCluster>> {
        match organization_id {
            Some(organization_id) => sqlx::query_as("SELECT id, organization_id, name, deployment_mode, sr_version, cluster_id, install_dir, status, created_by, created_at, updated_at FROM sr_managed_clusters WHERE organization_id = ? ORDER BY id DESC")
                .bind(organization_id).fetch_all(&self.pool).await.map_err(Into::into),
            None => sqlx::query_as("SELECT id, organization_id, name, deployment_mode, sr_version, cluster_id, install_dir, status, created_by, created_at, updated_at FROM sr_managed_clusters ORDER BY id DESC")
                .fetch_all(&self.pool).await.map_err(Into::into),
        }
    }

    pub async fn get_cluster_for_org(
        &self,
        id: i64,
        organization_id: Option<i64>,
    ) -> ApiResult<SrManagedClusterDetail> {
        let cluster = match organization_id {
            Some(organization_id) => sqlx::query_as("SELECT id, organization_id, name, deployment_mode, sr_version, cluster_id, install_dir, status, created_by, created_at, updated_at FROM sr_managed_clusters WHERE id = ? AND organization_id = ?")
                .bind(id).bind(organization_id).fetch_optional(&self.pool).await?,
            None => sqlx::query_as("SELECT id, organization_id, name, deployment_mode, sr_version, cluster_id, install_dir, status, created_by, created_at, updated_at FROM sr_managed_clusters WHERE id = ?")
                .bind(id).fetch_optional(&self.pool).await?,
        }.ok_or_else(|| ApiError::not_found("managed cluster not found"))?;
        let nodes = sqlx::query_as("SELECT id, managed_cluster_id, host_id, role, fe_role, advertise_host, service_port, http_port, query_port, rpc_port, brpc_port, webserver_port, starlet_port, meta_dir, storage_dir, status, created_at, updated_at FROM sr_cluster_nodes WHERE managed_cluster_id = ? ORDER BY id")
            .bind(id).fetch_all(&self.pool).await?;
        let observed_nodes = sqlx::query_as("SELECT id, managed_cluster_id, node_type, address, raw_json, created_at FROM sr_observed_nodes WHERE managed_cluster_id = ? ORDER BY id")
            .bind(id).fetch_all(&self.pool).await?;
        Ok(SrManagedClusterDetail { cluster, nodes, observed_nodes })
    }

    pub async fn recover_interrupted_tasks(&self) -> ApiResult<()> {
        let result = sqlx::query("UPDATE sr_operation_tasks SET status = 'interrupted', finished_at = CURRENT_TIMESTAMP, error_message = 'Task interrupted by Stellar restart' WHERE status = 'running'")
            .execute(&self.pool).await?;
        if result.rows_affected() > 0 {
            tracing::warn!(
                count = result.rows_affected(),
                "Marked stale physical deployment tasks interrupted"
            );
        }
        fs::remove_dir_all(&self.work_dir).await.ok();
        Ok(())
    }

    async fn run_deploy(&self, task_id: i64) -> ApiResult<()> {
        let task: (i64, i64, String, i64) = sqlx::query_as("SELECT organization_id, managed_cluster_id, payload_json, created_by FROM sr_operation_tasks WHERE id = ?")
            .bind(task_id).fetch_one(&self.pool).await?;
        let payload: DeploymentPayload = serde_json::from_str(&task.2)?;
        let runtime = self.load_runtime(task.1, Some(task.0)).await?;
        let (ssh_credential, private_key) = self
            .credential_service
            .ssh_secret(
                runtime.ssh_credential_id.ok_or_else(|| {
                    ApiError::internal_error("deployed cluster is missing its SSH credential")
                })?,
                task.0,
            )
            .await?;
        let (operator_credential, operator_password) = self
            .credential_service
            .database_secret(
                runtime.operator_credential_id.ok_or_else(|| {
                    ApiError::internal_error("deployed cluster is missing its operator credential")
                })?,
                task.0,
            )
            .await?;
        let task_dir = self.work_dir.join(task_id.to_string());
        fs::create_dir_all(&task_dir).await.map_err(|error| {
            ApiError::internal_error(format!("failed to create task work directory: {error}"))
        })?;

        let execution = async {
            self.record(task_id, "precheck", "running", "Checking SSH, disk, Java, and ports", None).await?;
            self.ensure_not_cancelled(task_id)?;
            let contexts = self.precheck(task_id, &runtime, &ssh_credential.username, &private_key, &task_dir).await?;
            self.ensure_not_cancelled(task_id)?;
            self.record(task_id, "precheck", "succeeded", "Host precheck passed", None).await?;
            self.ensure_not_cancelled(task_id)?;

            self.reserve_ports(task.1, &runtime).await?;
            self.record(task_id, "reserve", "succeeded", "Physical host service ports reserved for this managed cluster", None).await?;
            self.ensure_not_cancelled(task_id)?;

            let archive = self.cache_package(task_id, &payload).await?;
            let archive_root = validate_archive(&archive)?;
            self.record(task_id, "cache_package", "succeeded", "Installation package downloaded and SHA-256 verified", None).await?;
            self.ensure_not_cancelled(task_id)?;

            self.distribute_and_install(task_id, &runtime, &contexts, &archive, &archive_root, &HashMap::new()).await?;
            self.record(task_id, "install", "succeeded", "Package installed on all planned hosts", None).await?;
            self.ensure_not_cancelled(task_id)?;

            self.write_configs(task_id, task.1, task.3, &runtime, &contexts).await?;
            self.record(task_id, "render_config", "succeeded", "Managed FE and BE configuration written atomically", None).await?;
            self.ensure_not_cancelled(task_id)?;

            let leader = runtime.frontends.first().ok_or_else(|| ApiError::internal_error("deployment has no leader FE"))?;
            let leader_context = contexts.get(&leader.host_id).ok_or_else(|| ApiError::internal_error("leader SSH context missing"))?;
            let leader_query_port = required_port(leader.query_port, "leader query")?;
            self.executor.run(leader_context, &format!("sh {}/current/fe/bin/start_fe.sh --daemon", shell_quote(&runtime.install_dir))).await?;
            wait_for_sql(&leader.advertise_host, leader_query_port, "SELECT 1", &root_auth()).await?;
            self.record(task_id, "start_leader_fe", "succeeded", "Leader FE accepts MySQL connections", Some(leader.id)).await?;
            self.ensure_not_cancelled(task_id)?;

            for follower in runtime.frontends.iter().skip(1) {
                if !node_exists(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/frontends'",
                    &follower.advertise_host,
                    follower.service_port,
                    &root_auth(),
                )
                .await?
                {
                    execute_sql(&leader.advertise_host, leader_query_port, &[format!("ALTER SYSTEM ADD FOLLOWER \"{}:{}\"", follower.advertise_host, follower.service_port)], &root_auth()).await?;
                }
                let context = contexts.get(&follower.host_id).ok_or_else(|| ApiError::internal_error("follower SSH context missing"))?;
                self.executor.run(context, &format!("sh {}/current/fe/bin/start_fe.sh --helper {}:{} --daemon", shell_quote(&runtime.install_dir), follower.advertise_host, leader.service_port)).await?;
                wait_for_node(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/frontends'",
                    &follower.advertise_host,
                    follower.service_port,
                    &root_auth(),
                )
                .await?;
            }
            self.record(task_id, "add_followers", "succeeded", "Follower FEs joined the cluster", None).await?;
            self.ensure_not_cancelled(task_id)?;

            for backend in &runtime.backends {
                let context = contexts.get(&backend.host_id).ok_or_else(|| ApiError::internal_error("BE SSH context missing"))?;
                self.executor.run(context, &format!("sh {}/current/be/bin/start_be.sh --daemon", shell_quote(&runtime.install_dir))).await?;
                if !node_exists(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/backends'",
                    &backend.advertise_host,
                    backend.service_port,
                    &root_auth(),
                )
                .await?
                {
                    execute_sql(&leader.advertise_host, leader_query_port, &[format!("ALTER SYSTEM ADD BACKEND \"{}:{}\"", backend.advertise_host, backend.service_port)], &root_auth()).await?;
                }
                wait_for_node(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/backends'",
                    &backend.advertise_host,
                    backend.service_port,
                    &root_auth(),
                )
                .await?;
            }
            self.record(task_id, "start_and_add_be", "succeeded", "Backend nodes joined the cluster", None).await?;
            self.ensure_not_cancelled(task_id)?;

            let operator = sql_identifier(&operator_credential.username)?;
            execute_sql(&leader.advertise_host, leader_query_port, &[format!(
                "CREATE USER '{}'@'%' IDENTIFIED BY {}",
                operator,
                sql_string_literal(&operator_password)
            )], &root_auth()).await?;
            execute_sql(&leader.advertise_host, leader_query_port, &[format!(
                "GRANT OPERATE ON SYSTEM TO USER '{}'@'%'",
                operator
            )], &root_auth()).await?;
            // ALTER SYSTEM ADD/DROP requires the NODE privilege, which 4.1
            // cannot grant directly to a user; use the built-in role instead.
            execute_sql(&leader.advertise_host, leader_query_port, &[format!(
                "GRANT 'cluster_admin' TO USER '{}'@'%'",
                operator
            )], &root_auth()).await?;
            execute_sql(&leader.advertise_host, leader_query_port, &[format!(
                "GRANT SELECT ON ALL TABLES IN DATABASE information_schema TO USER '{}'@'%'",
                operator
            )], &root_auth()).await?;
            self.record(task_id, "create_operator", "succeeded", "Operator account created", None).await?;
            self.ensure_not_cancelled(task_id)?;

            // Freshly initialized clusters expose an empty-password root account;
            // close that window before the task completes.
            if let Some(bootstrap_credential_id) = payload.bootstrap_credential_id {
                let (_, bootstrap_password) = self
                    .credential_service
                    .database_secret(bootstrap_credential_id, task.0)
                    .await?;
                execute_sql(
                    &leader.advertise_host,
                    leader_query_port,
                    &[format!(
                        "ALTER USER root IDENTIFIED BY {}",
                        sql_string_literal(&bootstrap_password)
                    )],
                    &root_auth(),
                )
                .await?;
                self.record(task_id, "secure_root", "succeeded", "root account secured with the bootstrap credential", None).await?;
            self.ensure_not_cancelled(task_id)?;
            }

            sqlx::query("UPDATE sr_cluster_nodes SET status = 'running', updated_at = CURRENT_TIMESTAMP WHERE managed_cluster_id = ?")
                .bind(task.1).execute(&self.pool).await?;
            // Importing into the existing clusters registry is an explicit, audited
            // follow-up operation: POST /api/sr-ops/clusters/:id/import.
            sqlx::query("UPDATE sr_managed_clusters SET status = 'running', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(task.1).execute(&self.pool).await?;
            sqlx::query("UPDATE sr_operation_tasks SET status = 'succeeded', current_step = 'complete', result_json = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(serde_json::json!({"fe_host": leader.advertise_host, "query_port": leader_query_port}).to_string())
                .bind(task_id).execute(&self.pool).await?;
            self.record(task_id, "complete", "succeeded", "Deployment completed; import it into Stellar to enable monitoring", None).await?;
            self.ensure_not_cancelled(task_id)?;
            Ok(())
        }.await;

        fs::remove_dir_all(&task_dir).await.ok();
        execution
    }

    async fn run_task(&self, task_id: i64) -> ApiResult<()> {
        if sqlx::query("UPDATE sr_operation_tasks SET status = 'running', current_step = 'validate', started_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'pending'")
            .bind(task_id).execute(&self.pool).await?.rows_affected() == 0 {
            return Ok(());
        }

        let task_type: String =
            sqlx::query_scalar("SELECT task_type FROM sr_operation_tasks WHERE id = ?")
                .bind(task_id)
                .fetch_one(&self.pool)
                .await?;
        // Only infrastructure tasks take over the managed-cluster status; node
        // commands and imports must leave the cluster status untouched.
        if task_type == "deploy" {
            sqlx::query("UPDATE sr_managed_clusters SET status = 'deploying', updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT managed_cluster_id FROM sr_operation_tasks WHERE id = ?)")
                .bind(task_id)
                .execute(&self.pool)
                .await?;
        }
        let result = tokio::time::timeout(TASK_TIMEOUT, async {
            match task_type.as_str() {
                "deploy" => self.run_deploy(task_id).await,
                "adopt_read_only" => self.run_adoption(task_id).await,
                "node_command" => self.run_node_command(task_id).await,
                "import_cluster" => self.run_import(task_id).await,
                "scale_out" => self.run_scale_out(task_id).await,
                "config_change" => self.run_config_change(task_id).await,
                "decommission" => self.run_decommission(task_id).await,
                _ => Err(ApiError::internal_error("unsupported physical operation task type")),
            }
        })
        .await
        .unwrap_or_else(|_| Err(ApiError::cluster_connection_failed("physical task timed out")));
        fs::remove_dir_all(self.work_dir.join(task_id.to_string()))
            .await
            .ok();
        self.cancel_flags
            .lock()
            .expect("cancel flag mutex")
            .remove(&task_id);
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                self.record(task_id, "failed", "failed", &error.to_string(), None)
                    .await
                    .ok();
                // A cancelled task already carries status 'cancelled'; do not
                // overwrite it with 'failed'.
                let current: String =
                    sqlx::query_scalar("SELECT status FROM sr_operation_tasks WHERE id = ?")
                        .bind(task_id)
                        .fetch_one(&self.pool)
                        .await?;
                if current == "running" {
                    sqlx::query("UPDATE sr_operation_tasks SET status = 'failed', error_message = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                        .bind(error.to_string()).bind(task_id).execute(&self.pool).await?;
                    if matches!(task_type.as_str(), "deploy" | "adopt_read_only") {
                        sqlx::query("UPDATE sr_managed_clusters SET status = 'failed', updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT managed_cluster_id FROM sr_operation_tasks WHERE id = ?)")
                            .bind(task_id).execute(&self.pool).await?;
                    }
                    // A failed scale-out must release the planned nodes and
                    // their port reservations it introduced.
                    if task_type == "scale_out" {
                        self.rollback_scale_out(task_id).await.ok();
                    }
                }
                Err(error)
            },
        }
    }

    async fn run_node_command(&self, task_id: i64) -> ApiResult<()> {
        let task: (i64, i64, String, i64) = sqlx::query_as("SELECT organization_id, managed_cluster_id, payload_json, created_by FROM sr_operation_tasks WHERE id = ?")
            .bind(task_id).fetch_one(&self.pool).await?;
        let payload: NodeCommandPayload = serde_json::from_str(&task.2)?;
        let runtime = self.load_runtime(task.1, Some(task.0)).await?;
        let node = runtime
            .frontends
            .iter()
            .chain(&runtime.backends)
            .find(|node| node.id == payload.node_id)
            .ok_or_else(|| {
                ApiError::validation_error("node does not belong to this managed cluster")
            })?
            .clone();
        let ssh_credential_id = runtime.ssh_credential_id.ok_or_else(|| {
            ApiError::validation_error("node commands require an SSH-managed cluster")
        })?;
        let (ssh_credential, private_key) = self
            .credential_service
            .ssh_secret(ssh_credential_id, task.0)
            .await?;

        let task_dir = self.work_dir.join(task_id.to_string());
        fs::create_dir_all(&task_dir).await.map_err(|error| {
            ApiError::internal_error(format!("failed to create task work directory: {error}"))
        })?;
        let execution = async {
            let host = runtime
                .hosts
                .iter()
                .find(|host| host.id == node.host_id)
                .ok_or_else(|| {
                    ApiError::internal_error("node references an unavailable physical host")
                })?;
            let context = SshContext::create(
                &task_dir,
                SshTarget {
                    id: host.id,
                    target: host.ssh_target.clone(),
                    port: host.ssh_port,
                    host_key: host.host_key.clone(),
                },
                ssh_credential.username.clone(),
                &private_key,
            )
            .await?;
            match payload.action.as_str() {
                "start" => self.start_node(task_id, &runtime.install_dir, &context, &node).await,
                "stop" => self.stop_node(task_id, &runtime.install_dir, &context, &node).await,
                "restart" => {
                    self.stop_node(task_id, &runtime.install_dir, &context, &node).await?;
                    self.start_node(task_id, &runtime.install_dir, &context, &node).await
                },
                _ => Err(ApiError::internal_error("unsupported node command")),
            }?;
            sqlx::query("UPDATE sr_operation_tasks SET status = 'succeeded', current_step = 'complete', result_json = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(serde_json::json!({"node_id": node.id, "action": payload.action}).to_string())
                .bind(task_id)
                .execute(&self.pool)
                .await?;
            self.record(task_id, "complete", "succeeded", "Node command completed", Some(node.id)).await?;
            Ok(())
        }
        .await;
        fs::remove_dir_all(&task_dir).await.ok();
        execution
    }

    async fn start_node(
        &self,
        task_id: i64,
        install_dir: &str,
        context: &SshContext,
        node: &SrClusterNode,
    ) -> ApiResult<()> {
        let (role_suffix, listen_port) = node_service_endpoint(node)?;
        self.record(task_id, "start", "running", &format!("Starting {role_suffix}"), Some(node.id))
            .await?;
        // StarRocks start scripts must daemonize or the SSH session never
        // returns; they are bash scripts, so run them with bash not /bin/sh.
        let script = format!("{install_dir}/current/{role_suffix}/bin/start_{role_suffix}.sh");
        self.executor
            .run(context, &format!("bash {} --daemon", shell_quote(&script)))
            .await?;
        self.wait_for_remote_port(context, listen_port, true)
            .await?;
        sqlx::query("UPDATE sr_cluster_nodes SET status = 'running', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(node.id)
            .execute(&self.pool)
            .await?;
        self.record(
            task_id,
            "start",
            "succeeded",
            &format!("{role_suffix} accepts connections"),
            Some(node.id),
        )
        .await?;
        Ok(())
    }

    async fn stop_node(
        &self,
        task_id: i64,
        install_dir: &str,
        context: &SshContext,
        node: &SrClusterNode,
    ) -> ApiResult<()> {
        let (role_suffix, listen_port) = node_service_endpoint(node)?;
        self.record(task_id, "stop", "running", &format!("Stopping {role_suffix}"), Some(node.id))
            .await?;
        let script = format!("{install_dir}/current/{role_suffix}/bin/stop_{role_suffix}.sh");
        self.executor
            .run(context, &format!("bash {}", shell_quote(&script)))
            .await?;
        self.wait_for_remote_port(context, listen_port, false)
            .await?;
        sqlx::query("UPDATE sr_cluster_nodes SET status = 'stopped', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(node.id)
            .execute(&self.pool)
            .await?;
        self.record(task_id, "stop", "succeeded", &format!("{role_suffix} stopped"), Some(node.id))
            .await?;
        Ok(())
    }

    async fn wait_for_remote_port(
        &self,
        context: &SshContext,
        port: i64,
        require_present: bool,
    ) -> ApiResult<()> {
        let probe = format!(
            "ss -ltnH | awk '{{print $4}}' | grep -Eq '(:|\\]){port}$' && echo up || true",
        );
        for _ in 0..NODE_PORT_RETRIES {
            let output = self.executor.run(context, &probe).await?;
            if (output.stdout.trim() == "up") == require_present {
                return Ok(());
            }
            sleep(NODE_PORT_INTERVAL).await;
        }
        Err(ApiError::cluster_connection_failed(format!(
            "port {port} on {} did not {} in time",
            context.target.target,
            if require_present { "start listening" } else { "stop listening" },
        )))
    }

    async fn run_import(&self, task_id: i64) -> ApiResult<()> {
        let task: (i64, i64, String, i64) = sqlx::query_as("SELECT organization_id, managed_cluster_id, payload_json, created_by FROM sr_operation_tasks WHERE id = ?")
            .bind(task_id).fetch_one(&self.pool).await?;
        let runtime = self.load_runtime(task.1, Some(task.0)).await?;
        let managed: (String, Option<i64>) =
            sqlx::query_as("SELECT status, cluster_id FROM sr_managed_clusters WHERE id = ?")
                .bind(task.1)
                .fetch_one(&self.pool)
                .await?;
        if managed.1.is_some() {
            return Err(ApiError::validation_error("managed cluster is already imported"));
        }
        if managed.0 != "running" {
            return Err(ApiError::validation_error(
                "only a successfully deployed managed cluster can be imported",
            ));
        }

        self.record(
            task_id,
            "register_cluster",
            "running",
            "Registering cluster with Stellar",
            None,
        )
        .await?;
        let leader = runtime
            .frontends
            .first()
            .ok_or_else(|| ApiError::internal_error("managed cluster has no leader FE"))?;
        let operator_credential_id = runtime.operator_credential_id.ok_or_else(|| {
            ApiError::internal_error("managed cluster is missing its operator credential")
        })?;
        let (operator_credential, operator_password) = self
            .credential_service
            .database_secret(operator_credential_id, task.0)
            .await?;
        let leader_query_port = required_port(leader.query_port, "leader query")?;
        let leader_http_port = required_port(leader.http_port, "leader HTTP")?;
        let cluster = self
            .cluster_service
            .create_cluster_with_activation(
                CreateClusterRequest {
                    name: runtime.name.clone(),
                    description: Some(format!("Physical deployment task #{task_id}")),
                    fe_host: leader.advertise_host.clone(),
                    fe_http_port: leader_http_port as i32,
                    fe_query_port: leader_query_port as i32,
                    username: operator_credential.username.clone(),
                    password: operator_password,
                    enable_ssl: false,
                    connection_timeout: 10,
                    tags: Some(vec!["physical-deploy".to_string()]),
                    catalog: "default_catalog".to_string(),
                    organization_id: None,
                    deployment_mode: DeploymentMode::SharedNothing,
                    cluster_type: ClusterType::StarRocks,
                    admin_user: None,
                    admin_password: None,
                },
                task.3,
                Some(task.0),
                false,
                false,
            )
            .await?;
        self.record(
            task_id,
            "register_cluster",
            "succeeded",
            "Cluster registered with Stellar",
            None,
        )
        .await?;

        sqlx::query("UPDATE sr_managed_clusters SET cluster_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(cluster.id).bind(task.1).execute(&self.pool).await?;
        sqlx::query("UPDATE sr_operation_tasks SET status = 'succeeded', current_step = 'verify', result_json = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(serde_json::json!({"cluster_id": cluster.id, "fe_host": leader.advertise_host, "query_port": leader_query_port}).to_string())
            .bind(task_id)
            .execute(&self.pool)
            .await?;
        self.record(
            task_id,
            "verify",
            "succeeded",
            "Cluster imported; existing cluster health checks are now active",
            None,
        )
        .await?;
        Ok(())
    }

    /// Rolls back nodes and port reservations a failed scale-out introduced.
    async fn rollback_scale_out(&self, task_id: i64) -> ApiResult<()> {
        let managed_cluster_id: i64 =
            sqlx::query_scalar("SELECT managed_cluster_id FROM sr_operation_tasks WHERE id = ?")
                .bind(task_id)
                .fetch_one(&self.pool)
                .await?;
        sqlx::query(
            "DELETE FROM sr_cluster_nodes WHERE managed_cluster_id = ? AND status = 'planned'",
        )
        .bind(managed_cluster_id)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "DELETE FROM sr_host_port_allocations WHERE managed_cluster_id = ? AND NOT EXISTS (\
             SELECT 1 FROM sr_cluster_nodes n WHERE n.managed_cluster_id = sr_host_port_allocations.managed_cluster_id \
             AND n.status != 'planned' AND (n.service_port = sr_host_port_allocations.port \
             OR n.http_port = sr_host_port_allocations.port OR n.query_port = sr_host_port_allocations.port \
             OR n.rpc_port = sr_host_port_allocations.port OR n.brpc_port = sr_host_port_allocations.port \
             OR n.webserver_port = sr_host_port_allocations.port OR n.starlet_port = sr_host_port_allocations.port))",
        )
        .bind(managed_cluster_id)
        .execute(&self.pool)
        .await?;
        sqlx::query("DELETE FROM sr_host_allocations WHERE managed_cluster_id = ?")
            .bind(managed_cluster_id)
            .execute(&self.pool)
            .await?;
        tracing::warn!(task_id, managed_cluster_id, "rolled back failed scale-out nodes");
        Ok(())
    }

    async fn ensure_no_running_task(&self, managed_cluster_id: i64) -> ApiResult<()> {
        let running: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sr_operation_tasks WHERE managed_cluster_id = ? AND status IN ('pending', 'running')",
        )
        .bind(managed_cluster_id)
        .fetch_one(&self.pool)
        .await?;
        if running > 0 {
            return Err(ApiError::validation_error(
                "another operation is already running for this managed cluster",
            ));
        }
        Ok(())
    }

    pub async fn submit_decommission(
        self: &Arc<Self>,
        managed_cluster_id: i64,
        request: DecommissionRequest,
        organization_id: Option<i64>,
        user_id: i64,
    ) -> ApiResult<SrOperationTask> {
        let request = request.normalize()?;
        let managed: Option<(String, String, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT name, status, cluster_id, ssh_credential_id FROM sr_managed_clusters WHERE id = ? AND (? IS NULL OR organization_id = ?)",
        )
        .bind(managed_cluster_id)
        .bind(organization_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((name, status, cluster_id, ssh_credential_id)) = managed else {
            return Err(ApiError::not_found("managed cluster not found"));
        };
        if request.confirm != name {
            return Err(ApiError::validation_error(
                "confirm must exactly match the managed cluster name",
            ));
        }
        if status == "removed" {
            return Err(ApiError::validation_error("managed cluster is already removed"));
        }
        if ssh_credential_id.is_none() && request.remove_remote_files {
            return Err(ApiError::validation_error(
                "adopted clusters have no recorded hosts; remove_remote_files is not supported",
            ));
        }
        self.ensure_no_running_task(managed_cluster_id).await?;

        let payload = DecommissionPayload {
            remove_remote_files: request.remove_remote_files,
            deregister: request.deregister && cluster_id.is_some(),
        };
        let payload_json = serde_json::to_string(&payload)?;
        let task_id = sqlx::query(
            "INSERT INTO sr_operation_tasks (organization_id, managed_cluster_id, task_type, payload_json, created_by) VALUES (?, ?, 'decommission', ?, ?)",
        )
        .bind(
            organization_id
                .ok_or_else(|| ApiError::validation_error("organization is required"))?,
        )
        .bind(managed_cluster_id)
        .bind(payload_json)
        .bind(user_id)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        let task = self.get_task_for_org(task_id, organization_id).await?.task;
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = service.run_task(task_id).await {
                tracing::error!(task_id, error = %error, "Decommission task failed");
            }
        });
        Ok(task)
    }

    async fn run_decommission(&self, task_id: i64) -> ApiResult<()> {
        let task: (i64, i64, String, i64) = sqlx::query_as("SELECT organization_id, managed_cluster_id, payload_json, created_by FROM sr_operation_tasks WHERE id = ?")
            .bind(task_id).fetch_one(&self.pool).await?;
        let payload: DecommissionPayload = serde_json::from_str(&task.2)?;
        let runtime = self.load_runtime(task.1, Some(task.0)).await?;
        let task_dir = self.work_dir.join(task_id.to_string());
        fs::create_dir_all(&task_dir).await.map_err(|error| {
            ApiError::internal_error(format!("failed to create task work directory: {error}"))
        })?;
        let execution = async {
            let mut contexts: HashMap<i64, SshContext> = HashMap::new();
            if runtime.ssh_credential_id.is_some() {
                let ssh_credential_id = runtime
                    .ssh_credential_id
                    .ok_or_else(|| ApiError::internal_error("decommission requires an SSH credential"))?;
                let (ssh_credential, private_key) = self
                    .credential_service
                    .ssh_secret(ssh_credential_id, task.0)
                    .await?;
                self.record(task_id, "stop_nodes", "running", "Stopping managed nodes", None).await?;
                for host in &runtime.hosts {
                    let context = SshContext::create(
                        &task_dir,
                        SshTarget {
                            id: host.id,
                            target: host.ssh_target.clone(),
                            port: host.ssh_port,
                            host_key: host.host_key.clone(),
                        },
                        ssh_credential.username.clone(),
                        &private_key,
                    )
                    .await?;
                    contexts.insert(host.id, context);
                }
                let nodes: Vec<&SrClusterNode> =
                    runtime.frontends.iter().chain(&runtime.backends).collect();
                for node in nodes {
                    self.ensure_not_cancelled(task_id)?;
                    let context = contexts.get(&node.host_id).ok_or_else(|| {
                        ApiError::internal_error("node references an unavailable physical host")
                    })?;
                    let (role_suffix, listen_port) = node_service_endpoint(node)?;
                    let script = format!(
                        "{}/current/{role_suffix}/bin/stop_{role_suffix}.sh",
                        runtime.install_dir,
                    );
                    if let Err(error) = self
                        .executor
                        .run(context, &format!("bash {}", shell_quote(&script)))
                        .await
                    {
                        // Already stopped or already removed: proceed.
                        self.record(
                            task_id,
                            "stop_nodes",
                            "skipped",
                            &format!("stop_{role_suffix} on {} reported: {error}", node.advertise_host),
                            Some(node.id),
                        )
                        .await?;
                    }
                    self.wait_for_remote_port(context, listen_port, false)
                        .await?;
                }
                self.record(task_id, "stop_nodes", "succeeded", "All nodes stopped", None).await?;

                if payload.remove_remote_files {
                    self.ensure_not_cancelled(task_id)?;
                    self.record(task_id, "remove_files", "running", "Removing install and data directories", None).await?;
                    for host in &runtime.hosts {
                        let context = contexts.get(&host.id).ok_or_else(|| {
                            ApiError::internal_error("decommission host context missing")
                        })?;
                        self.executor
                            .run(context, &format!("rm -rf {}", shell_quote(&runtime.install_dir)))
                            .await?;
                    }
                    for node in runtime.frontends.iter() {
                        if let Some(meta_dir) = node.meta_dir.as_deref() {
                            let context = contexts.get(&node.host_id).ok_or_else(|| {
                                ApiError::internal_error("node host context missing")
                            })?;
                            self.executor
                                .run(context, &format!("rm -rf {}", shell_quote(meta_dir)))
                                .await?;
                        }
                    }
                    for node in runtime.backends.iter() {
                        if let Some(storage_dir) = node.storage_dir.as_deref() {
                            let context = contexts.get(&node.host_id).ok_or_else(|| {
                                ApiError::internal_error("node host context missing")
                            })?;
                            self.executor
                                .run(context, &format!("rm -rf {}", shell_quote(storage_dir)))
                                .await?;
                        }
                    }
                    self.record(task_id, "remove_files", "succeeded", "Remote install and data directories removed", None).await?;
                }
            }

            if let Some(cluster_id) = self
                .registered_cluster_id(task.1)
                .await?
                .filter(|_| payload.deregister)
            {
                self.ensure_not_cancelled(task_id)?;
                self.record(task_id, "deregister", "running", "Removing imported clusters row", None).await?;
                self.cluster_service.delete_cluster(cluster_id).await?;
                self.record(task_id, "deregister", "succeeded", "Imported clusters row removed", None).await?;
            }

            sqlx::query("UPDATE sr_cluster_nodes SET status = 'removed', updated_at = CURRENT_TIMESTAMP WHERE managed_cluster_id = ?")
                .bind(task.1)
                .execute(&self.pool)
                .await?;
            sqlx::query("DELETE FROM sr_host_port_allocations WHERE managed_cluster_id = ?")
                .bind(task.1)
                .execute(&self.pool)
                .await?;
            sqlx::query("DELETE FROM sr_host_allocations WHERE managed_cluster_id = ?")
                .bind(task.1)
                .execute(&self.pool)
                .await?;
            sqlx::query("UPDATE sr_managed_clusters SET status = 'removed', cluster_id = CASE WHEN ? THEN NULL ELSE cluster_id END, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(payload.deregister)
                .bind(task.1)
                .execute(&self.pool)
                .await?;
            sqlx::query("UPDATE sr_operation_tasks SET status = 'succeeded', current_step = 'complete', result_json = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(serde_json::json!({"remove_remote_files": payload.remove_remote_files, "deregister": payload.deregister}).to_string())
                .bind(task_id)
                .execute(&self.pool)
                .await?;
            self.record(task_id, "complete", "succeeded", "Managed cluster decommissioned; port allocations released", None).await?;
            Ok(())
        }
        .await;
        fs::remove_dir_all(&task_dir).await.ok();
        execution
    }

    async fn registered_cluster_id(&self, managed_cluster_id: i64) -> ApiResult<Option<i64>> {
        sqlx::query_scalar("SELECT cluster_id FROM sr_managed_clusters WHERE id = ?")
            .bind(managed_cluster_id)
            .fetch_optional(&self.pool)
            .await?
            .flatten()
            .map(Ok)
            .transpose()
    }

    pub async fn submit_scale_out(
        self: &Arc<Self>,
        managed_cluster_id: i64,
        request: NodeScaleRequest,
        organization_id: Option<i64>,
        user_id: i64,
    ) -> ApiResult<SrOperationTask> {
        let request = request.normalize()?;
        let managed: Option<(String, Option<i64>, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT status, cluster_id, ssh_credential_id, package_id FROM sr_managed_clusters WHERE id = ? AND (? IS NULL OR organization_id = ?)",
        )
        .bind(managed_cluster_id)
        .bind(organization_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((status, _cluster_id, ssh_credential_id, package_id)) = managed else {
            return Err(ApiError::not_found("managed cluster not found"));
        };
        if status != "running" {
            return Err(ApiError::validation_error("scale-out requires a running managed cluster"));
        }
        if ssh_credential_id.is_none() {
            return Err(ApiError::validation_error("scale-out requires an SSH-managed cluster"));
        }
        let package_id = package_id.ok_or_else(|| {
            ApiError::internal_error("managed cluster is missing its package reference")
        })?;
        let host_ids: HashSet<i64> = request
            .frontends
            .iter()
            .map(|node| node.host_id)
            .chain(request.backends.iter().map(|node| node.host_id))
            .collect();
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM physical_hosts WHERE (? IS NULL OR organization_id = (SELECT organization_id FROM sr_managed_clusters WHERE id = ?)) AND id IN (SELECT value FROM json_each(?))",
        )
        .bind(organization_id)
        .bind(managed_cluster_id)
        .bind(serde_json::to_string(&host_ids)?)
        .fetch_one(&self.pool)
        .await?;
        if count != host_ids.len() as i64 {
            return Err(ApiError::validation_error(
                "every scale-out host must belong to the cluster organization",
            ));
        }
        // Fail early on ports already reserved by any managed cluster; the
        // runner repeats the check inside a transaction against concurrent
        // submissions.
        let mut ports: Vec<i64> = Vec::new();
        for node in request.frontends.iter() {
            ports.extend([
                i64::from(node.edit_log_port),
                i64::from(node.http_port),
                i64::from(node.query_port),
                i64::from(node.rpc_port),
            ]);
        }
        for node in request.backends.iter() {
            ports.extend([
                i64::from(node.heartbeat_port),
                i64::from(node.be_port),
                i64::from(node.webserver_port),
                i64::from(node.brpc_port),
                i64::from(node.starlet_port),
            ]);
        }
        for host_id in &host_ids {
            for port in &ports {
                let taken: Option<i64> = sqlx::query_scalar(
                    "SELECT host_id FROM sr_host_port_allocations WHERE host_id = ? AND port = ?",
                )
                .bind(host_id)
                .bind(port)
                .fetch_optional(&self.pool)
                .await?;
                if taken.is_some() {
                    return Err(ApiError::validation_error(format!(
                        "port {port} on host {host_id} is already reserved"
                    )));
                }
            }
        }
        self.ensure_no_running_task(managed_cluster_id).await?;

        let package: (String, String, Option<String>) =
            sqlx::query_as("SELECT package_url, sha256, local_path FROM sr_packages WHERE id = ?")
                .bind(package_id)
                .fetch_one(&self.pool)
                .await?;
        let payload = ScaleOutPayload {
            package_id,
            package_url: package.0,
            sha256: package.1,
            local_path: package.2,
            frontends: request.frontends,
            backends: request.backends,
        };
        let payload_json = serde_json::to_string(&payload)?;
        let task_id = sqlx::query(
            "INSERT INTO sr_operation_tasks (organization_id, managed_cluster_id, task_type, payload_json, created_by) VALUES (?, ?, 'scale_out', ?, ?)",
        )
        .bind(
            organization_id
                .ok_or_else(|| ApiError::validation_error("organization is required"))?,
        )
        .bind(managed_cluster_id)
        .bind(payload_json)
        .bind(user_id)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        let task = self.get_task_for_org(task_id, organization_id).await?.task;
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = service.run_task(task_id).await {
                tracing::error!(task_id, error = %error, "Scale-out task failed");
            }
        });
        Ok(task)
    }

    async fn run_scale_out(&self, task_id: i64) -> ApiResult<()> {
        let task: (i64, i64, String, i64) = sqlx::query_as("SELECT organization_id, managed_cluster_id, payload_json, created_by FROM sr_operation_tasks WHERE id = ?")
            .bind(task_id).fetch_one(&self.pool).await?;
        let payload: ScaleOutPayload = serde_json::from_str(&task.2)?;
        let runtime = self.load_runtime(task.1, Some(task.0)).await?;
        let ssh_credential_id = runtime
            .ssh_credential_id
            .ok_or_else(|| ApiError::internal_error("scale-out requires an SSH-managed cluster"))?;
        let (ssh_credential, private_key) = self
            .credential_service
            .ssh_secret(ssh_credential_id, task.0)
            .await?;
        let operator_credential_id = runtime.operator_credential_id.ok_or_else(|| {
            ApiError::internal_error("managed cluster is missing its operator credential")
        })?;
        let (operator_credential, operator_password) = self
            .credential_service
            .database_secret(operator_credential_id, task.0)
            .await?;
        let operator_auth = SqlAuth {
            user: sql_identifier(&operator_credential.username)?.to_string(),
            password: Some(operator_password.clone()),
        };
        let leader = runtime
            .frontends
            .first()
            .ok_or_else(|| ApiError::internal_error("managed cluster has no leader FE"))?;
        let leader_query_port = required_port(leader.query_port, "leader query")?;

        // Resolve the new physical hosts before registering nodes.
        let host_ids: HashSet<i64> = payload
            .frontends
            .iter()
            .map(|node| node.host_id)
            .chain(payload.backends.iter().map(|node| node.host_id))
            .collect();
        let new_hosts: Vec<RuntimeHost> = sqlx::query_as(
            "SELECT DISTINCT id, hostname, ssh_target, ssh_port, host_key FROM physical_hosts WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id",
        )
        .bind(serde_json::to_string(&host_ids)?)
        .fetch_all(&self.pool)
        .await?;
        let mut tx = self.pool.begin().await?;
        let mut new_frontends: Vec<SrClusterNode> = Vec::new();
        for node in payload.frontends.iter() {
            let now = chrono::Utc::now();
            let id = sqlx::query(
                "INSERT INTO sr_cluster_nodes (managed_cluster_id, host_id, role, fe_role, advertise_host, service_port, http_port, query_port, rpc_port, meta_dir) VALUES (?, ?, 'fe', ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(task.1)
            .bind(node.host_id)
            .bind("follower")
            .bind(&node.advertise_host)
            .bind(i64::from(node.edit_log_port))
            .bind(i64::from(node.http_port))
            .bind(i64::from(node.query_port))
            .bind(i64::from(node.rpc_port))
            .bind(node.meta_dir.as_deref())
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
            new_frontends.push(SrClusterNode {
                id,
                managed_cluster_id: task.1,
                host_id: node.host_id,
                role: "fe".to_string(),
                fe_role: Some("follower".to_string()),
                advertise_host: node.advertise_host.clone(),
                service_port: i64::from(node.edit_log_port),
                http_port: Some(i64::from(node.http_port)),
                query_port: Some(i64::from(node.query_port)),
                rpc_port: Some(i64::from(node.rpc_port)),
                brpc_port: None,
                webserver_port: None,
                starlet_port: None,
                meta_dir: node.meta_dir.clone(),
                storage_dir: None,
                status: "planned".to_string(),
                created_at: now,
                updated_at: now,
            });
        }
        let mut new_backends: Vec<SrClusterNode> = Vec::new();
        for node in payload.backends.iter() {
            let now = chrono::Utc::now();
            let id = sqlx::query(
                "INSERT INTO sr_cluster_nodes (managed_cluster_id, host_id, role, advertise_host, service_port, http_port, brpc_port, webserver_port, starlet_port, storage_dir) VALUES (?, ?, 'be', ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(task.1)
            .bind(node.host_id)
            .bind(&node.advertise_host)
            .bind(i64::from(node.heartbeat_port))
            .bind(i64::from(node.be_port))
            .bind(i64::from(node.brpc_port))
            .bind(i64::from(node.webserver_port))
            .bind(i64::from(node.starlet_port))
            .bind(node.storage_dir.as_deref())
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
            new_backends.push(SrClusterNode {
                id,
                managed_cluster_id: task.1,
                host_id: node.host_id,
                role: "be".to_string(),
                fe_role: None,
                advertise_host: node.advertise_host.clone(),
                service_port: i64::from(node.heartbeat_port),
                http_port: Some(i64::from(node.be_port)),
                query_port: None,
                rpc_port: None,
                brpc_port: Some(i64::from(node.brpc_port)),
                webserver_port: Some(i64::from(node.webserver_port)),
                starlet_port: Some(i64::from(node.starlet_port)),
                meta_dir: None,
                storage_dir: node.storage_dir.clone(),
                status: "planned".to_string(),
                created_at: now,
                updated_at: now,
            });
        }
        tx.commit().await?;

        // Each new host gets its own installation tree: StarRocks start/stop
        // scripts key off a pidfile inside the install directory, so sharing
        // the cluster tree would make a second BE on one host unstartable.
        let scale_install_dirs: HashMap<i64, String> =
            incremental_hosts(&host_ids, &runtime.install_dir);
        let single_be = runtime.backends.len() + new_backends.len() == 1;
        let incremental = RuntimeCluster {
            name: runtime.name.clone(),
            install_dir: runtime.install_dir.clone(),
            sr_version: runtime.sr_version.clone(),
            ssh_credential_id: runtime.ssh_credential_id,
            operator_credential_id: runtime.operator_credential_id,
            sha256: payload.sha256.clone(),
            hosts: new_hosts,
            frontends: new_frontends.clone(),
            backends: new_backends.clone(),
        };

        let task_dir = self.work_dir.join(task_id.to_string());
        fs::create_dir_all(&task_dir).await.map_err(|error| {
            ApiError::internal_error(format!("failed to create task work directory: {error}"))
        })?;
        let execution = async {
            self.record(task_id, "precheck", "running", "Checking SSH, disk, Java, and ports on new hosts", None).await?;
            let contexts = self
                .precheck(task_id, &incremental, &ssh_credential.username, &private_key, &task_dir)
                .await?;
            self.record(task_id, "precheck", "succeeded", "New hosts passed precheck", None).await?;

            self.ensure_not_cancelled(task_id)?;
            self.reserve_ports(task.1, &incremental).await?;
            self.record(task_id, "reserve", "succeeded", "New host service ports reserved", None).await?;

            self.ensure_not_cancelled(task_id)?;
            let deploy_payload = DeploymentPayload {
                package_id: payload.package_id,
                sr_version: runtime.sr_version.clone(),
                package_url: payload.package_url.clone(),
                sha256: payload.sha256.clone(),
                local_path: payload.local_path.clone(),
                bootstrap_credential_id: None,
                frontends: payload.frontends.clone(),
                backends: payload.backends.clone(),
            };
            let archive = self.cache_package(task_id, &deploy_payload).await?;
            let archive_root = validate_archive(&archive)?;
            self.record(task_id, "cache_package", "succeeded", "Installation package verified from cache or source", None).await?;

            self.ensure_not_cancelled(task_id)?;
            self.distribute_and_install(task_id, &incremental, &contexts, &archive, &archive_root, &scale_install_dirs).await?;
            self.record(task_id, "install", "succeeded", "Package installed on new hosts", None).await?;

            self.ensure_not_cancelled(task_id)?;
            for frontend in &new_frontends {
                let node_install_dir = scale_install_dirs
                    .get(&frontend.host_id)
                    .cloned()
                    .unwrap_or_else(|| runtime.install_dir.clone());
                let context = contexts.get(&frontend.host_id).unwrap();
                let path = format!("{node_install_dir}/current/fe/conf/fe.conf");
                let existing = self
                    .executor
                    .run(context, &format!("cat {}", shell_quote(&path)))
                    .await?
                    .stdout;
                let config = fe_config(
                    &existing,
                    FeConfig {
                        advertise_host: &frontend.advertise_host,
                        meta_dir: frontend.meta_dir.as_deref().unwrap(),
                        edit_log_port: frontend.service_port,
                        http_port: required_port(frontend.http_port, "FE HTTP")?,
                        query_port: required_port(frontend.query_port, "FE query")?,
                        rpc_port: required_port(frontend.rpc_port, "FE RPC")?,
                        single_be,
                    },
                );
                write_remote_file(self.executor.as_ref(), context, &path, &config).await?;
                self.record_config_revision(task_id, task.1, task.3, frontend.id, &config).await?;
            }
            for backend in &new_backends {
                let node_install_dir = scale_install_dirs
                    .get(&backend.host_id)
                    .cloned()
                    .unwrap_or_else(|| runtime.install_dir.clone());
                let context = contexts.get(&backend.host_id).unwrap();
                let path = format!("{node_install_dir}/current/be/conf/be.conf");
                let existing = self
                    .executor
                    .run(context, &format!("cat {}", shell_quote(&path)))
                    .await?
                    .stdout;
                let config = be_config(
                    &existing,
                    &backend.advertise_host,
                    backend.storage_dir.as_deref().unwrap(),
                    backend.service_port,
                    required_port(backend.http_port, "BE port")?,
                    required_port(backend.webserver_port, "BE webserver")?,
                    required_port(backend.brpc_port, "BE brpc")?,
                    required_port(backend.starlet_port, "BE starlet")?,
                );
                write_remote_file(self.executor.as_ref(), context, &path, &config).await?;
                self.record_config_revision(task_id, task.1, task.3, backend.id, &config).await?;
            }
            self.record(task_id, "render_config", "succeeded", "Configurations written for new nodes", None).await?;

            self.ensure_not_cancelled(task_id)?;
            for frontend in &new_frontends {
                if !node_exists(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/frontends'",
                    &frontend.advertise_host,
                    frontend.service_port,
                    &operator_auth,
                )
                .await?
                {
                    execute_sql(
                        &leader.advertise_host,
                        leader_query_port,
                        &[
                            "SET ROLE cluster_admin".to_string(),
                            format!("ALTER SYSTEM ADD FOLLOWER \"{}:{}\"", frontend.advertise_host, frontend.service_port),
                        ],
                        &operator_auth,
                    )
                    .await?;
                }
                let context = contexts.get(&frontend.host_id).ok_or_else(|| {
                    ApiError::internal_error("follower SSH context missing")
                })?;
                let node_install_dir = scale_install_dirs
                    .get(&frontend.host_id)
                    .cloned()
                    .unwrap_or_else(|| runtime.install_dir.clone());
                self.executor
                    .run(context, &format!("bash {}/current/fe/bin/start_fe.sh --helper {}:{} --daemon", shell_quote(&node_install_dir), leader.advertise_host, leader.service_port))
                    .await?;
                wait_for_node(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/frontends'",
                    &frontend.advertise_host,
                    frontend.service_port,
                    &operator_auth,
                )
                .await?;
                self.ensure_not_cancelled(task_id)?;
            }
            for backend in &new_backends {
                let context = contexts.get(&backend.host_id).ok_or_else(|| {
                    ApiError::internal_error("BE SSH context missing")
                })?;
                let node_install_dir = scale_install_dirs
                    .get(&backend.host_id)
                    .cloned()
                    .unwrap_or_else(|| runtime.install_dir.clone());
                self.executor
                    .run(context, &format!("bash {}/current/be/bin/start_be.sh --daemon", shell_quote(&node_install_dir)))
                    .await?;
                if !node_exists(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/backends'",
                    &backend.advertise_host,
                    backend.service_port,
                    &operator_auth,
                )
                .await?
                {
                    execute_sql(
                        &leader.advertise_host,
                        leader_query_port,
                        &[
                            "SET ROLE cluster_admin".to_string(),
                            format!("ALTER SYSTEM ADD BACKEND \"{}:{}\"", backend.advertise_host, backend.service_port),
                        ],
                        &operator_auth,
                    )
                    .await?;
                }
                wait_for_node(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/backends'",
                    &backend.advertise_host,
                    backend.service_port,
                    &operator_auth,
                )
                .await?;
                self.ensure_not_cancelled(task_id)?;
            }
            self.record(task_id, "add_nodes", "succeeded", "New nodes joined the cluster and are alive", None).await?;

            sqlx::query("UPDATE sr_cluster_nodes SET status = 'running', updated_at = CURRENT_TIMESTAMP WHERE id IN (SELECT value FROM json_each(?))")
                .bind(serde_json::to_string(
                    &new_frontends.iter().map(|node| node.id).chain(new_backends.iter().map(|node| node.id)).collect::<Vec<_>>(),
                )?)
                .execute(&self.pool)
                .await?;
            sqlx::query("UPDATE sr_operation_tasks SET status = 'succeeded', current_step = 'complete', result_json = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(serde_json::json!({"added_fe": new_frontends.len(), "added_be": new_backends.len()}).to_string())
                .bind(task_id)
                .execute(&self.pool)
                .await?;
            self.record(task_id, "complete", "succeeded", "Scale-out completed", None).await?;
            Ok(())
        }
        .await;
        fs::remove_dir_all(&task_dir).await.ok();
        execution
    }

    pub async fn submit_node_config_change(
        self: &Arc<Self>,
        managed_cluster_id: i64,
        node_id: i64,
        request: NodeConfigUpdateRequest,
        organization_id: Option<i64>,
        user_id: i64,
    ) -> ApiResult<SrOperationTask> {
        let request = request.normalize()?;
        let managed: Option<(String, Option<i64>)> = sqlx::query_as(
            "SELECT status, ssh_credential_id FROM sr_managed_clusters WHERE id = ? AND (? IS NULL OR organization_id = ?)",
        )
        .bind(managed_cluster_id)
        .bind(organization_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((_, ssh_credential_id)) = managed else {
            return Err(ApiError::not_found("managed cluster not found"));
        };
        if ssh_credential_id.is_none() {
            return Err(ApiError::validation_error(
                "config changes require an SSH-managed cluster",
            ));
        }
        let node: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM sr_cluster_nodes WHERE id = ? AND managed_cluster_id = ?",
        )
        .bind(node_id)
        .bind(managed_cluster_id)
        .fetch_optional(&self.pool)
        .await?;
        if node.is_none() {
            return Err(ApiError::validation_error("node does not belong to this managed cluster"));
        }
        self.ensure_no_running_task(managed_cluster_id).await?;

        let payload =
            ConfigChangePayload { node_id, content: request.content, restart: request.restart };
        let payload_json = serde_json::to_string(&payload)?;
        let task_id = sqlx::query(
            "INSERT INTO sr_operation_tasks (organization_id, managed_cluster_id, task_type, payload_json, created_by) VALUES (?, ?, 'config_change', ?, ?)",
        )
        .bind(
            organization_id
                .ok_or_else(|| ApiError::validation_error("organization is required"))?,
        )
        .bind(managed_cluster_id)
        .bind(payload_json)
        .bind(user_id)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        let task = self.get_task_for_org(task_id, organization_id).await?.task;
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = service.run_task(task_id).await {
                tracing::error!(task_id, error = %error, "Config change task failed");
            }
        });
        Ok(task)
    }

    async fn run_config_change(&self, task_id: i64) -> ApiResult<()> {
        let task: (i64, i64, String, i64) = sqlx::query_as("SELECT organization_id, managed_cluster_id, payload_json, created_by FROM sr_operation_tasks WHERE id = ?")
            .bind(task_id).fetch_one(&self.pool).await?;
        let payload: ConfigChangePayload = serde_json::from_str(&task.2)?;
        let runtime = self.load_runtime(task.1, Some(task.0)).await?;
        let ssh_credential_id = runtime.ssh_credential_id.ok_or_else(|| {
            ApiError::internal_error("config changes require an SSH-managed cluster")
        })?;
        let node = runtime
            .frontends
            .iter()
            .chain(&runtime.backends)
            .find(|node| node.id == payload.node_id)
            .ok_or_else(|| {
                ApiError::validation_error("node does not belong to this managed cluster")
            })?
            .clone();
        let (ssh_credential, private_key) = self
            .credential_service
            .ssh_secret(ssh_credential_id, task.0)
            .await?;

        let task_dir = self.work_dir.join(task_id.to_string());
        fs::create_dir_all(&task_dir).await.map_err(|error| {
            ApiError::internal_error(format!("failed to create task work directory: {error}"))
        })?;
        let execution = async {
            self.ensure_not_cancelled(task_id)?;
            let context = SshContext::create(
                &task_dir,
                SshTarget {
                    id: node.host_id,
                    target: runtime
                        .hosts
                        .iter()
                        .find(|host| host.id == node.host_id)
                        .ok_or_else(|| {
                            ApiError::internal_error("node references an unavailable physical host")
                        })?
                        .ssh_target
                        .clone(),
                    port: runtime
                        .hosts
                        .iter()
                        .find(|host| host.id == node.host_id)
                        .ok_or_else(|| {
                            ApiError::internal_error("node references an unavailable physical host")
                        })?
                        .ssh_port,
                    host_key: runtime
                        .hosts
                        .iter()
                        .find(|host| host.id == node.host_id)
                        .ok_or_else(|| {
                            ApiError::internal_error("node references an unavailable physical host")
                        })?
                        .host_key
                        .clone(),
                },
                ssh_credential.username.clone(),
                &private_key,
            )
            .await?;

            let (role_suffix, listen_port) = node_service_endpoint(&node)?;
            let path = format!(
                "{}/current/{role_suffix}/conf/{role_suffix}.conf",
                runtime.install_dir,
            );
            let existing = self
                .executor
                .run(&context, &format!("cat {}", shell_quote(&path)))
                .await?
                .stdout;
            validate_config_topology(&existing, &payload.content, &node.role)?;

            self.record(task_id, "write_config", "running", "Writing validated configuration", Some(node.id)).await?;
            write_remote_file(self.executor.as_ref(), &context, &path, &payload.content).await?;
            self.record_config_revision(task_id, task.1, task.3, node.id, &payload.content).await?;
            self.record(task_id, "write_config", "succeeded", "Configuration written and versioned", Some(node.id)).await?;

            if payload.restart {
                self.ensure_not_cancelled(task_id)?;
                self.record(task_id, "restart", "running", "Restarting node to apply static keys", Some(node.id)).await?;
                self.stop_node(task_id, &runtime.install_dir, &context, &node).await?;
                self.start_node(task_id, &runtime.install_dir, &context, &node).await?;
            }

            sqlx::query("UPDATE sr_operation_tasks SET status = 'succeeded', current_step = 'complete', result_json = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(serde_json::json!({"node_id": node.id, "restart": payload.restart}).to_string())
                .bind(task_id)
                .execute(&self.pool)
                .await?;
            self.record(task_id, "complete", "succeeded", "Configuration change completed", Some(node.id)).await?;
            let _ = (role_suffix, listen_port);
            Ok(())
        }
        .await;
        fs::remove_dir_all(&task_dir).await.ok();
        execution
    }

    pub async fn refresh_cluster_status(
        &self,
        managed_cluster_id: i64,
        organization_id: Option<i64>,
    ) -> ApiResult<serde_json::Value> {
        let runtime = self
            .load_runtime(managed_cluster_id, organization_id)
            .await?;
        let ssh_credential_id = runtime.ssh_credential_id.ok_or_else(|| {
            ApiError::validation_error("status refresh requires an SSH-managed cluster")
        })?;
        let ssh_organization_id = match organization_id {
            Some(organization_id) => organization_id,
            None => {
                sqlx::query_scalar("SELECT organization_id FROM ssh_credentials WHERE id = ?")
                    .bind(ssh_credential_id)
                    .fetch_one(&self.pool)
                    .await?
            },
        };
        let (ssh_credential, private_key) = self
            .credential_service
            .ssh_secret(ssh_credential_id, ssh_organization_id)
            .await?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        let task_dir = self
            .work_dir
            .join(format!("refresh-{managed_cluster_id}-{nonce}"));
        let execution = async {
            let mut summaries: Vec<serde_json::Value> = Vec::new();
            for host in &runtime.hosts {
                let context = SshContext::create(
                    &task_dir,
                    SshTarget {
                        id: host.id,
                        target: host.ssh_target.clone(),
                        port: host.ssh_port,
                        host_key: host.host_key.clone(),
                    },
                    ssh_credential.username.clone(),
                    &private_key,
                )
                .await?;
                let listening = self
                    .executor
                    .run(&context, "ss -ltnH | awk '{print $4}'")
                    .await?
                    .stdout;
                for node in runtime
                    .frontends
                    .iter()
                    .chain(&runtime.backends)
                    .filter(|node| node.host_id == host.id)
                {
                    let port = node_service_endpoint(node)?.1;
                    let alive = listening
                        .lines()
                        .any(|line| line.trim_end().ends_with(&format!(":{port}")));
                    let status = if alive { "running" } else { "stopped" };
                    sqlx::query("UPDATE sr_cluster_nodes SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                        .bind(status)
                        .bind(node.id)
                        .execute(&self.pool)
                        .await?;
                    summaries.push(serde_json::json!({
                        "node_id": node.id,
                        "role": node.role,
                        "advertise_host": node.advertise_host,
                        "port": port,
                        "status": status,
                    }));
                }
            }
            Ok(serde_json::json!({ "managed_cluster_id": managed_cluster_id, "nodes": summaries }))
        }
        .await;
        fs::remove_dir_all(&task_dir).await.ok();
        execution
    }

    pub async fn read_node_logs(
        &self,
        managed_cluster_id: i64,
        organization_id: Option<i64>,
        node_id: i64,
        file: &str,
        lines: i64,
    ) -> ApiResult<String> {
        let runtime = self
            .load_runtime(managed_cluster_id, organization_id)
            .await?;
        let node = runtime
            .frontends
            .iter()
            .chain(&runtime.backends)
            .find(|node| node.id == node_id)
            .ok_or_else(|| ApiError::not_found("node not found"))?;
        let ssh_credential_id = runtime
            .ssh_credential_id
            .ok_or_else(|| ApiError::validation_error("logs require an SSH-managed cluster"))?;
        let ssh_organization_id = match organization_id {
            Some(organization_id) => organization_id,
            None => {
                sqlx::query_scalar("SELECT organization_id FROM ssh_credentials WHERE id = ?")
                    .bind(ssh_credential_id)
                    .fetch_one(&self.pool)
                    .await?
            },
        };
        let file = file.trim();
        let allowed = match node.role.as_str() {
            "fe" => matches!(file, "fe.log" | "fe.warn"),
            _ => matches!(file, "be.INFO" | "be.WARN" | "be.out"),
        };
        if !allowed {
            return Err(ApiError::validation_error(
                "log file must be fe.log, fe.warn, be.INFO, be.WARN, or be.out",
            ));
        }
        let lines = lines.clamp(1, 1_000);
        let log_dir = match node.role.as_str() {
            "fe" => "current/fe/log",
            _ => "current/be/log",
        };
        let path = format!("{}/{log_dir}/{}", runtime.install_dir, file);

        let (ssh_credential, private_key) = self
            .credential_service
            .ssh_secret(ssh_credential_id, ssh_organization_id)
            .await?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        let task_dir = self
            .work_dir
            .join(format!("node-logs-{managed_cluster_id}-{node_id}-{nonce}"));
        let execution = async {
            let host = runtime
                .hosts
                .iter()
                .find(|host| host.id == node.host_id)
                .ok_or_else(|| {
                    ApiError::internal_error("node references an unavailable physical host")
                })?;
            let context = SshContext::create(
                &task_dir,
                SshTarget {
                    id: host.id,
                    target: host.ssh_target.clone(),
                    port: host.ssh_port,
                    host_key: host.host_key.clone(),
                },
                ssh_credential.username.clone(),
                &private_key,
            )
            .await?;
            Ok(self
                .executor
                .run(&context, &format!("tail -n {lines} {}", shell_quote(&path)))
                .await?
                .stdout)
        }
        .await;
        fs::remove_dir_all(&task_dir).await.ok();
        execution
    }

    pub async fn list_config_revisions(
        &self,
        managed_cluster_id: i64,
        organization_id: Option<i64>,
        node_id: i64,
    ) -> ApiResult<Vec<SrConfigRevisionSummary>> {
        self.ensure_node_scope(managed_cluster_id, organization_id, node_id)
            .await?;
        Ok(sqlx::query_as(
            "SELECT id, node_id, revision, content_sha256, task_id, created_at FROM sr_config_revisions WHERE node_id = ? ORDER BY revision DESC",
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn get_config_revision(
        &self,
        managed_cluster_id: i64,
        organization_id: Option<i64>,
        node_id: i64,
        revision: i64,
    ) -> ApiResult<SrConfigRevision> {
        self.ensure_node_scope(managed_cluster_id, organization_id, node_id)
            .await?;
        sqlx::query_as(
            "SELECT id, node_id, revision, content, content_sha256, task_id, created_at FROM sr_config_revisions WHERE node_id = ? AND revision = ?",
        )
        .bind(node_id)
        .bind(revision)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("config revision not found"))
    }

    pub async fn diff_config_revisions(
        &self,
        managed_cluster_id: i64,
        organization_id: Option<i64>,
        node_id: i64,
        from_revision: i64,
        to_revision: i64,
    ) -> ApiResult<Vec<SrConfigDiffLine>> {
        let from = self
            .get_config_revision(managed_cluster_id, organization_id, node_id, from_revision)
            .await?;
        let to = self
            .get_config_revision(managed_cluster_id, organization_id, node_id, to_revision)
            .await?;
        Ok(diff_lines(&from.content, &to.content))
    }

    async fn ensure_node_scope(
        &self,
        managed_cluster_id: i64,
        organization_id: Option<i64>,
        node_id: i64,
    ) -> ApiResult<()> {
        let scoped: Option<i64> = sqlx::query_scalar(
            "SELECT n.id FROM sr_cluster_nodes n JOIN sr_managed_clusters m ON m.id = n.managed_cluster_id WHERE n.id = ? AND n.managed_cluster_id = ? AND (? IS NULL OR m.organization_id = ?)",
        )
        .bind(node_id)
        .bind(managed_cluster_id)
        .bind(organization_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?;
        if scoped.is_none() {
            return Err(ApiError::not_found("node not found"));
        }
        Ok(())
    }

    async fn run_adoption(&self, task_id: i64) -> ApiResult<()> {
        let task: (i64, i64, String, i64) = sqlx::query_as(
            "SELECT organization_id, managed_cluster_id, payload_json, created_by FROM sr_operation_tasks WHERE id = ?",
        )
        .bind(task_id)
        .fetch_one(&self.pool)
        .await?;
        let payload: AdoptionPayload = serde_json::from_str(&task.2)?;
        let (credential, password) = self
            .credential_service
            .database_secret(payload.operator_credential_id, task.0)
            .await?;

        self.record(
            task_id,
            "validate_connection",
            "running",
            "Validating existing StarRocks connection",
            None,
        )
        .await?;
        let version_rows = query_starrocks(
            &payload.fe_host,
            payload.fe_query_port,
            &credential.username,
            Some(&password),
            "SELECT VERSION()",
        )
        .await?;
        let version = version_rows
            .first()
            .and_then(|row| row.first())
            .filter(|value| !value.is_empty())
            .cloned()
            .ok_or_else(|| {
                ApiError::cluster_connection_failed("StarRocks version query returned no value")
            })?;
        self.record(
            task_id,
            "validate_connection",
            "succeeded",
            "Existing StarRocks connection validated",
            None,
        )
        .await?;

        self.record(task_id, "discover_nodes", "running", "Reading observed FE and BE nodes", None)
            .await?;
        let frontends = query_starrocks(
            &payload.fe_host,
            payload.fe_query_port,
            &credential.username,
            Some(&password),
            "SHOW PROC '/frontends'",
        )
        .await?;
        let backends = query_starrocks(
            &payload.fe_host,
            payload.fe_query_port,
            &credential.username,
            Some(&password),
            "SHOW PROC '/backends'",
        )
        .await?;
        let mut transaction = self.pool.begin().await?;
        insert_observed_nodes(&mut transaction, task.1, "fe", &frontends).await?;
        insert_observed_nodes(&mut transaction, task.1, "be", &backends).await?;
        transaction.commit().await?;
        self.record(
            task_id,
            "discover_nodes",
            "succeeded",
            "Observed FE and BE nodes imported as read-only metadata",
            None,
        )
        .await?;

        let cluster = self
            .cluster_service
            .create_cluster_with_activation(
                CreateClusterRequest {
                    name: payload.name.clone(),
                    description: Some(format!("Read-only adoption task #{task_id}")),
                    fe_host: payload.fe_host.clone(),
                    fe_http_port: payload.fe_http_port as i32,
                    fe_query_port: payload.fe_query_port as i32,
                    username: credential.username,
                    password,
                    enable_ssl: false,
                    connection_timeout: 10,
                    tags: Some(vec!["physical-adopted-read-only".to_string()]),
                    catalog: "default_catalog".to_string(),
                    organization_id: None,
                    deployment_mode: DeploymentMode::SharedNothing,
                    cluster_type: ClusterType::StarRocks,
                    admin_user: None,
                    admin_password: None,
                },
                task.3,
                Some(task.0),
                false,
                false,
            )
            .await?;

        sqlx::query("UPDATE sr_managed_clusters SET sr_version = ?, cluster_id = ?, status = 'adopted_read_only', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(&version)
            .bind(cluster.id)
            .bind(task.1)
            .execute(&self.pool)
            .await?;
        sqlx::query("UPDATE sr_operation_tasks SET status = 'succeeded', current_step = 'verify', result_json = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(serde_json::json!({"cluster_id": cluster.id, "fe_host": payload.fe_host, "query_port": payload.fe_query_port, "version": version}).to_string())
            .bind(task_id)
            .execute(&self.pool)
            .await?;
        self.record(
            task_id,
            "verify",
            "succeeded",
            "Read-only adoption completed; lifecycle operations remain disabled",
            None,
        )
        .await?;
        Ok(())
    }

    async fn precheck(
        &self,
        task_id: i64,
        runtime: &RuntimeCluster,
        username: &str,
        private_key: &str,
        task_dir: &Path,
    ) -> ApiResult<HashMap<i64, SshContext>> {
        let mut contexts = HashMap::new();
        let fe_hosts: HashSet<i64> = runtime.frontends.iter().map(|node| node.host_id).collect();
        let install_parent = Path::new(&runtime.install_dir)
            .parent()
            .and_then(Path::to_str)
            .ok_or_else(|| ApiError::internal_error("deployment install path has no parent"))?;
        for host in &runtime.hosts {
            let context = SshContext::create(
                task_dir,
                SshTarget {
                    id: host.id,
                    target: host.ssh_target.clone(),
                    port: host.ssh_port,
                    host_key: host.host_key.clone(),
                },
                username.to_owned(),
                private_key,
            )
            .await?;
            self.executor.run(&context, "printf stellar-ssh-ok").await?;
            self.executor
                .run(
                    &context,
                    &format!(
                        "if test -d {install}; then test -w {install}; else test -d {parent} && test -w {parent}; fi",
                        install = shell_quote(&runtime.install_dir),
                        parent = shell_quote(install_parent),
                    ),
                )
                .await?;
            let disk = self
                .executor
                .run(&context, &format!("df -Pk {} | tail -n 1", shell_quote(install_parent)))
                .await?;
            let free_kb = disk
                .stdout
                .split_whitespace()
                .nth(3)
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0);
            if free_kb < MIN_FREE_KB {
                return Err(ApiError::validation_error(format!(
                    "{} has less than 20 GiB free space",
                    host.hostname
                )));
            }
            if fe_hosts.contains(&host.id) {
                self.executor
                    .run(&context, "java -version >/dev/null 2>&1")
                    .await?;
            }
            self.executor
                .run(&context, "test \"$(uname -m)\" = x86_64")
                .await?;
            let ports = runtime
                .frontends
                .iter()
                .chain(&runtime.backends)
                .filter(|node| node.host_id == host.id)
                .flat_map(|node| {
                    [
                        Some(node.service_port),
                        node.http_port,
                        node.query_port,
                        node.rpc_port,
                        node.brpc_port,
                        node.webserver_port,
                        node.starlet_port,
                    ]
                })
                .collect::<HashSet<_>>();
            for port in ports.into_iter().flatten() {
                self.executor
                    .run(
                        &context,
                        &format!(
                            "command -v ss >/dev/null && ! ss -ltnH | awk '{{print $4}}' | grep -Eq '(:|\\]){port}$'",
                        ),
                    )
                    .await?;
            }
            let advertise_hosts = runtime
                .frontends
                .iter()
                .chain(&runtime.backends)
                .filter(|node| node.host_id == host.id)
                .map(|node| node.advertise_host.as_str())
                .collect::<HashSet<_>>();
            if advertise_hosts.is_empty() {
                return Err(ApiError::internal_error("planned host has no advertised address"));
            }
            let interfaces = self
                .executor
                .run(&context, "ip -o -4 addr show | awk '{print $4}'")
                .await?
                .stdout;
            let interface_addresses = interfaces
                .lines()
                .filter_map(|line| line.split('/').next())
                .collect::<HashSet<_>>();
            if !advertise_hosts.is_subset(&interface_addresses) {
                return Err(ApiError::validation_error(format!(
                    "{} does not own every configured advertised address",
                    host.hostname
                )));
            }
            self.record(
                task_id,
                "precheck",
                "succeeded",
                &format!("{} passed precheck", host.hostname),
                None,
            )
            .await?;
            contexts.insert(host.id, context);
        }
        Ok(contexts)
    }

    async fn reserve_ports(&self, cluster_id: i64, runtime: &RuntimeCluster) -> ApiResult<()> {
        let mut tx = self.pool.begin().await?;
        for node in runtime.frontends.iter().chain(&runtime.backends) {
            let host = runtime
                .hosts
                .iter()
                .find(|host| host.id == node.host_id)
                .ok_or_else(|| {
                    ApiError::internal_error("planned node references an unavailable physical host")
                })?;
            let mut ports = HashSet::new();
            for port in [
                Some(node.service_port),
                node.http_port,
                node.query_port,
                node.rpc_port,
                node.brpc_port,
                node.webserver_port,
                node.starlet_port,
            ]
            .into_iter()
            .flatten()
            .filter(|port| ports.insert(*port))
            {
                let inserted = sqlx::query("INSERT INTO sr_host_port_allocations (host_id, port, managed_cluster_id) VALUES (?, ?, ?) ON CONFLICT(host_id, port) DO NOTHING")
                    .bind(host.id)
                    .bind(port)
                    .bind(cluster_id)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                if inserted == 0 {
                    return Err(ApiError::validation_error(format!(
                        "port {port} on host {} is reserved by another managed cluster",
                        host.hostname
                    )));
                }
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn cache_package(&self, task_id: i64, payload: &DeploymentPayload) -> ApiResult<PathBuf> {
        fs::create_dir_all(&self.cache_dir).await.map_err(|error| {
            ApiError::internal_error(format!("failed to create package cache: {error}"))
        })?;
        let path = self.cache_dir.join(format!("{}.tar.gz", payload.sha256));
        if path.exists() {
            let cached_path = path.clone();
            let cached_digest = tokio::task::spawn_blocking(move || sha256_file(&cached_path))
                .await
                .map_err(|_| ApiError::internal_error("package checksum task failed"))??;
            if cached_digest == payload.sha256 {
                return Ok(path);
            }
        }
        // A pre-provisioned control-plane file avoids the controlled download
        // when it hashes to the registered digest.
        if let Some(source) = payload.local_path.as_deref() {
            let source_path = source.to_owned();
            let preset_digest =
                tokio::task::spawn_blocking(move || sha256_file(Path::new(&source_path)))
                    .await
                    .map_err(|_| {
                        ApiError::internal_error("pre-provisioned package checksum task failed")
                    })??;
            if preset_digest != payload.sha256 {
                return Err(ApiError::validation_error(
                    "pre-provisioned package SHA-256 does not match registered digest",
                ));
            }
            if tokio::fs::hard_link(&source, &path).await.is_err() {
                tokio::fs::copy(&source, &path).await.map_err(|error| {
                    ApiError::internal_error(format!(
                        "failed to import pre-provisioned package: {error}"
                    ))
                })?;
            }
            let size = tokio::fs::metadata(&path)
                .await
                .map_err(|error| {
                    ApiError::internal_error(format!(
                        "failed to import pre-provisioned package: {error}"
                    ))
                })?
                .len() as i64;
            self.mark_package_cached(payload.package_id, &path, size)
                .await?;
            return Ok(path);
        }
        self.ensure_package_source(&payload.package_url)?;
        let temporary = path.with_extension(format!("download.{task_id}"));
        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(1800))
            .build()
            .map_err(|error| {
                ApiError::internal_error(format!("failed to create package downloader: {error}"))
            })?
            .get(&payload.package_url)
            .send()
            .await
            .map_err(|error| {
                ApiError::cluster_connection_failed(format!("package download failed: {error}"))
            })?;
        if !response.status().is_success() {
            return Err(ApiError::cluster_connection_failed(format!(
                "package download returned HTTP {}",
                response.status()
            )));
        }
        if response
            .content_length()
            .is_some_and(|size| size > 8 * 1024 * 1024 * 1024)
        {
            return Err(ApiError::validation_error("package exceeds 8 GiB limit"));
        }
        let mut file = fs::File::create(&temporary).await.map_err(|error| {
            ApiError::internal_error(format!("failed to create package cache: {error}"))
        })?;
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        let mut response = response;
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            ApiError::cluster_connection_failed(format!("failed to read package: {error}"))
        })? {
            size += chunk.len() as u64;
            if size > 8 * 1024 * 1024 * 1024 {
                fs::remove_file(&temporary).await.ok();
                return Err(ApiError::validation_error("package exceeds 8 GiB limit"));
            }
            digest.update(&chunk);
            file.write_all(&chunk).await.map_err(|error| {
                ApiError::internal_error(format!("failed to cache package: {error}"))
            })?;
        }
        file.flush().await.map_err(|error| {
            ApiError::internal_error(format!("failed to finalize package cache: {error}"))
        })?;
        if format!("{:x}", digest.finalize()) != payload.sha256 {
            fs::remove_file(&temporary).await.ok();
            return Err(ApiError::validation_error(
                "package SHA-256 does not match registered digest",
            ));
        }
        fs::rename(&temporary, &path).await.map_err(|error| {
            ApiError::internal_error(format!("failed to finalize package cache: {error}"))
        })?;
        self.mark_package_cached(payload.package_id, &path, size as i64)
            .await?;
        Ok(path)
    }

    async fn mark_package_cached(&self, package_id: i64, path: &Path, size: i64) -> ApiResult<()> {
        sqlx::query(
            "UPDATE sr_packages SET cached_path = ?, size_bytes = ?, status = 'cached' WHERE id = ?",
        )
        .bind(path.to_string_lossy().to_string())
        .bind(size)
        .bind(package_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn distribute_and_install(
        &self,
        task_id: i64,
        runtime: &RuntimeCluster,
        contexts: &HashMap<i64, SshContext>,
        archive: &Path,
        archive_root: &str,
        install_dirs: &HashMap<i64, String>,
    ) -> ApiResult<()> {
        let remote_dir = format!("/tmp/stellar-sr-{task_id}");
        let remote_archive = format!("{remote_dir}/package.tar.gz");
        for host in &runtime.hosts {
            let install_dir = install_dirs
                .get(&host.id)
                .cloned()
                .unwrap_or_else(|| runtime.install_dir.clone());
            let context = contexts
                .get(&host.id)
                .ok_or_else(|| ApiError::internal_error("SSH context missing"))?;
            self.executor
                .run(context, &format!("mkdir -p {}", shell_quote(&remote_dir)))
                .await?;
            self.executor
                .copy(context, archive, &remote_archive)
                .await?;
            let digest = self
                .executor
                .run(context, &format!("sha256sum {}", shell_quote(&remote_archive)))
                .await?;
            if digest.stdout.split_whitespace().next() != Some(runtime.sha256.as_str()) {
                return Err(ApiError::validation_error(format!(
                    "package digest mismatch on {}",
                    host.hostname
                )));
            }
            let version_dir = format!("{install_dir}/{archive_root}");
            // Extraction is idempotent: a partially retried scale-out finds the
            // version directory already present and only refreshes the symlink.
            self.executor
                .run(
                    context,
                    &format!(
                        "if test ! -e {}; then mkdir -p {} && tar -xzf {} -C {} --no-same-owner; fi",
                        shell_quote(&version_dir),
                        shell_quote(&version_dir),
                        shell_quote(&remote_archive),
                        shell_quote(&install_dir),
                    ),
                )
                .await?;
            self.executor
                .run(
                    context,
                    &format!(
                        "ln -sfn {} {}/current && rm -rf {}",
                        shell_quote(&version_dir),
                        shell_quote(&install_dir),
                        shell_quote(&remote_dir),
                    ),
                )
                .await?;
        }
        for frontend in &runtime.frontends {
            let context = contexts.get(&frontend.host_id).unwrap();
            self.executor
                .run(
                    context,
                    &format!("mkdir -p {}", shell_quote(frontend.meta_dir.as_deref().unwrap())),
                )
                .await?;
        }
        for backend in &runtime.backends {
            let context = contexts.get(&backend.host_id).unwrap();
            self.executor
                .run(
                    context,
                    &format!("mkdir -p {}", shell_quote(backend.storage_dir.as_deref().unwrap())),
                )
                .await?;
        }
        Ok(())
    }

    async fn write_configs(
        &self,
        task_id: i64,
        cluster_id: i64,
        created_by: i64,
        runtime: &RuntimeCluster,
        contexts: &HashMap<i64, SshContext>,
    ) -> ApiResult<()> {
        for frontend in &runtime.frontends {
            let context = contexts.get(&frontend.host_id).unwrap();
            let path = format!("{}/current/fe/conf/fe.conf", runtime.install_dir);
            let existing = self
                .executor
                .run(context, &format!("cat {}", shell_quote(&path)))
                .await?
                .stdout;
            let config = fe_config(
                &existing,
                FeConfig {
                    advertise_host: &frontend.advertise_host,
                    meta_dir: frontend.meta_dir.as_deref().unwrap(),
                    edit_log_port: frontend.service_port,
                    http_port: required_port(frontend.http_port, "FE HTTP")?,
                    query_port: required_port(frontend.query_port, "FE query")?,
                    rpc_port: required_port(frontend.rpc_port, "FE RPC")?,
                    single_be: runtime.backends.len() == 1,
                },
            );
            write_remote_file(self.executor.as_ref(), context, &path, &config).await?;
            self.record_config_revision(task_id, cluster_id, created_by, frontend.id, &config)
                .await?;
        }
        for backend in &runtime.backends {
            let context = contexts.get(&backend.host_id).unwrap();
            let path = format!("{}/current/be/conf/be.conf", runtime.install_dir);
            let existing = self
                .executor
                .run(context, &format!("cat {}", shell_quote(&path)))
                .await?
                .stdout;
            let config = be_config(
                &existing,
                &backend.advertise_host,
                backend.storage_dir.as_deref().unwrap(),
                backend.service_port,
                required_port(backend.http_port, "BE port")?,
                required_port(backend.webserver_port, "BE webserver")?,
                required_port(backend.brpc_port, "BE brpc")?,
                required_port(backend.starlet_port, "BE starlet")?,
            );
            write_remote_file(self.executor.as_ref(), context, &path, &config).await?;
            self.record_config_revision(task_id, cluster_id, created_by, backend.id, &config)
                .await?;
        }
        Ok(())
    }

    async fn record_config_revision(
        &self,
        task_id: i64,
        cluster_id: i64,
        created_by: i64,
        node_id: i64,
        content: &str,
    ) -> ApiResult<()> {
        let revision: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(revision), 0) + 1 FROM sr_config_revisions WHERE node_id = ?",
        )
        .bind(node_id)
        .fetch_one(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO sr_config_revisions (managed_cluster_id, node_id, revision, content, content_sha256, task_id, created_by) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(cluster_id)
        .bind(node_id)
        .bind(revision)
        .bind(content)
        .bind(format!("{:x}", Sha256::digest(content.as_bytes())))
        .bind(task_id)
        .bind(created_by)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_runtime(
        &self,
        managed_cluster_id: i64,
        organization_id: Option<i64>,
    ) -> ApiResult<RuntimeCluster> {
        let cluster: RuntimeClusterRow = sqlx::query_as("SELECT name, install_dir, sr_version, ssh_credential_id, operator_credential_id FROM sr_managed_clusters WHERE id = ? AND (? IS NULL OR organization_id = ?)")
            .bind(managed_cluster_id).bind(organization_id).bind(organization_id).fetch_one(&self.pool).await?;
        let package: (String,) = sqlx::query_as("SELECT sha256 FROM sr_packages WHERE id = (SELECT package_id FROM sr_managed_clusters WHERE id = ?)")
            .bind(managed_cluster_id).fetch_one(&self.pool).await?;
        let hosts: Vec<RuntimeHost> = sqlx::query_as("SELECT DISTINCT h.id, h.hostname, h.ssh_target, h.ssh_port, h.host_key FROM physical_hosts h JOIN sr_cluster_nodes n ON n.host_id = h.id WHERE n.managed_cluster_id = ? ORDER BY h.id")
            .bind(managed_cluster_id).fetch_all(&self.pool).await?;
        let nodes: Vec<SrClusterNode> = sqlx::query_as("SELECT id, managed_cluster_id, host_id, role, fe_role, advertise_host, service_port, http_port, query_port, rpc_port, brpc_port, webserver_port, starlet_port, meta_dir, storage_dir, status, created_at, updated_at FROM sr_cluster_nodes WHERE managed_cluster_id = ? ORDER BY id")
            .bind(managed_cluster_id).fetch_all(&self.pool).await?;
        let frontends = nodes
            .iter()
            .filter(|node| node.role == "fe")
            .cloned()
            .collect::<Vec<_>>();
        let backends = nodes
            .into_iter()
            .filter(|node| node.role == "be")
            .collect::<Vec<_>>();
        Ok(RuntimeCluster {
            name: cluster.name,
            install_dir: cluster.install_dir,
            sr_version: cluster.sr_version,
            ssh_credential_id: cluster.ssh_credential_id,
            operator_credential_id: cluster.operator_credential_id,
            sha256: package.0,
            hosts,
            frontends,
            backends,
        })
    }

    async fn ensure_credential_ownership(
        &self,
        ssh_id: i64,
        operator_id: i64,
        organization_id: i64,
    ) -> ApiResult<()> {
        let ssh: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM ssh_credentials WHERE id = ? AND organization_id = ?",
        )
        .bind(ssh_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?;
        let database: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM sr_database_credentials WHERE id = ? AND organization_id = ?",
        )
        .bind(operator_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?;
        if ssh.is_none() || database.is_none() {
            return Err(ApiError::validation_error(
                "credentials must belong to the deployment organization",
            ));
        }
        Ok(())
    }

    fn ensure_package_source(&self, package_url: &str) -> ApiResult<()> {
        let package_url = reqwest::Url::parse(package_url)
            .map_err(|_| ApiError::validation_error("package URL is invalid"))?;
        let host = package_url
            .host_str()
            .ok_or_else(|| ApiError::validation_error("package URL must include a host"))?;
        if !package_url.username().is_empty() || package_url.password().is_some() {
            return Err(ApiError::validation_error(
                "package URL must not contain user credentials",
            ));
        }
        if self.package_allowed_hosts.is_empty()
            || !self
                .package_allowed_hosts
                .contains(&host.to_ascii_lowercase())
        {
            return Err(ApiError::forbidden(
                "package host is not in APP_SR_PHYSICAL_PACKAGE_ALLOWED_HOSTS",
            ));
        }
        Ok(())
    }

    async fn ensure_hosts_ownership(
        &self,
        request: &CreateDeploymentRequest,
        organization_id: i64,
    ) -> ApiResult<()> {
        let ids: HashSet<i64> = request
            .frontends
            .iter()
            .map(|node| node.host_id)
            .chain(request.backends.iter().map(|node| node.host_id))
            .collect();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM physical_hosts WHERE organization_id = ? AND id IN (SELECT value FROM json_each(?))")
            .bind(organization_id).bind(serde_json::to_string(&ids)?).fetch_one(&self.pool).await?;
        if count != ids.len() as i64 {
            return Err(ApiError::validation_error(
                "every deployment host must belong to the organization",
            ));
        }
        Ok(())
    }

    async fn record(
        &self,
        task_id: i64,
        step: &str,
        status: &str,
        message: &str,
        node_id: Option<i64>,
    ) -> ApiResult<()> {
        sqlx::query("INSERT INTO sr_operation_events (task_id, step, status, node_id, message) VALUES (?, ?, ?, ?, ?)")
            .bind(task_id).bind(step).bind(status).bind(node_id).bind(message.chars().take(2048).collect::<String>()).execute(&self.pool).await?;
        sqlx::query(
            "UPDATE sr_operation_tasks SET current_step = ? WHERE id = ? AND status = 'running'",
        )
        .bind(step)
        .bind(task_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct DeploymentPayload {
    package_id: i64,
    sr_version: String,
    package_url: String,
    sha256: String,
    #[serde(default)]
    local_path: Option<String>,
    #[serde(default)]
    bootstrap_credential_id: Option<i64>,
    frontends: Vec<FrontendDeploymentNode>,
    backends: Vec<BackendDeploymentNode>,
}
impl DeploymentPayload {
    fn from_request(
        request: &CreateDeploymentRequest,
        sr_version: String,
        package_url: String,
        local_path: Option<String>,
        sha256: String,
    ) -> Self {
        Self {
            package_id: request.package_id,
            sr_version,
            package_url,
            sha256,
            local_path,
            bootstrap_credential_id: request.bootstrap_credential_id,
            frontends: request.frontends.clone(),
            backends: request.backends.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct NodeCommandPayload {
    node_id: i64,
    action: String,
}

#[derive(Serialize, Deserialize)]
struct DecommissionPayload {
    remove_remote_files: bool,
    deregister: bool,
}

#[derive(Serialize, Deserialize)]
struct ScaleOutPayload {
    package_id: i64,
    package_url: String,
    sha256: String,
    local_path: Option<String>,
    frontends: Vec<FrontendDeploymentNode>,
    backends: Vec<BackendDeploymentNode>,
}

#[derive(Serialize, Deserialize)]
struct ConfigChangePayload {
    node_id: i64,
    content: String,
    restart: bool,
}

#[derive(Serialize, Deserialize)]
struct AdoptionPayload {
    name: String,
    fe_host: String,
    fe_http_port: u16,
    fe_query_port: u16,
    operator_credential_id: i64,
}

impl AdoptionPayload {
    fn from_request(request: &AdoptClusterRequest) -> Self {
        Self {
            name: request.name.clone(),
            fe_host: request.fe_host.clone(),
            fe_http_port: request.fe_http_port,
            fe_query_port: request.fe_query_port,
            operator_credential_id: request.operator_credential_id,
        }
    }
}

#[derive(sqlx::FromRow)]
struct RuntimeClusterRow {
    name: String,
    install_dir: String,
    sr_version: String,
    ssh_credential_id: Option<i64>,
    operator_credential_id: Option<i64>,
}
#[derive(sqlx::FromRow)]
struct RuntimeHost {
    id: i64,
    hostname: String,
    ssh_target: String,
    ssh_port: i64,
    host_key: String,
}
struct RuntimeCluster {
    name: String,
    install_dir: String,
    sr_version: String,
    ssh_credential_id: Option<i64>,
    operator_credential_id: Option<i64>,
    sha256: String,
    hosts: Vec<RuntimeHost>,
    frontends: Vec<SrClusterNode>,
    backends: Vec<SrClusterNode>,
}

async fn query_starrocks(
    host: &str,
    port: u16,
    username: &str,
    password: Option<&str>,
    sql: &str,
) -> ApiResult<Vec<Vec<String>>> {
    let pool = Pool::new(
        OptsBuilder::default()
            .ip_or_hostname(host)
            .tcp_port(port)
            .user(Some(username))
            .pass(password.map(str::to_owned))
            .prefer_socket(false),
    );
    let client = MySQLClient::from_pool(pool.clone());
    let result = timeout(SQL_TIMEOUT, client.query_raw(sql))
        .await
        .map_err(|_| ApiError::cluster_connection_failed("StarRocks query timed out"))
        .and_then(|result| {
            result
                .map(|(_, rows)| rows)
                .map_err(|_| ApiError::cluster_connection_failed("StarRocks query failed"))
        });
    drop(client);
    pool.disconnect().await.map_err(|_| {
        ApiError::cluster_connection_failed("failed to close StarRocks adoption connection")
    })?;
    result
}

async fn insert_observed_nodes(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    managed_cluster_id: i64,
    node_type: &str,
    rows: &[Vec<String>],
) -> ApiResult<()> {
    for row in rows {
        let address = row
            .get(1)
            .or_else(|| row.first())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError::cluster_connection_failed("observed node has no address"))?;
        sqlx::query("INSERT INTO sr_observed_nodes (managed_cluster_id, node_type, address, raw_json) VALUES (?, ?, ?, ?)")
            .bind(managed_cluster_id)
            .bind(node_type)
            .bind(address)
            .bind(serde_json::to_string(row)?)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

async fn write_remote_file(
    executor: &dyn SshExecutor,
    context: &SshContext,
    path: &str,
    content: &str,
) -> ApiResult<()> {
    let encoded = STANDARD.encode(content.as_bytes());
    let temporary = format!("{path}.stellar-tmp");
    executor
        .run(
            context,
            &format!(
                "printf '%s' {} | base64 -d > {} && mv {} {}",
                shell_quote(&encoded),
                shell_quote(&temporary),
                shell_quote(&temporary),
                shell_quote(path)
            ),
        )
        .await?;
    Ok(())
}

async fn wait_for_sql(host: &str, port: i64, sql: &str, auth: &SqlAuth) -> ApiResult<()> {
    let mut last_error = None;
    for _ in 0..READY_RETRIES {
        match execute_sql(host, port, &[sql.to_string()], auth).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        sleep(READY_INTERVAL).await;
    }
    Err(last_error
        .unwrap_or_else(|| ApiError::cluster_connection_failed("FE did not become ready")))
}

async fn wait_for_node(
    leader_host: &str,
    query_port: i64,
    statement: &str,
    node_host: &str,
    service_port: i64,
    auth: &SqlAuth,
) -> ApiResult<()> {
    for _ in 0..READY_RETRIES {
        if let Ok(rows) = query_starrocks(
            leader_host,
            query_port as u16,
            &auth.user,
            auth.password.as_deref(),
            statement,
        )
        .await
            && rows
                .iter()
                .any(|row| node_matches(row, node_host, service_port, true))
        {
            return Ok(());
        }
        sleep(READY_INTERVAL).await;
    }
    Err(ApiError::cluster_connection_failed(
        "new StarRocks node did not become alive before timeout",
    ))
}

async fn node_exists(
    leader_host: &str,
    query_port: i64,
    statement: &str,
    node_host: &str,
    service_port: i64,
    auth: &SqlAuth,
) -> ApiResult<bool> {
    let rows = query_starrocks(
        leader_host,
        query_port as u16,
        &auth.user,
        auth.password.as_deref(),
        statement,
    )
    .await?;
    Ok(rows
        .iter()
        .any(|row| node_matches(row, node_host, service_port, false)))
}

fn node_matches(row: &[String], node_host: &str, service_port: i64, require_alive: bool) -> bool {
    row.iter().any(|value| value == node_host)
        && row.iter().any(|value| value == &service_port.to_string())
        && (!require_alive || row.iter().any(|value| value.eq_ignore_ascii_case("true")))
}

/// Maps a node row to its start/stop script suffix and the port that must
/// listen once the process is serving. BE rows store be_port in the http_port
/// column (service_port holds the heartbeat port).
fn incremental_hosts(host_ids: &HashSet<i64>, cluster_install_dir: &str) -> HashMap<i64, String> {
    host_ids
        .iter()
        .map(|host_id| {
            (*host_id, format!("{}-host{host_id}", cluster_install_dir.trim_end_matches('/')))
        })
        .collect()
}

fn node_service_endpoint(node: &SrClusterNode) -> ApiResult<(String, i64)> {
    match node.role.as_str() {
        "fe" => Ok(("fe".to_string(), required_port(node.query_port, "FE query")?)),
        _ => Ok(("be".to_string(), required_port(node.http_port, "BE port")?)),
    }
}

/// Rejects a configuration update that alters topology facts (paths, ports,
/// addresses). Tunables outside the managed key set are allowed to change.
fn validate_config_topology(existing: &str, updated: &str, role: &str) -> ApiResult<()> {
    let managed_keys: Vec<&str> = match role {
        "fe" => FE_MANAGED_KEYS.to_vec(),
        _ => BE_MANAGED_KEYS.to_vec(),
    };
    let current = extract_config_values(existing);
    let proposed = extract_config_values(updated);
    for key in managed_keys {
        let old_value = current
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.clone());
        let new_value = proposed
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.clone());
        if let (Some(old_value), Some(new_value)) = (&old_value, &new_value) {
            if old_value != new_value {
                return Err(ApiError::validation_error(format!(
                    "managed key {key} must stay {old_value}"
                )));
            }
        } else if old_value.is_some() && new_value.is_none() {
            return Err(ApiError::validation_error(format!(
                "managed key {key} must not be removed"
            )));
        }
    }
    Ok(())
}

/// Minimal LCS line diff used to render configuration revision changes.
fn diff_lines(old_content: &str, new_content: &str) -> Vec<SrConfigDiffLine> {
    let old = old_content.lines().collect::<Vec<_>>();
    let new = new_content.lines().collect::<Vec<_>>();
    let mut lcs = vec![vec![0_usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut lines = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            lines.push(SrConfigDiffLine { kind: "context".to_string(), text: old[i].to_owned() });
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            lines.push(SrConfigDiffLine { kind: "removed".to_string(), text: old[i].to_owned() });
            i += 1;
        } else {
            lines.push(SrConfigDiffLine { kind: "added".to_string(), text: new[j].to_owned() });
            j += 1;
        }
    }
    lines.extend(
        old[i..]
            .iter()
            .map(|line| SrConfigDiffLine { kind: "removed".to_string(), text: (*line).to_owned() }),
    );
    lines.extend(
        new[j..]
            .iter()
            .map(|line| SrConfigDiffLine { kind: "added".to_string(), text: (*line).to_owned() }),
    );
    lines
}

async fn execute_sql(
    host: &str,
    port: i64,
    statements: &[String],
    auth: &SqlAuth,
) -> ApiResult<()> {
    let pool = Pool::new(
        OptsBuilder::default()
            .ip_or_hostname(host)
            .tcp_port(port as u16)
            .user(Some(auth.user.as_str()))
            .pass(auth.password.clone())
            .prefer_socket(false),
    );
    let mut connection = timeout(SQL_TIMEOUT, pool.get_conn())
        .await
        .map_err(|_| ApiError::cluster_connection_failed("FE connection timed out"))?
        .map_err(|error| {
            ApiError::cluster_connection_failed(format!("FE connection failed: {error}"))
        })?;
    let mut result: ApiResult<()> = Ok(());
    {
        let mut connection = timeout(SQL_TIMEOUT, pool.get_conn())
            .await
            .map_err(|_| ApiError::cluster_connection_failed("FE connection timed out"))?
            .map_err(|error| {
                ApiError::cluster_connection_failed(format!("FE connection failed: {error}"))
            })?;
        for statement in statements {
            let query: std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = std::result::Result<Vec<mysql_async::Row>, mysql_async::Error>,
                        > + Send
                        + '_,
                >,
            > = connection.query(statement);
            result = timeout(SQL_TIMEOUT, query)
                .await
                .map_err(|_| ApiError::cluster_connection_failed("FE SQL command timed out"))
                .and_then(|inner| {
                    inner
                        .map(|_| ())
                        .map_err(|_| ApiError::cluster_connection_failed("FE SQL command failed"))
                });
            if result.is_err() {
                tracing::warn!(
                    host,
                    port,
                    statements = statements.len(),
                    "FE SQL statement failed"
                );
                break;
            }
        }
        drop(connection);
    }
    pool.disconnect().await.map_err(|error| {
        ApiError::cluster_connection_failed(format!("failed to close FE connection: {error}"))
    })?;
    result
}

/// MySQL credentials for control-plane SQL statements. Fresh clusters use an
/// empty-password root account until secure_root runs.
struct SqlAuth {
    user: String,
    password: Option<String>,
}

fn root_auth() -> SqlAuth {
    SqlAuth { user: "root".to_string(), password: None }
}

fn sha256_file(path: &Path) -> ApiResult<String> {
    use std::io::Read;
    let mut file = File::open(path)
        .map_err(|error| ApiError::internal_error(format!("failed to open package: {error}")))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            ApiError::internal_error(format!("failed to read package: {error}"))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_archive(path: &Path) -> ApiResult<String> {
    let file = File::open(path)
        .map_err(|error| ApiError::internal_error(format!("failed to open archive: {error}")))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut root = None;
    let mut unpacked_size = 0_u64;
    for entry in archive
        .entries()
        .map_err(|error| ApiError::validation_error(format!("invalid package archive: {error}")))?
    {
        let entry = entry.map_err(|error| {
            ApiError::validation_error(format!("invalid package archive entry: {error}"))
        })?;
        unpacked_size = unpacked_size.saturating_add(entry.header().size().unwrap_or(u64::MAX));
        if unpacked_size > 16 * 1024 * 1024 * 1024 {
            return Err(ApiError::validation_error("package archive expands beyond 16 GiB limit"));
        }
        let path = entry.path().map_err(|error| {
            ApiError::validation_error(format!("invalid archive path: {error}"))
        })?;
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(ApiError::validation_error("package archive contains an unsafe path"));
        }
        if let Some(component) = path.components().next()
            && let Component::Normal(value) = component
        {
            let value = value.to_string_lossy().to_string();
            if value == "."
                || value == ".."
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            {
                return Err(ApiError::validation_error(
                    "package archive root directory is invalid",
                ));
            }
            if let Some(root) = &root {
                if root != &value {
                    return Err(ApiError::validation_error(
                        "package archive must have exactly one root directory",
                    ));
                }
            } else {
                root = Some(value);
            }
        }
        if let Some(link) = entry
            .link_name()
            .map_err(|error| ApiError::validation_error(format!("invalid archive link: {error}")))?
        {
            if link.is_absolute()
                || link
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(ApiError::validation_error("package archive contains an unsafe link"));
            }
        }
    }
    root.ok_or_else(|| ApiError::validation_error("package archive has no root directory"))
}

fn sql_identifier(value: &str) -> ApiResult<&str> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(ApiError::validation_error(
            "operator username contains unsupported characters",
        ));
    }
    Ok(value)
}

fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

fn required_port(port: Option<i64>, name: &str) -> ApiResult<i64> {
    port.filter(|port| (1..=65_535).contains(port))
        .ok_or_else(|| ApiError::internal_error(format!("{name} port is missing or invalid")))
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::{diff_lines, node_matches, sql_string_literal, validate_config_topology};

    #[test]
    fn rejects_topology_changes_but_allows_tunables() {
        let existing = "mem_limit = 80%\nbe_port = 9060\nstarlet_port = 9070\n";

        let tuned = "mem_limit = 90%\nbe_port = 9060\nstarlet_port = 9070\n";
        assert!(validate_config_topology(existing, tuned, "be").is_ok());

        let port_changed = "mem_limit = 80%\nbe_port = 19060\nstarlet_port = 9070\n";
        assert!(validate_config_topology(existing, port_changed, "be").is_err());

        let key_removed = "mem_limit = 80%\nbe_port = 9060\n";
        assert!(validate_config_topology(existing, key_removed, "be").is_err());
    }

    #[test]
    fn escapes_operator_password_sql_literals() {
        assert_eq!(sql_string_literal("pa'ss\\word"), "'pa''ss\\\\word'");
    }

    #[test]
    fn identifies_an_alive_observed_node() {
        let row = vec![
            "fe-host".to_string(),
            "10.0.0.1".to_string(),
            "9010".to_string(),
            "true".to_string(),
        ];

        assert!(node_matches(&row, "10.0.0.1", 9010, true));
        assert!(!node_matches(&row, "10.0.0.1", 9011, false));
    }

    #[test]
    fn diffs_config_revisions_with_context_and_changes() {
        let lines = diff_lines(
            "http_port = 8030\nquery_port = 9030\n",
            "http_port = 18030\nquery_port = 9030\nmem_limit = 90%\n",
        );
        let rendered = lines
            .iter()
            .map(|line| (line.kind.as_str(), line.text.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec![
                ("removed", "http_port = 8030"),
                ("added", "http_port = 18030"),
                ("context", "query_port = 9030"),
                ("added", "mem_limit = 90%"),
            ]
        );
    }

    /// 来源区分：自托管（deploy 生命周期）与外部导入（无关联/只读接管）。
    #[tokio::test]
    async fn cluster_origin_classification_follows_managed_cluster_link() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        let organization_id: i64 =
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let user_id: i64 = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();

        let service = super::SrDeploymentService::new(
            pool.clone(),
            std::sync::Arc::new(crate::services::ClusterService::new(
                pool.clone(),
                std::sync::Arc::new(crate::services::MySQLPoolManager::new()),
            )),
            std::sync::Arc::new(crate::services::CredentialService::new(
                pool.clone(),
                "0123456789abcdef0123456789abcdef",
            )),
            std::env::temp_dir().join("stellar-sr-deploy-test-origin"),
            vec![],
            vec![],
        );

        // 托管集群：deploy 生命周期状态 → managed
        let managed_cluster_id = sqlx::query("INSERT INTO sr_managed_clusters (organization_id, name, sr_version, status, created_by) VALUES (?, 'origin-managed', '3.3.0', 'planning', ?)")
            .bind(organization_id)
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
        sqlx::query("INSERT INTO clusters (name, fe_host, fe_http_port, fe_query_port, username, password_encrypted, catalog, is_active, created_at, updated_at) VALUES ('origin-managed', '10.0.0.1', 8030, 9030, 'root', '', 'default_catalog', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)")
            .execute(&pool)
            .await
            .unwrap();
        let cluster_id: i64 =
            sqlx::query_scalar("SELECT id FROM clusters WHERE name = 'origin-managed'")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query("UPDATE sr_managed_clusters SET cluster_id = ?, status = 'running' WHERE id = ?")
            .bind(cluster_id)
            .bind(managed_cluster_id)
            .execute(&pool)
            .await
            .unwrap();

        // 只读接管集群：已关联但不属于 deploy 生命周期 → 非自托管
        let adopted_id = sqlx::query("INSERT INTO sr_managed_clusters (organization_id, name, sr_version, status, created_by) VALUES (?, 'origin-adopted', 'unknown', 'adopted_read_only', ?)")
            .bind(organization_id)
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
        sqlx::query("INSERT INTO clusters (name, fe_host, fe_http_port, fe_query_port, username, password_encrypted, catalog, is_active, created_at, updated_at) VALUES ('origin-adopted', '10.0.0.2', 8030, 9030, 'root', '', 'default_catalog', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)")
            .execute(&pool)
            .await
            .unwrap();
        let adopted_cluster_id: i64 =
            sqlx::query_scalar("SELECT id FROM clusters WHERE name = 'origin-adopted'")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query("UPDATE sr_managed_clusters SET cluster_id = ? WHERE id = ?")
            .bind(adopted_cluster_id)
            .bind(adopted_id)
            .execute(&pool)
            .await
            .unwrap();

        let ids = vec![cluster_id, adopted_cluster_id];
        let managed = service.self_managed_cluster_ids(&ids).await.unwrap();
        assert!(managed.contains(&cluster_id), "deploy lifecycle must be self-managed");
        assert!(!managed.contains(&adopted_cluster_id), "adopted_read_only must not be self-managed");

        let adopted_status = service.managed_cluster_status(adopted_cluster_id).await.unwrap();
        assert_eq!(adopted_status.as_deref(), Some("adopted_read_only"));

        let external_status = service.managed_cluster_status(0).await.unwrap();
        assert!(external_status.is_none(), "unlinked cluster must be external import");
    }

    #[tokio::test]
    async fn migration_accepts_read_only_adoption_without_infrastructure_bindings() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        let organization_id: i64 =
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let user_id: i64 = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();

        let managed_cluster_id = sqlx::query("INSERT INTO sr_managed_clusters (organization_id, name, sr_version, status, created_by) VALUES (?, 'adopted-test', 'unknown', 'planning', ?)")
            .bind(organization_id)
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
        sqlx::query("INSERT INTO sr_operation_tasks (organization_id, managed_cluster_id, task_type, payload_json, created_by) VALUES (?, ?, 'adopt_read_only', '{}', ?)")
            .bind(organization_id)
            .bind(managed_cluster_id)
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE sr_managed_clusters SET status = 'adopted_read_only' WHERE id = ?")
            .bind(managed_cluster_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sr_observed_nodes (managed_cluster_id, node_type, address, raw_json) VALUES (?, 'fe', '10.0.0.1', '[]')")
            .bind(managed_cluster_id)
            .execute(&pool)
            .await
            .unwrap();

        let observed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sr_observed_nodes WHERE managed_cluster_id = ?",
        )
        .bind(managed_cluster_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(observed, 1);
    }
}
