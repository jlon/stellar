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
        CreateDeploymentRequest, DeploymentMode, FrontendDeploymentNode, SrClusterNode,
        SrManagedCluster, SrManagedClusterDetail, SrOperationTask, SrOperationTaskDetail,
    },
    services::{ClusterService, MySQLClient},
    utils::{ApiError, ApiResult},
};

use super::{
    CredentialService, FeConfig, OpenSshExecutor, SshContext, SshExecutor, SshTarget, be_config,
    fe_config, shell_quote,
};

// A fresh FE may take several minutes to initialize BDB and elect itself leader.
const READY_RETRIES: usize = 200;
const READY_INTERVAL: Duration = Duration::from_secs(3);
const MIN_FREE_KB: i64 = 20 * 1024 * 1024;
const SQL_TIMEOUT: Duration = Duration::from_secs(30);
const TASK_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

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
        }
    }

    pub async fn submit(
        self: &Arc<Self>,
        request: CreateDeploymentRequest,
        organization_id: i64,
        user_id: i64,
    ) -> ApiResult<SrOperationTask> {
        let request = request.normalize()?;
        let package: (String, String, String) = sqlx::query_as(
            "SELECT version, package_url, sha256 FROM sr_packages WHERE id = ? AND organization_id = ?",
        )
        .bind(request.package_id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::validation_error("package does not belong to the organization"))?;
        self.ensure_package_source(&package.1)?;
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

        let payload =
            DeploymentPayload::from_request(&request, package.0.clone(), package.1, package.2);
        let payload_json = serde_json::to_string(&payload)?;
        let mut tx = self.pool.begin().await?;
        let cluster_id = sqlx::query(
            "INSERT INTO sr_managed_clusters (organization_id, name, sr_version, install_dir, ssh_credential_id, package_id, operator_credential_id, created_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(organization_id)
        .bind(&request.name)
        .bind(&payload.sr_version)
        .bind(&request.install_dir)
        .bind(request.ssh_credential_id)
        .bind(request.package_id)
        .bind(request.operator_credential_id)
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
                "INSERT INTO sr_cluster_nodes (managed_cluster_id, host_id, role, advertise_host, service_port, http_port, brpc_port, webserver_port, storage_dir) VALUES (?, ?, 'be', ?, ?, ?, ?, ?, ?)",
            )
            .bind(cluster_id)
            .bind(backend.host_id)
            .bind(&backend.advertise_host)
            .bind(i64::from(backend.heartbeat_port))
            .bind(i64::from(backend.be_port))
            .bind(i64::from(backend.brpc_port))
            .bind(i64::from(backend.webserver_port))
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

    pub async fn list_tasks(
        &self,
        organization_id: Option<i64>,
    ) -> ApiResult<Vec<SrOperationTask>> {
        match organization_id {
            Some(organization_id) => sqlx::query_as(
                "SELECT id, organization_id, managed_cluster_id, task_type, status, current_step, error_message, result_json, created_by, created_at, started_at, finished_at FROM sr_operation_tasks WHERE organization_id = ? ORDER BY id DESC",
            )
            .bind(organization_id)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into),
            None => sqlx::query_as(
                "SELECT id, organization_id, managed_cluster_id, task_type, status, current_step, error_message, result_json, created_by, created_at, started_at, finished_at FROM sr_operation_tasks ORDER BY id DESC",
            )
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
        if query.execute(&self.pool).await?.rows_affected() == 0 {
            return Err(ApiError::validation_error(
                "only a pending task in the current organization can be cancelled",
            ));
        }
        Ok(())
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
        let nodes = sqlx::query_as("SELECT id, managed_cluster_id, host_id, role, fe_role, advertise_host, service_port, http_port, query_port, rpc_port, brpc_port, webserver_port, meta_dir, storage_dir, status, created_at, updated_at FROM sr_cluster_nodes WHERE managed_cluster_id = ? ORDER BY id")
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

    async fn run_task(&self, task_id: i64) -> ApiResult<()> {
        if sqlx::query("UPDATE sr_operation_tasks SET status = 'running', current_step = 'validate', started_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'pending'")
            .bind(task_id).execute(&self.pool).await?.rows_affected() == 0 {
            return Ok(());
        }
        sqlx::query("UPDATE sr_managed_clusters SET status = 'deploying', updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT managed_cluster_id FROM sr_operation_tasks WHERE id = ?)")
            .bind(task_id)
            .execute(&self.pool)
            .await?;

        let task_type: String =
            sqlx::query_scalar("SELECT task_type FROM sr_operation_tasks WHERE id = ?")
                .bind(task_id)
                .fetch_one(&self.pool)
                .await?;
        let result = tokio::time::timeout(TASK_TIMEOUT, async {
            match task_type.as_str() {
                "deploy" => self.run_deploy(task_id).await,
                "adopt_read_only" => self.run_adoption(task_id).await,
                _ => Err(ApiError::internal_error("unsupported physical operation task type")),
            }
        })
        .await
        .unwrap_or_else(|_| Err(ApiError::cluster_connection_failed("physical task timed out")));
        fs::remove_dir_all(self.work_dir.join(task_id.to_string()))
            .await
            .ok();
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                self.record(task_id, "failed", "failed", &error.to_string(), None)
                    .await
                    .ok();
                sqlx::query("UPDATE sr_operation_tasks SET status = 'failed', error_message = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                    .bind(error.to_string()).bind(task_id).execute(&self.pool).await?;
                sqlx::query("UPDATE sr_managed_clusters SET status = 'failed', updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT managed_cluster_id FROM sr_operation_tasks WHERE id = ?)")
                    .bind(task_id).execute(&self.pool).await?;
                Err(error)
            },
        }
    }

    async fn run_deploy(&self, task_id: i64) -> ApiResult<()> {
        let task: (i64, i64, String, i64) = sqlx::query_as("SELECT organization_id, managed_cluster_id, payload_json, created_by FROM sr_operation_tasks WHERE id = ?")
            .bind(task_id).fetch_one(&self.pool).await?;
        let payload: DeploymentPayload = serde_json::from_str(&task.2)?;
        let runtime = self.load_runtime(task.1, task.0).await?;
        let (ssh_credential, private_key) = self
            .credential_service
            .ssh_secret(runtime.ssh_credential_id, task.0)
            .await?;
        let (operator_credential, operator_password) = self
            .credential_service
            .database_secret(runtime.operator_credential_id, task.0)
            .await?;
        let task_dir = self.work_dir.join(task_id.to_string());
        fs::create_dir_all(&task_dir).await.map_err(|error| {
            ApiError::internal_error(format!("failed to create task work directory: {error}"))
        })?;

        let execution = async {
            self.record(task_id, "precheck", "running", "Checking SSH, disk, Java, and ports", None).await?;
            let contexts = self.precheck(task_id, &runtime, &ssh_credential.username, &private_key, &task_dir).await?;
            self.record(task_id, "precheck", "succeeded", "Host precheck passed", None).await?;

            self.reserve_hosts(task.1, &runtime.hosts).await?;
            self.record(task_id, "reserve", "succeeded", "Physical hosts reserved for this managed cluster", None).await?;

            let archive = self.cache_package(task_id, &payload).await?;
            let archive_root = validate_archive(&archive)?;
            self.record(task_id, "cache_package", "succeeded", "Installation package downloaded and SHA-256 verified", None).await?;

            self.distribute_and_install(task_id, &runtime, &contexts, &archive, &archive_root).await?;
            self.record(task_id, "install", "succeeded", "Package installed on all planned hosts", None).await?;

            self.write_configs(&runtime, &contexts).await?;
            self.record(task_id, "render_config", "succeeded", "Managed FE and BE configuration written atomically", None).await?;

            let leader = runtime.frontends.first().ok_or_else(|| ApiError::internal_error("deployment has no leader FE"))?;
            let leader_context = contexts.get(&leader.host_id).ok_or_else(|| ApiError::internal_error("leader SSH context missing"))?;
            let leader_http_port = required_port(leader.http_port, "leader HTTP")?;
            let leader_query_port = required_port(leader.query_port, "leader query")?;
            self.executor.run(leader_context, &format!("sh {}/current/fe/bin/start_fe.sh --daemon", shell_quote(&runtime.install_dir))).await?;
            wait_for_sql(&leader.advertise_host, leader_query_port, "SELECT 1").await?;
            self.record(task_id, "start_leader_fe", "succeeded", "Leader FE accepts MySQL connections", Some(leader.id)).await?;

            for follower in runtime.frontends.iter().skip(1) {
                if !node_exists(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/frontends'",
                    &follower.advertise_host,
                    follower.service_port,
                )
                .await?
                {
                    execute_sql(&leader.advertise_host, leader_query_port, &format!("ALTER SYSTEM ADD FOLLOWER \"{}:{}\"", follower.advertise_host, follower.service_port)).await?;
                }
                let context = contexts.get(&follower.host_id).ok_or_else(|| ApiError::internal_error("follower SSH context missing"))?;
                self.executor.run(context, &format!("sh {}/current/fe/bin/start_fe.sh --helper {}:{} --daemon", shell_quote(&runtime.install_dir), follower.advertise_host, leader.service_port)).await?;
                wait_for_node(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/frontends'",
                    &follower.advertise_host,
                    follower.service_port,
                )
                .await?;
            }
            self.record(task_id, "add_followers", "succeeded", "Follower FEs joined the cluster", None).await?;

            for backend in &runtime.backends {
                let context = contexts.get(&backend.host_id).ok_or_else(|| ApiError::internal_error("BE SSH context missing"))?;
                self.executor.run(context, &format!("sh {}/current/be/bin/start_be.sh --daemon", shell_quote(&runtime.install_dir))).await?;
                if !node_exists(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/backends'",
                    &backend.advertise_host,
                    backend.service_port,
                )
                .await?
                {
                    execute_sql(&leader.advertise_host, leader_query_port, &format!("ALTER SYSTEM ADD BACKEND \"{}:{}\"", backend.advertise_host, backend.service_port)).await?;
                }
                wait_for_node(
                    &leader.advertise_host,
                    leader_query_port,
                    "SHOW PROC '/backends'",
                    &backend.advertise_host,
                    backend.service_port,
                )
                .await?;
            }
            self.record(task_id, "start_and_add_be", "succeeded", "Backend nodes joined the cluster", None).await?;

            let operator = sql_identifier(&operator_credential.username)?;
            execute_sql(&leader.advertise_host, leader_query_port, &format!(
                "CREATE USER '{}'@'%' IDENTIFIED BY {}",
                operator,
                sql_string_literal(&operator_password)
            )).await?;
            execute_sql(&leader.advertise_host, leader_query_port, &format!(
                "GRANT OPERATE ON SYSTEM TO USER '{}'@'%'",
                operator
            )).await?;
            execute_sql(&leader.advertise_host, leader_query_port, &format!(
                "GRANT SELECT ON ALL TABLES IN DATABASE information_schema TO USER '{}'@'%'",
                operator
            )).await?;
            self.record(task_id, "create_operator", "succeeded", "Operator account created", None).await?;

            let cluster = self.cluster_service.create_cluster_with_activation(
                CreateClusterRequest {
                    name: runtime.name.clone(), description: Some(format!("Physical deployment task #{task_id}")),
                    fe_host: leader.advertise_host.clone(), fe_http_port: leader_http_port as i32,
                    fe_query_port: leader_query_port as i32, username: operator_credential.username.clone(),
                    password: operator_password, enable_ssl: false, connection_timeout: 10,
                    tags: Some(vec!["physical-deploy".to_string()]), catalog: "default_catalog".to_string(),
                    organization_id: None, deployment_mode: DeploymentMode::SharedNothing,
                    cluster_type: ClusterType::StarRocks, admin_user: None, admin_password: None,
                },
                task.3, Some(task.0), false, false,
            ).await?;
            self.record(task_id, "register_cluster", "succeeded", "Cluster registered with Stellar", None).await?;

            sqlx::query("UPDATE sr_cluster_nodes SET status = 'running', updated_at = CURRENT_TIMESTAMP WHERE managed_cluster_id = ?")
                .bind(task.1).execute(&self.pool).await?;
            sqlx::query("UPDATE sr_managed_clusters SET status = 'running', cluster_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(cluster.id).bind(task.1).execute(&self.pool).await?;
            sqlx::query("UPDATE sr_operation_tasks SET status = 'succeeded', current_step = 'verify', result_json = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(serde_json::json!({"cluster_id": cluster.id, "fe_host": leader.advertise_host, "query_port": leader_query_port}).to_string())
                .bind(task_id).execute(&self.pool).await?;
            self.record(task_id, "verify", "succeeded", "Deployment completed; existing cluster health checks are now active", None).await?;
            Ok(())
        }.await;

        fs::remove_dir_all(&task_dir).await.ok();
        execution
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

    async fn reserve_hosts(&self, cluster_id: i64, hosts: &[RuntimeHost]) -> ApiResult<()> {
        let mut tx = self.pool.begin().await?;
        for host in hosts {
            let inserted = sqlx::query("INSERT INTO sr_host_allocations (host_id, managed_cluster_id) VALUES (?, ?) ON CONFLICT(host_id) DO NOTHING")
                .bind(host.id)
                .bind(cluster_id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if inserted == 0 {
                return Err(ApiError::validation_error(format!(
                    "host {} is allocated to another managed cluster",
                    host.hostname
                )));
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn cache_package(&self, task_id: i64, payload: &DeploymentPayload) -> ApiResult<PathBuf> {
        self.ensure_package_source(&payload.package_url)?;
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
        sqlx::query("UPDATE sr_packages SET cached_path = ?, size_bytes = ?, status = 'cached' WHERE id = ?")
            .bind(path.to_string_lossy().to_string())
            .bind(size as i64).bind(payload.package_id).execute(&self.pool).await?;
        Ok(path)
    }

    async fn distribute_and_install(
        &self,
        task_id: i64,
        runtime: &RuntimeCluster,
        contexts: &HashMap<i64, SshContext>,
        archive: &Path,
        archive_root: &str,
    ) -> ApiResult<()> {
        let remote_dir = format!("/tmp/stellar-sr-{task_id}");
        let remote_archive = format!("{remote_dir}/package.tar.gz");
        for host in &runtime.hosts {
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
            let version_dir = format!("{}/{}", runtime.install_dir, archive_root);
            self.executor.run(context, &format!("test ! -e {} && mkdir -p {} && tar -xzf {} -C {} --no-same-owner && ln -sfn {} {}/current", shell_quote(&version_dir), shell_quote(&runtime.install_dir), shell_quote(&remote_archive), shell_quote(&runtime.install_dir), shell_quote(&version_dir), shell_quote(&runtime.install_dir))).await?;
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
            );
            write_remote_file(self.executor.as_ref(), context, &path, &config).await?;
        }
        Ok(())
    }

    async fn load_runtime(
        &self,
        managed_cluster_id: i64,
        organization_id: i64,
    ) -> ApiResult<RuntimeCluster> {
        let cluster: RuntimeClusterRow = sqlx::query_as("SELECT name, install_dir, ssh_credential_id, operator_credential_id FROM sr_managed_clusters WHERE id = ? AND organization_id = ?")
            .bind(managed_cluster_id).bind(organization_id).fetch_one(&self.pool).await?;
        let package: (String,) = sqlx::query_as("SELECT sha256 FROM sr_packages WHERE id = (SELECT package_id FROM sr_managed_clusters WHERE id = ?)")
            .bind(managed_cluster_id).fetch_one(&self.pool).await?;
        let hosts: Vec<RuntimeHost> = sqlx::query_as("SELECT DISTINCT h.id, h.hostname, h.ssh_target, h.ssh_port, h.host_key FROM physical_hosts h JOIN sr_cluster_nodes n ON n.host_id = h.id WHERE n.managed_cluster_id = ? ORDER BY h.id")
            .bind(managed_cluster_id).fetch_all(&self.pool).await?;
        let nodes: Vec<SrClusterNode> = sqlx::query_as("SELECT id, managed_cluster_id, host_id, role, fe_role, advertise_host, service_port, http_port, query_port, rpc_port, brpc_port, webserver_port, meta_dir, storage_dir, status, created_at, updated_at FROM sr_cluster_nodes WHERE managed_cluster_id = ? ORDER BY id")
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
    frontends: Vec<FrontendDeploymentNode>,
    backends: Vec<BackendDeploymentNode>,
}
impl DeploymentPayload {
    fn from_request(
        request: &CreateDeploymentRequest,
        sr_version: String,
        package_url: String,
        sha256: String,
    ) -> Self {
        Self {
            package_id: request.package_id,
            sr_version,
            package_url,
            sha256,
            frontends: request.frontends.clone(),
            backends: request.backends.clone(),
        }
    }
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
    ssh_credential_id: i64,
    operator_credential_id: i64,
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
    ssh_credential_id: i64,
    operator_credential_id: i64,
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

async fn wait_for_sql(host: &str, port: i64, sql: &str) -> ApiResult<()> {
    let mut last_error = None;
    for _ in 0..READY_RETRIES {
        match execute_sql(host, port, sql).await {
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
) -> ApiResult<()> {
    for _ in 0..READY_RETRIES {
        if let Ok(rows) =
            query_starrocks(leader_host, query_port as u16, "root", None, statement).await
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
) -> ApiResult<bool> {
    let rows = query_starrocks(leader_host, query_port as u16, "root", None, statement).await?;
    Ok(rows
        .iter()
        .any(|row| node_matches(row, node_host, service_port, false)))
}

fn node_matches(row: &[String], node_host: &str, service_port: i64, require_alive: bool) -> bool {
    row.iter().any(|value| value == node_host)
        && row.iter().any(|value| value == &service_port.to_string())
        && (!require_alive || row.iter().any(|value| value.eq_ignore_ascii_case("true")))
}

async fn execute_sql(host: &str, port: i64, sql: &str) -> ApiResult<()> {
    let pool = Pool::new(
        OptsBuilder::default()
            .ip_or_hostname(host)
            .tcp_port(port as u16)
            .user(Some("root"))
            .pass(None::<String>)
            .prefer_socket(false),
    );
    let mut connection = timeout(SQL_TIMEOUT, pool.get_conn())
        .await
        .map_err(|_| ApiError::cluster_connection_failed("FE connection timed out"))?
        .map_err(|error| {
            ApiError::cluster_connection_failed(format!("FE connection failed: {error}"))
        })?;
    let result: ApiResult<Vec<mysql_async::Row>> = timeout(SQL_TIMEOUT, connection.query(sql))
        .await
        .map_err(|_| ApiError::cluster_connection_failed("FE SQL command timed out"))
        .and_then(|result| {
            result.map_err(|_| ApiError::cluster_connection_failed("FE SQL command failed"))
        });
    drop(connection);
    pool.disconnect().await.map_err(|error| {
        ApiError::cluster_connection_failed(format!("failed to close FE connection: {error}"))
    })?;
    let _ = result?;
    Ok(())
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

    use super::{node_matches, sql_string_literal};

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
