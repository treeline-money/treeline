//! Encryption domain models

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Default Argon2id parameters
pub const DEFAULT_TIME_COST: u32 = 3;
pub const DEFAULT_MEMORY_COST: u32 = 65536; // 64 MiB
pub const DEFAULT_PARALLELISM: u32 = 4;
pub const DEFAULT_HASH_LEN: u32 = 32;

/// Argon2id parameters for key derivation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Argon2Params {
    pub time_cost: u32,
    pub memory_cost: u32,
    pub parallelism: u32,
    pub hash_len: u32,
}

impl Default for Argon2Params {
    fn default() -> Self {
        Self {
            time_cost: DEFAULT_TIME_COST,
            memory_cost: DEFAULT_MEMORY_COST,
            parallelism: DEFAULT_PARALLELISM,
            hash_len: DEFAULT_HASH_LEN,
        }
    }
}

/// Encryption configuration metadata stored in encryption.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionMetadata {
    pub encrypted: bool,
    /// Base64-encoded random salt
    pub salt: String,
    pub algorithm: String,
    pub version: u32,
    pub argon2_params: Argon2Params,
}

impl EncryptionMetadata {
    /// Create metadata for a new encrypted database
    pub fn new_encrypted(salt: String) -> Self {
        Self {
            encrypted: true,
            salt,
            algorithm: "argon2id".to_string(),
            version: 1,
            argon2_params: Argon2Params::default(),
        }
    }

    /// Create metadata for a new encrypted database with custom params
    pub fn new_encrypted_with_params(salt: String, argon2_params: Argon2Params) -> Self {
        Self {
            encrypted: true,
            salt,
            algorithm: "argon2id".to_string(),
            version: 1,
            argon2_params,
        }
    }

    /// Convert to HashMap for legacy compatibility
    pub fn argon2_params_map(&self) -> HashMap<String, u32> {
        let mut map = HashMap::new();
        map.insert("time_cost".to_string(), self.argon2_params.time_cost);
        map.insert("memory_cost".to_string(), self.argon2_params.memory_cost);
        map.insert("parallelism".to_string(), self.argon2_params.parallelism);
        map.insert("hash_len".to_string(), self.argon2_params.hash_len);
        map
    }
}

/// Status of database encryption for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionStatus {
    pub encrypted: bool,
    pub algorithm: Option<String>,
    pub version: Option<u32>,
}

impl EncryptionStatus {
    pub fn unencrypted() -> Self {
        Self {
            encrypted: false,
            algorithm: None,
            version: None,
        }
    }

    pub fn from_metadata(meta: &EncryptionMetadata) -> Self {
        Self {
            encrypted: meta.encrypted,
            algorithm: Some(meta.algorithm.clone()),
            version: Some(meta.version),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_metadata_creation() {
        let meta = EncryptionMetadata::new_encrypted("base64salt==".to_string());
        assert!(meta.encrypted);
        assert_eq!(meta.algorithm, "argon2id");
        assert_eq!(meta.version, 1);
        assert_eq!(meta.argon2_params.memory_cost, 65536);
    }

    #[test]
    fn test_encryption_status() {
        let status = EncryptionStatus::unencrypted();
        assert!(!status.encrypted);
        assert!(status.algorithm.is_none());
    }
}
