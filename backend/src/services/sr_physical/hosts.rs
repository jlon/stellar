use sqlx::SqlitePool;

use crate::{
    models::{CreatePhysicalHostRequest, PhysicalHost},
    utils::{ApiError, ApiResult},
};

#[derive(Clone)]
pub struct PhysicalHostService {
    pool: SqlitePool,
}

impl PhysicalHostService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list_hosts(&self, organization_id: Option<i64>) -> ApiResult<Vec<PhysicalHost>> {
        let hosts = match organization_id {
            Some(organization_id) => {
                sqlx::query_as(
                    r#"
                    SELECT id, organization_id, hostname, ssh_target, ssh_port,
                           host_key_fingerprint, status, created_at, updated_at
                    FROM physical_hosts
                    WHERE organization_id = ?
                    ORDER BY id DESC
                    "#,
                )
                .bind(organization_id)
                .fetch_all(&self.pool)
                .await?
            },
            None => {
                sqlx::query_as(
                    r#"
                    SELECT id, organization_id, hostname, ssh_target, ssh_port,
                           host_key_fingerprint, status, created_at, updated_at
                    FROM physical_hosts
                    ORDER BY id DESC
                    "#,
                )
                .fetch_all(&self.pool)
                .await?
            },
        };

        Ok(hosts)
    }

    pub async fn create_host(
        &self,
        request: CreatePhysicalHostRequest,
        organization_id: i64,
    ) -> ApiResult<PhysicalHost> {
        let request = request.normalize()?;

        let organization_exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM organizations WHERE id = ?")
                .bind(organization_id)
                .fetch_optional(&self.pool)
                .await?;
        if organization_exists.is_none() {
            return Err(ApiError::validation_error("Organization not found"));
        }

        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM physical_hosts WHERE organization_id = ? AND ssh_target = ? AND ssh_port = ?",
        )
        .bind(organization_id)
        .bind(&request.ssh_target)
        .bind(i64::from(request.ssh_port))
        .fetch_optional(&self.pool)
        .await?;
        if existing.is_some() {
            return Err(ApiError::validation_error(
                "A host with this SSH target and port already exists in the organization",
            ));
        }

        let result = sqlx::query(
            r#"
            INSERT INTO physical_hosts (
                organization_id, hostname, ssh_target, ssh_port, host_key, host_key_fingerprint
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(organization_id)
        .bind(&request.hostname)
        .bind(&request.ssh_target)
        .bind(i64::from(request.ssh_port))
        .bind(&request.host_key)
        .bind(&request.host_key_fingerprint)
        .execute(&self.pool)
        .await?;

        let host = sqlx::query_as(
            r#"
            SELECT id, organization_id, hostname, ssh_target, ssh_port,
                   host_key_fingerprint, status, created_at, updated_at
            FROM physical_hosts
            WHERE id = ?
            "#,
        )
        .bind(result.last_insert_rowid())
        .fetch_one(&self.pool)
        .await?;

        Ok(host)
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::PhysicalHostService;
    use crate::models::{CreatePhysicalHostRequest, sr_physical::host_key_fingerprint};

    async fn test_service() -> PhysicalHostService {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("test database should connect");
        sqlx::migrate!()
            .run(&pool)
            .await
            .expect("migrations should run");

        PhysicalHostService::new(pool)
    }

    fn request() -> CreatePhysicalHostRequest {
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

    #[tokio::test]
    async fn creates_a_host_without_exposing_its_public_key() {
        let service = test_service().await;
        let organization_id: i64 =
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_one(&service.pool)
                .await
                .expect("default organization should exist");

        let host = service
            .create_host(request(), organization_id)
            .await
            .expect("host should be created");

        assert_eq!(host.hostname, "be-01");
        assert_eq!(host.status, "unknown");
        assert_eq!(
            service
                .list_hosts(Some(organization_id))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn rejects_duplicate_ssh_targets_within_an_organization() {
        let service = test_service().await;
        let organization_id: i64 =
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_one(&service.pool)
                .await
                .expect("default organization should exist");

        service
            .create_host(request(), organization_id)
            .await
            .unwrap();

        assert!(
            service
                .create_host(request(), organization_id)
                .await
                .is_err()
        );
    }
}
