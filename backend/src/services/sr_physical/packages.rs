use std::collections::HashSet;

use sqlx::SqlitePool;

use crate::{
    models::{CreateSrPackageRequest, SrPackage},
    utils::{ApiError, ApiResult},
};

#[derive(Clone)]
pub struct PackageService {
    pool: SqlitePool,
    supported_versions: HashSet<String>,
}

impl PackageService {
    pub fn new(pool: SqlitePool, supported_versions: Vec<String>) -> Self {
        Self { pool, supported_versions: supported_versions.into_iter().collect() }
    }

    pub async fn list_packages(&self, organization_id: Option<i64>) -> ApiResult<Vec<SrPackage>> {
        let packages = match organization_id {
            Some(organization_id) => {
                sqlx::query_as(
                    r#"
                    SELECT id, organization_id, version, package_url, local_path, sha256, status, created_at
                    FROM sr_packages
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
                    SELECT id, organization_id, version, package_url, local_path, sha256, status, created_at
                    FROM sr_packages
                    ORDER BY id DESC
                    "#,
                )
                .fetch_all(&self.pool)
                .await?
            },
        };

        Ok(packages)
    }

    pub async fn create_package(
        &self,
        request: CreateSrPackageRequest,
        organization_id: i64,
    ) -> ApiResult<SrPackage> {
        let request = request.normalize()?;
        if self.supported_versions.is_empty() || !self.supported_versions.contains(&request.version)
        {
            return Err(ApiError::validation_error(
                "package version is not in APP_SR_PHYSICAL_SUPPORTED_VERSIONS",
            ));
        }

        let organization_exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM organizations WHERE id = ?")
                .bind(organization_id)
                .fetch_optional(&self.pool)
                .await?;
        if organization_exists.is_none() {
            return Err(ApiError::validation_error("Organization not found"));
        }

        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM sr_packages WHERE organization_id = ? AND version = ? AND sha256 = ?",
        )
        .bind(organization_id)
        .bind(&request.version)
        .bind(&request.sha256)
        .fetch_optional(&self.pool)
        .await?;
        if existing.is_some() {
            return Err(ApiError::validation_error(
                "This package version and SHA-256 digest is already registered",
            ));
        }

        let result = sqlx::query(
            r#"
            INSERT INTO sr_packages (organization_id, version, package_url, local_path, sha256)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(organization_id)
        .bind(&request.version)
        .bind(&request.package_url.as_deref().unwrap_or(""))
        .bind(&request.local_path)
        .bind(&request.sha256)
        .execute(&self.pool)
        .await?;

        let package = sqlx::query_as(
            r#"
            SELECT id, organization_id, version, package_url, local_path, sha256, status, created_at
            FROM sr_packages
            WHERE id = ?
            "#,
        )
        .bind(result.last_insert_rowid())
        .fetch_one(&self.pool)
        .await?;

        Ok(package)
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::PackageService;
    use crate::models::CreateSrPackageRequest;

    async fn test_service() -> PackageService {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("test database should connect");
        sqlx::migrate!()
            .run(&pool)
            .await
            .expect("migrations should run");

        PackageService::new(pool, vec!["3.3.9".to_string()])
    }

    fn request() -> CreateSrPackageRequest {
        CreateSrPackageRequest {
            organization_id: Some(1),
            version: "3.3.9".to_string(),
            package_url: Some("https://packages.example.com/starrocks-3.3.9.tar.gz".to_string()),
            local_path: None,
            sha256: "a".repeat(64),
        }
    }

    #[tokio::test]
    async fn creates_a_pending_package_record() {
        let service = test_service().await;
        let organization_id: i64 =
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_one(&service.pool)
                .await
                .expect("default organization should exist");

        let package = service
            .create_package(request(), organization_id)
            .await
            .expect("package should be created");

        assert_eq!(package.status, "pending");
        assert_eq!(
            service
                .list_packages(Some(organization_id))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn creates_a_package_with_a_preset_local_path() {
        let service = test_service().await;
        let organization_id: i64 =
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_one(&service.pool)
                .await
                .expect("default organization should exist");

        let request = CreateSrPackageRequest {
            package_url: None,
            local_path: Some("/opt/stellar/packages/starrocks-3.3.9.tar.gz".to_string()),
            ..request()
        };
        let package = service
            .create_package(request, organization_id)
            .await
            .expect("preset package should be created");

        assert_eq!(package.package_url, "");
        assert_eq!(
            package.local_path.as_deref(),
            Some("/opt/stellar/packages/starrocks-3.3.9.tar.gz")
        );
    }

    #[tokio::test]
    async fn rejects_a_duplicate_package_digest() {
        let service = test_service().await;
        let organization_id: i64 =
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_one(&service.pool)
                .await
                .expect("default organization should exist");

        service
            .create_package(request(), organization_id)
            .await
            .unwrap();

        assert!(
            service
                .create_package(request(), organization_id)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn rejects_an_unverified_package_version() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        let service = PackageService::new(pool.clone(), vec!["3.3.8".to_string()]);
        let organization_id: i64 =
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert!(
            service
                .create_package(request(), organization_id)
                .await
                .is_err()
        );
    }
}
