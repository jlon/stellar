use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore},
};
use sha2::{Digest, Sha256};

use crate::utils::{ApiError, ApiResult};

pub const KEY_VERSION: i64 = 1;

#[derive(Clone)]
pub struct DeploymentCipher {
    key: [u8; 32],
}

pub struct EncryptedSecret {
    pub key_version: i64,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl DeploymentCipher {
    pub fn from_key_material(key_material: &str) -> ApiResult<Self> {
        let key_material = key_material.trim();
        if key_material.len() < 16 {
            return Err(ApiError::validation_error(
                "APP_SR_PHYSICAL_ENCRYPTION_KEY must contain at least 16 characters",
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(b"stellar-sr-physical-credential-v1\0");
        hasher.update(key_material.as_bytes());
        let key: [u8; 32] = hasher.finalize().into();
        Ok(Self { key })
    }

    pub fn encrypt(&self, plaintext: &str, aad: &str) -> ApiResult<EncryptedSecret> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| ApiError::internal_error("failed to initialize credential cipher"))?;
        let mut nonce = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload { msg: plaintext.as_bytes(), aad: aad.as_bytes() },
            )
            .map_err(|_| ApiError::internal_error("failed to encrypt deployment credential"))?;

        Ok(EncryptedSecret { key_version: KEY_VERSION, nonce: nonce.to_vec(), ciphertext })
    }

    pub fn decrypt(&self, encrypted: &EncryptedSecret, aad: &str) -> ApiResult<String> {
        if encrypted.key_version != KEY_VERSION || encrypted.nonce.len() != 12 {
            return Err(ApiError::internal_error("unsupported deployment credential key version"));
        }

        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| ApiError::internal_error("failed to initialize credential cipher"))?;
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&encrypted.nonce),
                Payload { msg: &encrypted.ciphertext, aad: aad.as_bytes() },
            )
            .map_err(|_| ApiError::forbidden("unable to decrypt deployment credential"))?;

        String::from_utf8(plaintext)
            .map_err(|_| ApiError::internal_error("deployment credential is not valid UTF-8"))
    }
}

#[cfg(test)]
mod tests {
    use super::DeploymentCipher;

    #[test]
    fn encrypts_and_decrypts_with_bound_aad() {
        let cipher = DeploymentCipher::from_key_material("a-strong-independent-test-key").unwrap();
        let encrypted = cipher.encrypt("secret", "org:1:ssh").unwrap();

        assert_eq!(cipher.decrypt(&encrypted, "org:1:ssh").unwrap(), "secret");
        assert!(cipher.decrypt(&encrypted, "org:2:ssh").is_err());
    }
}
