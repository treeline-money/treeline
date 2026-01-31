//! Backup service - database backup management
//!
//! Creates ZIP archives containing the database and config files,
//! compatible with the Python CLI backup format.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::adapters::duckdb::DuckDbRepository;
use crate::domain::BackupMetadata;

/// Config files to include in backup (relative to treeline dir)
const CONFIG_FILES: &[&str] = &["settings.json", "encryption.json"];

/// Backup service for database backup management
///
/// The repository is optional - if provided, create() will checkpoint
/// before copying to ensure WAL data is flushed. For operations that
/// don't need the database (list, restore, clear), use new() without
/// a repository.
pub struct BackupService {
    treeline_dir: PathBuf,
    db_filename: String,
    repository: Option<Arc<DuckDbRepository>>,
}

impl BackupService {
    /// Create a new BackupService without repository access.
    ///
    /// Use this for operations that don't need database access (list, restore, clear).
    /// Note: create() will skip checkpointing when called without a repository.
    pub fn new(treeline_dir: PathBuf, db_filename: String) -> Self {
        Self {
            treeline_dir,
            db_filename,
            repository: None,
        }
    }

    /// Create a new BackupService with repository access.
    ///
    /// This enables checkpointing before backup to ensure WAL data is flushed.
    pub fn new_with_repository(
        treeline_dir: PathBuf,
        db_filename: String,
        repository: Arc<DuckDbRepository>,
    ) -> Self {
        Self {
            treeline_dir,
            db_filename,
            repository: Some(repository),
        }
    }

    fn backups_dir(&self) -> PathBuf {
        self.treeline_dir.join("backups")
    }

    /// Create a backup of the database and config files as a ZIP archive
    ///
    /// If a repository is available, this method first forces a checkpoint
    /// to flush any pending WAL data to the main database file, ensuring
    /// backup consistency.
    pub fn create(&self, max_backups: Option<usize>) -> Result<BackupMetadata> {
        let backups_dir = self.backups_dir();
        fs::create_dir_all(&backups_dir)?;

        let db_path = self.treeline_dir.join(&self.db_filename);
        if !db_path.exists() {
            anyhow::bail!("Database file not found");
        }

        // Force checkpoint to flush WAL to main database file before backup.
        // This ensures the backup contains all committed data.
        // If no repository is available (e.g., during encryption), skip checkpointing.
        if let Some(ref repo) = self.repository {
            repo.checkpoint()?;
        }

        let now = Utc::now();
        let timestamp = now.format("%Y-%m-%dT%H-%M-%S");
        let micros = now.timestamp_subsec_micros();
        // COMMENT: the previous Python CLI allowed users to provide
        // a backup name, the generated one was a fallback. I'm ok with this,
        // cause at least it works. But consider it.
        let backup_name = format!("treeline-{}-{:06}.zip", timestamp, micros);
        let backup_path = backups_dir.join(&backup_name);

        // Create ZIP archive
        let file = File::create(&backup_path).context("Failed to create backup file")?;
        let mut zip = ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        // Add database file
        zip.start_file(&self.db_filename, options)?;
        let mut db_file = File::open(&db_path)?;
        let mut buffer = Vec::new();
        db_file.read_to_end(&mut buffer)?;
        zip.write_all(&buffer)?;

        // Add config files if they exist
        for config_file in CONFIG_FILES {
            let config_path = self.treeline_dir.join(config_file);
            if config_path.exists() {
                zip.start_file(*config_file, options)?;
                let mut cf = File::open(&config_path)?;
                buffer.clear();
                cf.read_to_end(&mut buffer)?;
                zip.write_all(&buffer)?;
            }
        }

        zip.finish()?;

        let metadata = fs::metadata(&backup_path)?;
        let size_bytes = metadata.len();

        // Apply retention policy
        if let Some(max) = max_backups {
            self.apply_retention(max)?;
        }

        Ok(BackupMetadata {
            name: backup_name,
            created_at: Utc::now(),
            size_bytes,
        })
    }

    /// List all backups (both .zip and legacy .duckdb formats)
    pub fn list(&self) -> Result<Vec<BackupMetadata>> {
        let backups_dir = self.backups_dir();
        if !backups_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();
        for entry in fs::read_dir(&backups_dir)? {
            let entry = entry?;
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());

            // Support both .zip (new) and .duckdb (legacy) formats
            if ext != Some("zip") && ext != Some("duckdb") {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            if !name.starts_with("treeline-") {
                continue;
            }

            let metadata = fs::metadata(&path)?;
            let size_bytes = metadata.len();

            // Parse timestamp from filename
            let created_at = self.parse_backup_time(&name);

            backups.push(BackupMetadata {
                name,
                created_at,
                size_bytes,
            });
        }

        backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(backups)
    }

