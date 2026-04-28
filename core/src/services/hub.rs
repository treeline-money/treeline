//! Hub service - manages database sync for hub operations
//!
//! Handles accepting pushed databases, serving pulls, and
//! token-based authentication for the hub server.
//!
//! The hub treats sync bundles as opaque blobs — it doesn't
//! need to open or understand the contents to accept pushes
//! or serve pulls.
//!
//! Conflict detection uses SHA-256 hashes of the database file.
//! The hub stores the current hash, and clients track which hash
//! they're based on. Pushes are rejected if the hub has changed
//! since the client last synced.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

/// Result of attempting a push
#[derive(Debug, Serialize)]
#[serde(tag = "status")]
pub enum PushOutcome {
    /// Push accepted, no conflict
    #[serde(rename = "ok")]
    Accepted {
        backup_name: Option<String>,
        bytes_received: u64,
        new_hash: String,
    },
    /// Push rejected — hub has changed since client last synced
    #[serde(rename = "conflict")]
    Conflict {
        hub_hash: String,
    },
}

/// Hub sync metadata — stored in hub-sync.json on the hub
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HubSyncMeta {
    /// SHA-256 hash of the current database file
    pub current_hash: Option<String>,
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

    /// Extract a sync bundle into a treeline directory.
    ///
    /// Acquires the DuckDB filesystem lock (`treeline.duckdb.lock`) for the
    /// duration of the extraction. This prevents `treeline.duckdb` from being
    /// overwritten while another process holds a DuckDB operation against it
    /// (e.g. the desktop app mid-query, or `tl hub watch` running alongside
    /// an open app). The lock file and naming convention match
    /// `DuckDbRepository::acquire_lock`, so extract serializes with all
    /// DuckDB operations on the same directory.
    pub fn extract(data: &[u8], treeline_dir: &Path) -> Result<()> {
        fs::create_dir_all(treeline_dir)?;
        let _lock = acquire_db_lock(treeline_dir)?;

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
        // _lock drops here — releases the file lock.
    }
}

/// Acquire the same exclusive file lock that `DuckDbRepository` takes per
/// operation. Returns the held `File` — dropping it releases the lock.
fn acquire_db_lock(treeline_dir: &Path) -> Result<fs::File> {
    use fs2::FileExt;
    let lock_path = treeline_dir.join("treeline.duckdb.lock");
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("Failed to open {}", lock_path.display()))?;
    lock_file
        .lock_exclusive()
        .context("Failed to acquire treeline.duckdb.lock for bundle extract")?;
    Ok(lock_file)
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

    /// Load or generate the hub auth token.
    ///
    /// Lookup order:
    /// 1. `TL_HUB_TOKEN` env var — provisioners (e.g. Treeline Cloud) set this
    ///    so they know the value of the master token without having to read
    ///    it back off the machine. The value is persisted to disk so subsequent
    ///    boots without the env var continue to work.
    /// 2. The token file at `treeline_dir/hub-token`.
    /// 3. A freshly generated token (also persisted).
    pub fn load_or_create_token(treeline_dir: &Path) -> Result<String> {
        let token_path = Self::token_path(treeline_dir);

        if let Ok(env_token) = std::env::var("TL_HUB_TOKEN") {
            let env_token = env_token.trim().to_string();
            if !env_token.is_empty() {
                fs::write(&token_path, &env_token)
                    .context("Failed to persist TL_HUB_TOKEN to hub token file")?;
                return Ok(env_token);
            }
        }

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

    /// Accept a pushed sync bundle with conflict detection.
    ///
    /// If `base_hash` is provided, the hub checks whether its current
    /// database hash matches. If not, the push is rejected with
    /// `PushOutcome::Conflict` so the client can diff and merge.
    ///
    /// If `base_hash` is None (first push, or force push), the push
    /// is accepted unconditionally.
    pub fn accept_push(&self, data: &[u8], base_hash: Option<&str>) -> Result<PushOutcome> {
        let db_path = self.treeline_dir.join(&self.db_filename);

        // Check for conflicts
        if let Some(base_hash) = base_hash {
            let hub_hash = self.current_hash()?;
            if let Some(ref hub_hash) = hub_hash {
                if hub_hash != base_hash {
                    return Ok(PushOutcome::Conflict {
                        hub_hash: hub_hash.clone(),
                    });
                }
            }
        }

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

        // Update the hash
        let new_hash = self.compute_and_store_hash()?;

        Ok(PushOutcome::Accepted {
            backup_name,
            bytes_received,
            new_hash,
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

    /// Get the current database hash (from stored metadata)
    pub fn current_hash(&self) -> Result<Option<String>> {
        let meta = self.load_sync_meta()?;
        Ok(meta.current_hash)
    }

    /// Compute the SHA-256 hash of the database file and store it
    pub fn compute_and_store_hash(&self) -> Result<String> {
        let db_path = self.treeline_dir.join(&self.db_filename);
        let hash = compute_file_hash(&db_path)?;

        let mut meta = self.load_sync_meta()?;
        meta.current_hash = Some(hash.clone());
        self.save_sync_meta(&meta)?;

        Ok(hash)
    }

    fn sync_meta_path(&self) -> PathBuf {
        self.treeline_dir.join("hub-sync.json")
    }

    fn load_sync_meta(&self) -> Result<HubSyncMeta> {
        let path = self.sync_meta_path();
        if !path.exists() {
            return Ok(HubSyncMeta::default());
        }
        let content = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content).unwrap_or_default())
    }

    fn save_sync_meta(&self, meta: &HubSyncMeta) -> Result<()> {
        let path = self.sync_meta_path();
        let content = serde_json::to_string_pretty(meta)?;
        fs::write(&path, content)?;
        Ok(())
    }
}

/// Compute SHA-256 hash of a file
pub fn compute_file_hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open file for hashing: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
