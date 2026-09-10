use sqlx::SqlitePool;

use crate::{
    models::{
        CreateSrDatabaseCredentialRequest, CreateSshCredentialRequest, SrDatabaseCredential,
        SshCredential,
    },
    utils::{ApiError, ApiResult},
};

use super::cipher::{DeploymentCipher, EncryptedSecret};

#[derive(Clone)]
pub struct CredentialService {
    pool: SqlitePool,
    cipher: Option<DeploymentCipher>,
}

impl CredentialService {
    pub fn new(pool: SqlitePool, encryption_key: &str) -> Self {
        let cipher = DeploymentCipher::from_key_material(encryption_key).ok();
        Self { pool, cipher }
    }

    fn cipher(&self) -> ApiResult<&DeploymentCipher> {
        self.cipher.as_ref().ok_or_else(|| {
            ApiError::validation_error(
                "physical deployment credentials require APP_SR_PHYSICAL_ENCRYPTION_KEY",
            )
        })
    }

    pub async fn list_ssh(&self, organization_id: Option<i64>) -> ApiResult<Vec<SshCredential>> {
        match organization_id {
            Some(organization_id) => sqlx::query_as(
                "SELECT id, organization_id, name, username, auth_type, created_at, updated_at FROM ssh_credentials WHERE organization_id = ? ORDER BY id DESC",
            )
            .bind(organization_id)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into),
            None => sqlx::query_as(
                "SELECT id, organization_id, name, username, auth_type, created_at, updated_at FROM ssh_credentials ORDER BY id DESC",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into),
        }
    }

    pub async fn create_ssh(
        &self,
        request: CreateSshCredentialRequest,
        organization_id: i64,
    ) -> ApiResult<SshCredential> {
        let request = request.normalize()?;
        let encrypted = self
            .cipher()?
            .encrypt(&request.private_key, &credential_aad(organization_id, "ssh"))?;
        ensure_organization_exists(&self.pool, organization_id).await?;

        let result = sqlx::query(
            "INSERT INTO ssh_credentials (organization_id, name, username, auth_type, key_version, secret_nonce, secret_ciphertext) VALUES (?, ?, ?, 'key', ?, ?, ?)",
        )
        .bind(organization_id)
        .bind(&request.name)
        .bind(&request.username)
        .bind(encrypted.key_version)
        .bind(encrypted.nonce)
        .bind(encrypted.ciphertext)
        .execute(&self.pool)
        .await
        .map_err(|error| ApiError::validation_error(format!("failed to create SSH credential: {error}")))?;

        sqlx::query_as(
            "SELECT id, organization_id, name, username, auth_type, created_at, updated_at FROM ssh_credentials WHERE id = ?",
        )
        .bind(result.last_insert_rowid())
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn delete_ssh(&self, id: i64, organization_id: Option<i64>) -> ApiResult<()> {
        let referenced: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM sr_managed_clusters WHERE ssh_credential_id = ? LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        if referenced.is_some() {
            return Err(ApiError::validation_error(
                "SSH credential is referenced by a managed cluster",
            ));
        }
        delete_scoped(&self.pool, "ssh_credentials", id, organization_id).await
    }

    pub async fn list_database(
        &self,
        organization_id: Option<i64>,
    ) -> ApiResult<Vec<SrDatabaseCredential>> {
        match organization_id {
            Some(organization_id) => sqlx::query_as(
                "SELECT id, organization_id, name, username, created_at, updated_at FROM sr_database_credentials WHERE organization_id = ? ORDER BY id DESC",
            )
            .bind(organization_id)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into),
            None => sqlx::query_as(
                "SELECT id, organization_id, name, username, created_at, updated_at FROM sr_database_credentials ORDER BY id DESC",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into),
        }
    }

    pub async fn create_database(
        &self,
        request: CreateSrDatabaseCredentialRequest,
        organization_id: i64,
    ) -> ApiResult<SrDatabaseCredential> {
        let request = request.normalize()?;
        let encrypted = self
            .cipher()?
            .encrypt(&request.password, &credential_aad(organization_id, "database"))?;
        ensure_organization_exists(&self.pool, organization_id).await?;

        let result = sqlx::query(
            "INSERT INTO sr_database_credentials (organization_id, name, username, key_version, secret_nonce, secret_ciphertext) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(organization_id)
        .bind(&request.name)
        .bind(&request.username)
        .bind(encrypted.key_version)
        .bind(encrypted.nonce)
        .bind(encrypted.ciphertext)
        .execute(&self.pool)
        .await
        .map_err(|error| ApiError::validation_error(format!("failed to create database credential: {error}")))?;

        sqlx::query_as(
            "SELECT id, organization_id, name, username, created_at, updated_at FROM sr_database_credentials WHERE id = ?",
        )
        .bind(result.last_insert_rowid())
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn delete_database(&self, id: i64, organization_id: Option<i64>) -> ApiResult<()> {
        let referenced: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM sr_managed_clusters WHERE operator_credential_id = ? LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        if referenced.is_some() {
            return Err(ApiError::validation_error(
                "database credential is referenced by a managed cluster",
            ));
        }
        delete_scoped(&self.pool, "sr_database_credentials", id, organization_id).await
    }

    pub async fn ssh_secret(
        &self,
        id: i64,
        organization_id: i64,
    ) -> ApiResult<(SshCredential, String)> {
        let row: EncryptedSshCredential = sqlx::query_as(
            "SELECT id, organization_id, name, username, auth_type, created_at, updated_at, key_version, secret_nonce, secret_ciphertext FROM ssh_credentials WHERE id = ? AND organization_id = ?",
        )
        .bind(id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("SSH credential not found"))?;
        let secret = decrypt(
            self.cipher()?,
            row.key_version,
            row.secret_nonce.clone(),
            row.secret_ciphertext.clone(),
            organization_id,
            "ssh",
        )?;
        Ok((row.public(), secret))
    }

    pub async fn database_secret(
        &self,
        id: i64,
        organization_id: i64,
    ) -> ApiResult<(SrDatabaseCredential, String)> {
        let row: EncryptedDatabaseCredential = sqlx::query_as(
            "SELECT id, organization_id, name, username, created_at, updated_at, key_version, secret_nonce, secret_ciphertext FROM sr_database_credentials WHERE id = ? AND organization_id = ?",
        )
        .bind(id)
        .bind(organization_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("database credential not found"))?;
        let secret = decrypt(
            self.cipher()?,
            row.key_version,
            row.secret_nonce.clone(),
            row.secret_ciphertext.clone(),
            organization_id,
            "database",
        )?;
        Ok((row.public(), secret))
    }
}

#[derive(sqlx::FromRow)]
struct EncryptedSshCredential {
    id: i64,
    organization_id: i64,
    name: String,
    username: String,
    auth_type: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    key_version: i64,
    secret_nonce: Vec<u8>,
    secret_ciphertext: Vec<u8>,
}

impl EncryptedSshCredential {
    fn public(self) -> SshCredential {
        SshCredential {
            id: self.id,
            organization_id: self.organization_id,
            name: self.name,
            username: self.username,
            auth_type: self.auth_type,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct EncryptedDatabaseCredential {
    id: i64,
    organization_id: i64,
    name: String,
    username: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    key_version: i64,
    secret_nonce: Vec<u8>,
    secret_ciphertext: Vec<u8>,
}

impl EncryptedDatabaseCredential {
    fn public(self) -> SrDatabaseCredential {
        SrDatabaseCredential {
            id: self.id,
            organization_id: self.organization_id,
            name: self.name,
            username: self.username,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

fn decrypt(
    cipher: &DeploymentCipher,
    key_version: i64,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    organization_id: i64,
    kind: &str,
) -> ApiResult<String> {
    cipher.decrypt(
        &EncryptedSecret { key_version, nonce, ciphertext },
        &credential_aad(organization_id, kind),
    )
}

fn credential_aad(organization_id: i64, kind: &str) -> String {
    format!("stellar:sr-physical:{organization_id}:{kind}")
}

async fn ensure_organization_exists(pool: &SqlitePool, organization_id: i64) -> ApiResult<()> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM organizations WHERE id = ?")
        .bind(organization_id)
        .fetch_optional(pool)
        .await?;
    if exists.is_none() {
        return Err(ApiError::validation_error("Organization not found"));
    }
    Ok(())
}

async fn delete_scoped(
    pool: &SqlitePool,
    table: &str,
    id: i64,
    organization_id: Option<i64>,
) -> ApiResult<()> {
    let sql = if organization_id.is_some() {
        format!("DELETE FROM {table} WHERE id = ? AND organization_id = ?")
    } else {
        format!("DELETE FROM {table} WHERE id = ?")
    };
    let mut query = sqlx::query(&sql).bind(id);
    if let Some(organization_id) = organization_id {
        query = query.bind(organization_id);
    }
    if query.execute(pool).await?.rows_affected() == 0 {
        return Err(ApiError::not_found("deployment credential not found"));
    }
    Ok(())
}
