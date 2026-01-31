//! Encryption service - database encryption management
//!
//! Uses DuckDB's native AES-256-GCM encryption with Argon2id key derivation.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use base64::Engine;
use duckdb::Connection;
use serde::Serialize;

use crate::domain::{EncryptionMetadata, EncryptionStatus};

/// Default Argon2 parameters matching Python CLI
const DEFAULT_TIME_COST: u32 = 3;
const DEFAULT_MEMORY_COST: u32 = 65536; // 64 MiB
const DEFAULT_PARALLELISM: u32 = 4;
const DEFAULT_HASH_LEN: u32 = 32;

/// Encryption service for database encryption
pub struct EncryptionService {
    treeline_dir: PathBuf,
    db_path: PathBuf,
}

impl EncryptionService {
    pub fn new(treeline_dir: PathBuf, db_path: PathBuf) -> Self {
        Self {
            treeline_dir,
            db_path,
        }
    }

    fn encryption_file(&self) -> PathBuf {
        self.treeline_dir.join("encryption.json")
    }

    /// Derive encryption key from password using Argon2id
    fn derive_key(
        &self,
        password: &str,
        salt: &[u8],
        params: &crate::domain::Argon2Params,
    ) -> Result<Vec<u8>> {
        let argon2_params = argon2::Params::new(
            params.memory_cost,
            params.time_cost,
            params.parallelism,
            Some(params.hash_len as usize),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create argon2 params: {:?}", e))?;

        let argon2 = argon2::Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2_params,
        );

        let mut key = vec![0u8; params.hash_len as usize];
        argon2
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .map_err(|e| anyhow::anyhow!("Failed to derive key: {:?}", e))?;

        Ok(key)
    }

    /// Get encryption status
    pub fn get_status(&self) -> Result<EncryptionStatus> {
        let enc_file = self.encryption_file();
        if !enc_file.exists() {
            return Ok(EncryptionStatus::unencrypted());
        }

        let content = fs::read_to_string(&enc_file)?;
        let metadata: EncryptionMetadata = serde_json::from_str(&content)?;

        Ok(EncryptionStatus::from_metadata(&metadata))
    }

    /// Check if database is encrypted
    pub fn is_encrypted(&self) -> Result<bool> {
        let status = self.get_status()?;
        Ok(status.encrypted)
    }

    /// Get the encryption key as hex string for database connections
    pub fn derive_key_for_connection(&self, password: &str) -> Result<String> {
        let enc_file = self.encryption_file();
        if !enc_file.exists() {
            anyhow::bail!("Database is not encrypted");
        }

        let content = fs::read_to_string(&enc_file)?;
        let metadata: EncryptionMetadata = serde_json::from_str(&content)?;

        if !metadata.encrypted {
            anyhow::bail!("Database is not encrypted");
        }

        let salt = base64::engine::general_purpose::STANDARD
            .decode(&metadata.salt)
            .context("Invalid salt in encryption metadata")?;

        let key = self.derive_key(password, &salt, &metadata.argon2_params)?;
        Ok(hex::encode(&key))
    }

    /// Enable encryption
    pub fn encrypt(
        &self,
        password: &str,
        backup_service: &super::BackupService,
    ) -> Result<EncryptResult> {
        if self.is_encrypted()? {
            anyhow::bail!("Database is already encrypted");
        }

        // Check database exists
        if !self.db_path.exists() {
            anyhow::bail!("Database file not found");
        }

        // Create backup first
        let backup = backup_service.create(None)?;

        // Generate salt (16 bytes like Python)
        use rand::Rng;
        let salt: [u8; 16] = rand::thread_rng().gen();
        let salt_b64 = base64::engine::general_purpose::STANDARD.encode(&salt);

        // Create Argon2 params
        let argon2_params = crate::domain::Argon2Params {
            time_cost: DEFAULT_TIME_COST,
            memory_cost: DEFAULT_MEMORY_COST,
            parallelism: DEFAULT_PARALLELISM,
            hash_len: DEFAULT_HASH_LEN,
        };

        // Derive key
        let key = self.derive_key(password, &salt, &argon2_params)?;
        let key_hex = hex::encode(&key);

        // Create temp directory for export
        let export_dir =
            tempfile::tempdir().context("Failed to create temp directory for export")?;
        let export_path = export_dir.path();

        // Create temp file for new encrypted database
        let temp_db = tempfile::NamedTempFile::new()
            .context("Failed to create temp file for encrypted database")?;
        let temp_db_path = temp_db.path().to_path_buf();
        // Close and delete the temp file so DuckDB can create fresh
        drop(temp_db);
        if temp_db_path.exists() {
            fs::remove_file(&temp_db_path)?;
        }

        // Export original database to CSV files (default format, no extensions needed)
        // IMPORTANT: Disable extension autoloading to avoid macOS code signing issues
        {
            let config = duckdb::Config::default()
                .enable_autoload_extension(false)
                .context("Failed to configure database")?;
            let conn = Connection::open_with_flags(&self.db_path, config)
                .context("Failed to open original database")?;
            conn.execute_batch(&format!("EXPORT DATABASE '{}'", export_path.display()))
                .context("Failed to export database")?;
        }

        // Create new encrypted database and import data
        {
            let config = duckdb::Config::default()
                .enable_autoload_extension(false)
                .context("Failed to configure database")?;
            let conn = Connection::open_in_memory_with_flags(config)
                .context("Failed to open in-memory connection")?;

            // Attach encrypted database
            conn.execute_batch(&format!(
                "ATTACH '{}' AS enc (ENCRYPTION_KEY '{}')",
                temp_db_path.display(),
                key_hex
            ))
            .context("Failed to attach encrypted database")?;

            // Use the attached database and import
            conn.execute_batch("USE enc")
                .context("Failed to use encrypted database")?;
            conn.execute_batch(&format!("IMPORT DATABASE '{}'", export_path.display()))
                .context("Failed to import database")?;
        }

        // Replace original with encrypted version
        fs::rename(&temp_db_path, &self.db_path)
            .or_else(|_| {
                // rename might fail across filesystems, try copy instead
                fs::copy(&temp_db_path, &self.db_path)?;
                fs::remove_file(&temp_db_path)?;
                Ok::<_, std::io::Error>(())
            })
            .context("Failed to replace original database with encrypted version")?;

        // Save encryption metadata
        let metadata = EncryptionMetadata::new_encrypted_with_params(salt_b64, argon2_params);
        let content = serde_json::to_string_pretty(&metadata)?;
        fs::write(self.encryption_file(), content)?;

        Ok(EncryptResult {
            encrypted: true,
            backup_name: Some(backup.name),
        })
    }

