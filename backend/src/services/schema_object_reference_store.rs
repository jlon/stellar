use crate::{
    middleware::OrgContext,
    models::SchemaObjectIdentity,
    utils::{ApiError, ApiResult},
};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const OBJECT_REFERENCE_TTL_MINUTES: i64 = 10;
const NONCE_BYTES: usize = 12;
const MAX_TOKEN_BYTES: usize = 4_096;

#[derive(Deserialize, Serialize)]
struct SchemaObjectReferenceClaims {
    identity: SchemaObjectIdentity,
    user_id: i64,
    organization_id: Option<i64>,
    expires_at: i64,
}

/// Encrypts short-lived object identities, so references work across replicas
/// without putting paths or tenancy data in a public URL.
pub struct SchemaObjectReferenceStore {
    cipher: Aes256Gcm,
}

impl SchemaObjectReferenceStore {
    pub fn new(secret: &str) -> Self {
        let key = Sha256::digest(secret.as_bytes());
        Self { cipher: Aes256Gcm::new_from_slice(&key).expect("SHA-256 is a valid AES-256 key") }
    }

    pub fn issue(&self, identity: SchemaObjectIdentity, org_ctx: &OrgContext) -> ApiResult<String> {
        let now = Utc::now();
        let claims = SchemaObjectReferenceClaims {
            identity,
            user_id: org_ctx.user_id,
            organization_id: org_ctx.organization_id,
            expires_at: (now + Duration::minutes(OBJECT_REFERENCE_TTL_MINUTES)).timestamp(),
        };
        let plaintext = serde_json::to_vec(&claims).map_err(|error| {
            ApiError::internal_error(format!("Failed to encode schema reference: {error}"))
        })?;
        let mut nonce = [0_u8; NONCE_BYTES];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
            .map_err(|_| ApiError::internal_error("Failed to encrypt schema reference"))?;
        let mut token = Vec::with_capacity(NONCE_BYTES + ciphertext.len());
        token.extend_from_slice(&nonce);
        token.extend_from_slice(&ciphertext);
        Ok(URL_SAFE_NO_PAD.encode(token))
    }

    pub fn resolve(
        &self,
        object_ref: &str,
        cluster_id: i64,
        org_ctx: &OrgContext,
    ) -> ApiResult<SchemaObjectIdentity> {
        if object_ref.len() > MAX_TOKEN_BYTES {
            return Err(ApiError::not_found("Schema object reference not found or expired"));
        }
        let token = URL_SAFE_NO_PAD
            .decode(object_ref)
            .map_err(|_| ApiError::not_found("Schema object reference not found or expired"))?;
        let Some((nonce, ciphertext)) = token.split_first_chunk::<NONCE_BYTES>() else {
            return Err(ApiError::not_found("Schema object reference not found or expired"));
        };
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| ApiError::not_found("Schema object reference not found or expired"))?;
        let claims: SchemaObjectReferenceClaims = serde_json::from_slice(&plaintext)
            .map_err(|_| ApiError::not_found("Schema object reference not found or expired"))?;

        if claims.expires_at <= Utc::now().timestamp()
            || claims.identity.cluster_id != cluster_id
            || claims.user_id != org_ctx.user_id
            || claims.organization_id != org_ctx.organization_id
        {
            return Err(ApiError::not_found("Schema object reference not found"));
        }

        Ok(claims.identity)
    }
}