    /// Parse creation time from backup filename
    fn parse_backup_time(&self, backup_name: &str) -> chrono::DateTime<Utc> {
        // Extract timestamp part: "treeline-TIMESTAMP.zip" or "treeline-TIMESTAMP.duckdb"
        let ts = backup_name
            .strip_prefix("treeline-")
            .and_then(|s| s.strip_suffix(".zip").or_else(|| s.strip_suffix(".duckdb")));

        if let Some(ts) = ts {
            // Try with microseconds first, then without
            chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H-%M-%S-%f")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H-%M-%S"))
                .map(|dt| dt.and_utc())
                .unwrap_or_else(|_| Utc::now())
        } else {
            Utc::now()
        }
    }

    /// Restore from a backup
    pub fn restore(&self, backup_name: &str) -> Result<()> {
        let backup_path = self.backups_dir().join(backup_name);
        if !backup_path.exists() {
            anyhow::bail!("Backup not found: {}", backup_name);
        }

        let db_path = self.treeline_dir.join(&self.db_filename);

        // Create a backup of current state first
        if db_path.exists() {
            let now = Utc::now();
            let timestamp = now.format("%Y-%m-%dT%H-%M-%S");
            let micros = now.timestamp_subsec_micros();
            let pre_restore_backup =
                format!("treeline-pre-restore-{}-{:06}.zip", timestamp, micros);
            let pre_restore_path = self.backups_dir().join(&pre_restore_backup);

            // Create a quick backup of just the DB
            let file = File::create(&pre_restore_path)?;
            let mut zip = ZipWriter::new(file);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

            zip.start_file(&self.db_filename, options)?;
            let mut db_file = File::open(&db_path)?;
            let mut buffer = Vec::new();
            db_file.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
            zip.finish()?;
        }

        // Restore based on backup format
        if backup_name.ends_with(".zip") {
            // New ZIP format - extract all files
            let file = File::open(&backup_path)?;
            let mut archive = ZipArchive::new(file)?;

            // Track which config files are in the backup
            let mut restored_configs: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            for i in 0..archive.len() {
                let mut file = archive.by_index(i)?;
                let name = file.name().to_string();

                let target_path = if name.ends_with(".duckdb") {
                    self.treeline_dir.join(&self.db_filename)
                } else {
                    // Track config files that are being restored
                    if CONFIG_FILES.contains(&name.as_str()) {
                        restored_configs.insert(name.clone());
                    }
                    self.treeline_dir.join(&name)
                };

                let mut outfile = File::create(&target_path)?;
                std::io::copy(&mut file, &mut outfile)?;
            }

            // Remove config files that were NOT in the backup
            // This ensures encryption.json is removed when restoring an unencrypted backup
            for config_file in CONFIG_FILES {
                if !restored_configs.contains(*config_file) {
                    let config_path = self.treeline_dir.join(config_file);
                    if config_path.exists() {
                        fs::remove_file(&config_path)?;
                    }
                }
            }
        } else {
            // Legacy .duckdb format - simple copy
            // Also remove encryption.json since legacy backups are unencrypted
            let enc_path = self.treeline_dir.join("encryption.json");
            if enc_path.exists() {
                fs::remove_file(&enc_path)?;
            }
            fs::copy(&backup_path, &db_path).context("Failed to restore backup")?;
        }

        Ok(())
    }

    /// Clear all backups (both .zip and legacy .duckdb)
    pub fn clear(&self) -> Result<ClearResult> {
        let backups = self.list()?;
        let count = backups.len() as i64;

        for backup in &backups {
            let path = self.backups_dir().join(&backup.name);
            fs::remove_file(path)?;
        }

        Ok(ClearResult { deleted: count })
    }

    fn apply_retention(&self, max_backups: usize) -> Result<()> {
        let mut backups = self.list()?;

        while backups.len() > max_backups {
            if let Some(oldest) = backups.pop() {
                let path = self.backups_dir().join(&oldest.name);
                fs::remove_file(path)?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct ClearResult {
    pub deleted: i64,
}