    /// Disable encryption
    pub fn decrypt(
        &self,
        password: &str,
        backup_service: &super::BackupService,
    ) -> Result<EncryptResult> {
        if !self.is_encrypted()? {
            anyhow::bail!("Database is not encrypted");
        }

        // Load metadata
        let enc_file = self.encryption_file();
        let content = fs::read_to_string(&enc_file)?;
        let metadata: EncryptionMetadata = serde_json::from_str(&content)?;

        // Derive key
        let salt = base64::engine::general_purpose::STANDARD
            .decode(&metadata.salt)
            .context("Invalid salt in encryption metadata")?;
        let key = self.derive_key(password, &salt, &metadata.argon2_params)?;
        let key_hex = hex::encode(&key);

        // Verify password by attempting to read the encrypted database
        // IMPORTANT: Disable extension autoloading to avoid macOS code signing issues
        {
            let config = duckdb::Config::default()
                .enable_autoload_extension(false)
                .context("Failed to configure database")?;
            let conn = Connection::open_in_memory_with_flags(config)
                .context("Failed to open in-memory connection")?;
            conn.execute_batch(&format!(
                "ATTACH '{}' AS enc (ENCRYPTION_KEY '{}', READ_ONLY)",
                self.db_path.display(),
                key_hex
            ))
            .map_err(|_| anyhow::anyhow!("Invalid password"))?;

            // Try to read something to verify
            conn.execute_batch("USE enc")
                .map_err(|_| anyhow::anyhow!("Invalid password"))?;
            conn.query_row(
                "SELECT table_name FROM information_schema.tables LIMIT 1",
                [],
                |_| Ok(()),
            )
            .map_err(|_| anyhow::anyhow!("Invalid password"))?;
        }

        // Create backup first
        let backup = backup_service.create(None)?;

        // Create temp directory for export
        let export_dir =
            tempfile::tempdir().context("Failed to create temp directory for export")?;
        let export_path = export_dir.path();

        // Create temp file for new decrypted database
        let temp_db = tempfile::NamedTempFile::new()
            .context("Failed to create temp file for decrypted database")?;
        let temp_db_path = temp_db.path().to_path_buf();
        // Close and delete the temp file so DuckDB can create fresh
        drop(temp_db);
        if temp_db_path.exists() {
            fs::remove_file(&temp_db_path)?;
        }

        // Export encrypted database to parquet files
        {
            let config = duckdb::Config::default()
                .enable_autoload_extension(false)
                .context("Failed to configure database")?;
            let conn = Connection::open_in_memory_with_flags(config)
                .context("Failed to open in-memory connection")?;
            conn.execute_batch(&format!(
                "ATTACH '{}' AS enc (ENCRYPTION_KEY '{}')",
                self.db_path.display(),
                key_hex
            ))
            .context("Failed to attach encrypted database")?;

            conn.execute_batch("USE enc")
                .context("Failed to use encrypted database")?;

            // Export to CSV (default format, no extensions needed)
            conn.execute_batch(&format!("EXPORT DATABASE '{}'", export_path.display()))
                .context("Failed to export encrypted database")?;
        }

        // Create new unencrypted database and import data
        {
            let config = duckdb::Config::default()
                .enable_autoload_extension(false)
                .context("Failed to configure database")?;
            let conn = Connection::open_with_flags(&temp_db_path, config)
                .context("Failed to create new unencrypted database")?;
            conn.execute_batch(&format!("IMPORT DATABASE '{}'", export_path.display()))
                .context("Failed to import database")?;
        }

        // Replace original with decrypted version
        fs::rename(&temp_db_path, &self.db_path)
            .or_else(|_| {
                // rename might fail across filesystems, try copy instead
                fs::copy(&temp_db_path, &self.db_path)?;
                fs::remove_file(&temp_db_path)?;
                Ok::<_, std::io::Error>(())
            })
            .context("Failed to replace original database with decrypted version")?;

        // Remove encryption metadata
        fs::remove_file(&enc_file)?;

        Ok(EncryptResult {
            encrypted: false,
            backup_name: Some(backup.name),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct EncryptResult {
    /// Whether the database is now encrypted (true after encrypt, false after decrypt)
    pub encrypted: bool,
    pub backup_name: Option<String>,
}
