//! Hub service - manages database sync for hub operations
//!
//! Handles accepting pushed databases, serving pulls, and
//! token-based authentication for the hub server.
//!
//! The hub treats sync bundles as opaque blobs — it doesn't
//! need to open or understand the contents to accept pushes
//! or serve pulls.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use zip::write::SimpleFileOptions;

use crate::services::BackupService;

/// Files included in sync bundles (allowlist).
/// These are the files that define a Treeline instance's state.
const SYNC_FILES: &[&str] = &["treeline.duckdb", "encryption.json", "settings.json"];

/// Directories included in sync bundles (recursive, allowlist).
const SYNC_DIRS: &[&str] = &["skills", "plugins"];

/// Hub service for managing database sync
pub struct HubService {
    treeline_dir: PathBuf,
    db_filename: String,
}

/// Result of accepting a push
#[derive(Debug, Serialize)]
pub struct PushResult {
    /// Name of the backup created before replacing (None on first push)
    pub backup_name: Option<String>,
    pub bytes_received: u64,
}

/// Sync bundle — a zip archive containing the allowlisted files/dirs
pub struct SyncBundle;

impl SyncBundle {
    /// Create a sync bundle from a treeline directory
    pub fn create(treeline_dir: &Path) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            // Add individual files
            for filename in SYNC_FILES {
                let path = treeline_dir.join(filename);
                if path.exists() {
                    zip.start_file(*filename, options)?;
                    let mut f = fs::File::open(&path)?;
                    let mut contents = Vec::new();
                    f.read_to_end(&mut contents)?;
                    zip.write_all(&contents)?;
                }
            }

            // Add directories recursively
            for dir_name in SYNC_DIRS {
                let dir_path = treeline_dir.join(dir_name);
                if dir_path.is_dir() {
                    add_dir_to_zip(&mut zip, &dir_path, dir_name, options)?;
                }
            }

            zip.finish()?;
        }
        Ok(buf)
    }

    /// Extract a sync bundle into a treeline directory
    pub fn extract(data: &[u8], treeline_dir: &Path) -> Result<()> {
        let cursor = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(cursor)
            .context("Failed to read sync bundle")?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();

            // Validate the path is within our allowlist
            if !is_allowed_path(&name) {
                continue;
            }

            let target = treeline_dir.join(&name);

            // Create parent directories if needed
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }

            if file.is_dir() {
                fs::create_dir_all(&target)?;
            } else {
                let mut outfile = fs::File::create(&target)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }

        Ok(())
    }
}

/// Check if a path from a zip is within the sync allowlist
fn is_allowed_path(path: &str) -> bool {
    // Direct files
    for f in SYNC_FILES {
        if path == *f {
            return true;
        }
    }
    // Files within allowed directories
    for d in SYNC_DIRS {
        if path.starts_with(&format!("{}/", d)) || path == *d {
            return true;
        }
    }
    false
}

/// Recursively add a directory to a zip archive
fn add_dir_to_zip<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    dir_path: &Path,
    prefix: &str,
    options: SimpleFileOptions,
) -> Result<()> {
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        let name = format!(
            "{}/{}",
            prefix,
            entry.file_name().to_string_lossy()
        );

        if path.is_dir() {
            add_dir_to_zip(zip, &path, &name, options)?;
        } else {
            zip.start_file(&name, options)?;
            let mut f = fs::File::open(&path)?;
            let mut contents = Vec::new();
            f.read_to_end(&mut contents)?;
            zip.write_all(&contents)?;
        }
    }
    Ok(())
}

impl HubService {
    pub fn new(treeline_dir: PathBuf, db_filename: String) -> Self {
        Self {
            treeline_dir,
            db_filename,
        }
    }

    /// Get the path to the hub token file
    pub fn token_path(treeline_dir: &Path) -> PathBuf {
        treeline_dir.join("hub-token")
    }

    /// Load or generate the hub auth token
    pub fn load_or_create_token(treeline_dir: &Path) -> Result<String> {
        let token_path = Self::token_path(treeline_dir);

        if token_path.exists() {
            let token = fs::read_to_string(&token_path)
                .context("Failed to read hub token")?
                .trim()
                .to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }

        // Generate a new token
        let token = Self::generate_token();
        fs::write(&token_path, &token).context("Failed to write hub token")?;
        Ok(token)
    }

    /// Generate a random auth token
    fn generate_token() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        hex::encode(bytes)
    }

    /// Validate an auth token
    pub fn validate_token(treeline_dir: &Path, token: &str) -> Result<bool> {
        let expected = fs::read_to_string(Self::token_path(treeline_dir))
            .unwrap_or_default()
            .trim()
            .to_string();
        Ok(!expected.is_empty() && expected == token)
    }

    /// Accept a pushed sync bundle
    ///
    /// The bundle is treated as an opaque blob — the hub does not
    /// try to open or validate it.
    ///
    /// 1. Backs up the current database (if one exists)
    /// 2. Extracts the bundle into the treeline directory
    pub fn accept_push(&self, data: &[u8]) -> Result<PushResult> {
        let db_path = self.treeline_dir.join(&self.db_filename);
        let bytes_received = data.len() as u64;

        // Back up current database before replacing (if one exists)
        let backup_name = if db_path.exists() {
            let backup_service = BackupService::new(
                self.treeline_dir.clone(),
                self.db_filename.clone(),
            );
            match backup_service.create(Some(20)) {
                Ok(backup) => Some(backup.name),
                Err(e) => {
                    anyhow::bail!("Failed to create backup before accepting push: {}", e);
                }
            }
        } else {
            None
        };

        // Extract the sync bundle
        SyncBundle::extract(data, &self.treeline_dir)?;

        Ok(PushResult {
            backup_name,
            bytes_received,
        })
    }

    /// Get the current sync bundle for a pull
    pub fn get_bundle_for_pull(&self) -> Result<Vec<u8>> {
        let db_path = self.treeline_dir.join(&self.db_filename);

        if !db_path.exists() {
            anyhow::bail!("No database on hub yet. Push a database first.");
        }

        SyncBundle::create(&self.treeline_dir)
    }

    /// Check if the hub has a database file
    pub fn has_database(&self) -> bool {
        self.treeline_dir.join(&self.db_filename).exists()
    }
}
