//! Server-side encryption for stored LLM provider API keys.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use super::LLMError;

const CREDENTIAL_VERSION: &str = "v1:";

/// Encrypts provider credentials with a key independent from the JWT signing key.
#[derive(Clone)]
pub struct LlmCredentialCipher {
    key: Option<[u8; 32]>,
}

impl LlmCredentialCipher {
    pub fn new(key_material: &str) -> Self {
        let key_material = key_material.trim();
        let key = (!key_material.is_empty()).then(|| {
            let mut hash = Sha256::new();
            hash.update(b"stellar.llm-provider.credentials.v1\0");
            hash.update(key_material.as_bytes());
            hash.finalize().into()
        });
        Self { key }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, LLMError> {
        let key = self.key.ok_or(LLMError::CredentialEncryptionUnavailable)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|_| LLMError::ApiError("Invalid LLM credential encryption key".to_string()))?;
        let nonce = Aes256Gcm::generate_nonce(&mut aes_gcm::aead::OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| LLMError::ApiError("Failed to encrypt LLM API key".to_string()))?;
        let mut payload = nonce.to_vec();
        payload.extend(ciphertext);
        Ok(format!("{}{}", CREDENTIAL_VERSION, URL_SAFE_NO_PAD.encode(payload)))
    }

    pub fn decrypt(&self, value: &str) -> Result<String, LLMError> {
        let encoded = value.strip_prefix(CREDENTIAL_VERSION).ok_or_else(|| {
            LLMError::ApiError("Unsupported encrypted LLM API key format".to_string())
        })?;
        let payload = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| LLMError::ApiError("Invalid encrypted LLM API key".to_string()))?;
        if payload.len() <= 12 {
            return Err(LLMError::ApiError("Invalid encrypted LLM API key".to_string()));
        }

        let key = self.key.ok_or(LLMError::CredentialEncryptionUnavailable)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|_| LLMError::ApiError("Invalid LLM credential encryption key".to_string()))?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&payload[..12]), &payload[12..])
            .map_err(|_| LLMError::ApiError("Unable to decrypt LLM API key".to_string()))?;
        String::from_utf8(plaintext)
            .map_err(|_| LLMError::ApiError("Invalid decrypted LLM API key".to_string()))
    }

    pub fn is_encrypted(value: &str) -> bool {
        value.starts_with(CREDENTIAL_VERSION)
    }
}
