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

/// Files the producer includes in sync bundles.
/// These are the files that define a Treeline instance's state. The receiver
/// does NOT enforce a positive allowlist — see `is_safe_path` /
/// `DEVICE_LOCAL_DENYLIST` for what the receiver rejects.
const SYNC_FILES: &[&str] = &["treeline.duckdb", "encryption.json", "settings.json"];

/// Directories the producer includes in sync bundles (recursive).
const SYNC_DIRS: &[&str] = &["skills", "plugins"];

/// `settings.json` paths whose values sync across devices ("bundle wins" on
/// receive). Anything not under one of these prefixes is treated as
/// per-device: kept local if already set, bootstrapped from the bundle only
/// when the local key is missing.
///
/// **When adding a new shared setting**, add its path here. Otherwise the
/// new field defaults to per-device behavior. See `SettingsFile` in
/// `core/src/config.rs`.
const SHARED_SETTING_PATHS: &[&str] = &[
    "app.currency",
    "app.experimentalFeatures",
    "app.lastSyncDate",
    "plugins",
    "disabledPlugins",
    "importProfiles",
];

/// Files that must never be overwritten by a bundle, regardless of what the
/// producer chose to include. These are device-local: hub credentials, the
/// hub's own sync metadata, the DuckDB lock file, the local 3-way-merge
/// snapshot, and per-device logs. Defense in depth — a correct producer
/// won't include any of these, but the receiver enforces it anyway.
const DEVICE_LOCAL_DENYLIST: &[&str] = &[
    "hub.json",
    "hub-token",
    "hub-sync.json",
    "treeline.duckdb.lock",
    ".treeline.base.duckdb",
    "logs.duckdb",
];

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
    Conflict { hub_hash: String },
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
    /// Create a sync bundle from a treeline directory.
    ///
    /// Holds a shared lock on `treeline.duckdb.lock` while reading, so a
    /// concurrent DuckDB write (which takes the lock exclusively per-op)
    /// can't checkpoint underneath us and produce a torn database inside
    /// the bundle.
    pub fn create(treeline_dir: &Path) -> Result<Vec<u8>> {
        let _lock = acquire_db_lock_shared(treeline_dir)?;
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

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

    /// Read the raw `treeline.duckdb` bytes out of a bundle without
    /// extracting it. Returns `None` if the bundle carries no database.
    /// Reading the full entry also verifies it against the zip's CRC-32,
    /// so a torn upload fails here rather than being silently accepted.
    pub fn db_entry_bytes(data: &[u8]) -> Result<Option<Vec<u8>>> {
        let cursor = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(cursor).context("Failed to read sync bundle")?;
        let result = match archive.by_name("treeline.duckdb") {
            Ok(mut entry) => {
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .context("Failed to read database entry from sync bundle")?;
                Some(buf)
            }
            Err(zip::result::ZipError::FileNotFound) => None,
            Err(e) => return Err(e.into()),
        };
        Ok(result)
    }

    /// Extract a sync bundle into a treeline directory.
    ///
    /// **Receive semantics (asymmetric vs. the producer):**
    ///
    /// - **Top-level files**: the receiver writes whatever the bundle
    ///   contains; it never deletes a top-level file the bundle omits. Path
    ///   safety + `DEVICE_LOCAL_DENYLIST` are enforced.
    /// - **`settings.json`**: special-cased. Shared paths
    ///   (`SHARED_SETTING_PATHS`) overwrite the local value; per-device paths
    ///   are kept locally and only bootstrapped from the bundle when the
    ///   local key is missing. If `settings.json` doesn't exist locally, the
    ///   bundle's bytes are written verbatim (first-pull bootstrap).
    /// - **`skills/`**: mirror semantics. If the bundle contains any
    ///   `skills/` entries, the local `skills/` directory is cleared before
    ///   extraction. Deletes propagate.
    /// - **`plugins/<id>/`**: per-plugin replacement. For each plugin id
    ///   present in the bundle, the local `plugins/<id>/` directory is
    ///   cleared before extraction (so upgrades don't leak old files). Local
    ///   plugin directories that aren't in the bundle are left alone —
    ///   uninstalls do NOT propagate. The reasoning: plugins are software
    ///   that may legitimately differ across devices (one spouse uses Goals,
    ///   the other doesn't), unlike skills which are user-authored content
    ///   where deletes are deliberate.
    ///
    /// Acquires the DuckDB filesystem lock (`treeline.duckdb.lock`) for the
    /// duration of the extraction. This prevents `treeline.duckdb` from
    /// being overwritten while another process holds a DuckDB operation
    /// against it (e.g. the desktop app mid-query, or `tl hub watch` running
    /// alongside an open app).
    pub fn extract(data: &[u8], treeline_dir: &Path) -> Result<()> {
        fs::create_dir_all(treeline_dir)?;
        let _lock = acquire_db_lock(treeline_dir)?;

        // Sweep any orphan `.incoming` files left by a prior crashed extract.
        // Any *.incoming in treeline_dir is ours — produced by `atomic_write`
        // and only deleted on the rename step. If we see one, the previous
        // extract didn't reach commit; the live file is the old, intact copy
        // and the staging file should be discarded.
        cleanup_orphan_incoming(treeline_dir);

        let cursor = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(cursor).context("Failed to read sync bundle")?;

        // First pass: scan the bundle to figure out which directories to
        // clear before writing entries. We need this up front so that
        // pre-existing local files don't survive a per-plugin replace or a
        // skills mirror.
        let mut bundle_has_skills = false;
        let mut bundle_plugin_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for i in 0..archive.len() {
            let entry = archive.by_index(i)?;
            let name = entry.name();
            if !is_safe_path(name) || is_denylisted(name) {
                continue;
            }
            if let Some(rest) = name.strip_prefix("skills/") {
                if !rest.is_empty() {
                    bundle_has_skills = true;
                }
            }
            if let Some(rest) = name.strip_prefix("plugins/") {
                if let Some(slash) = rest.find('/') {
                    let plugin_id = &rest[..slash];
                    if !plugin_id.is_empty() {
                        bundle_plugin_ids.insert(plugin_id.to_string());
                    }
                }
            }
        }

        // Pre-extract cleanup: skills mirror, per-plugin replace.
        if bundle_has_skills {
            let skills_dir = treeline_dir.join("skills");
            if skills_dir.exists() {
                fs::remove_dir_all(&skills_dir)
                    .with_context(|| format!("Failed to clear {}", skills_dir.display()))?;
            }
        }
        for plugin_id in &bundle_plugin_ids {
            let plugin_dir = treeline_dir.join("plugins").join(plugin_id);
            if plugin_dir.exists() {
                fs::remove_dir_all(&plugin_dir)
                    .with_context(|| format!("Failed to clear {}", plugin_dir.display()))?;
            }
        }

        // Second pass: write entries. Defer settings.json — accumulate the
        // bundle's bytes and apply the merge at the end. Top-level files
        // (treeline.duckdb, encryption.json, …) are written atomically:
        // a crash mid-write leaves the live file intact and an orphan
        // `.incoming` for the next extract to sweep up.
        let mut bundled_settings: Option<Vec<u8>> = None;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();

            if !is_safe_path(&name) || is_denylisted(&name) {
                continue;
            }

            if name == "settings.json" {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                bundled_settings = Some(buf);
                continue;
            }

            let target = treeline_dir.join(&name);
            let is_top_level = !name.contains('/');
            if file.is_dir() {
                fs::create_dir_all(&target)?;
            } else if is_top_level {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                atomic_write(&target, &buf)?;
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut outfile = fs::File::create(&target)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }

        if let Some(bytes) = bundled_settings {
            merge_settings_into_local(&bytes, treeline_dir)?;
        }

        Ok(())
        // _lock drops here — releases the file lock.
    }
}

/// Constant-time string equality for credential checks. The xor-fold
/// touches every byte regardless of where the first mismatch is, so timing
/// doesn't leak how much of a guessed token was correct.
fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn open_db_lock_file(treeline_dir: &Path) -> Result<fs::File> {
    let lock_path = treeline_dir.join("treeline.duckdb.lock");
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("Failed to open {}", lock_path.display()))
}

/// Acquire the same exclusive file lock that `DuckDbRepository` takes per
/// operation. Returns the held `File` — dropping it releases the lock.
pub(crate) fn acquire_db_lock(treeline_dir: &Path) -> Result<fs::File> {
    use fs2::FileExt;
    let lock_file = open_db_lock_file(treeline_dir)?;
    lock_file
        .lock_exclusive()
        .context("Failed to acquire treeline.duckdb.lock")?;
    Ok(lock_file)
}

/// Shared variant — blocks while a DuckDB op holds the lock exclusively,
/// but doesn't exclude other readers.
fn acquire_db_lock_shared(treeline_dir: &Path) -> Result<fs::File> {
    let lock_file = open_db_lock_file(treeline_dir)?;
    fs2::FileExt::lock_shared(&lock_file)
        .context("Failed to acquire treeline.duckdb.lock for bundle create")?;
    Ok(lock_file)
}

/// Build the staging path used for atomic writes (`<target>.incoming`).
/// Same filesystem as the target so the final `rename` is atomic on
/// Linux/macOS/Windows.
fn incoming_path(target: &Path) -> PathBuf {
    let mut s = target.as_os_str().to_owned();
    s.push(".incoming");
    PathBuf::from(s)
}

/// Write `contents` to `target` atomically: stage to `<target>.incoming`,
/// fsync, then rename. `std::fs::rename` is an atomic file replacement on
/// all three platforms when source and destination are on the same
/// filesystem (we always stage inside `treeline_dir`).
///
/// On crash before the rename, the live file at `target` is untouched.
/// The orphan staging file is swept up by `cleanup_orphan_incoming` at
/// the start of the next extract.
pub(crate) fn atomic_write(target: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let staging = incoming_path(target);
    {
        let mut f = fs::File::create(&staging)
            .with_context(|| format!("Failed to create staging file {}", staging.display()))?;
        f.write_all(contents)
            .with_context(|| format!("Failed to write staging file {}", staging.display()))?;
        f.sync_all()
            .with_context(|| format!("Failed to fsync staging file {}", staging.display()))?;
    }
    fs::rename(&staging, target).with_context(|| {
        format!(
            "Failed to atomically rename {} → {}",
            staging.display(),
            target.display()
        )
    })?;
    Ok(())
}

/// Remove any `*.incoming` entries directly under `treeline_dir`. These
/// only come from a prior crashed `atomic_write`. Best-effort: errors are
/// swallowed because we'd rather proceed with the new extract than abort
/// over a stale staging file.
fn cleanup_orphan_incoming(treeline_dir: &Path) {
    let Ok(entries) = fs::read_dir(treeline_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.ends_with(".incoming") {
            continue;
        }
        let path = entry.path();
        let _ = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
    }
}

/// Reject paths that would escape the destination directory (zip slip) or
/// that are absolute. The receiver trusts the producer's contents but never
/// the structure of the paths inside the zip.
fn is_safe_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    if std::path::Path::new(path).is_absolute() {
        return false;
    }
    // Reject `..` components in either separator. Zip entries are usually
    // forward-slash, but some producers emit backslashes on Windows.
    for component in path.split(|c| c == '/' || c == '\\') {
        if component == ".." {
            return false;
        }
    }
    true
}

/// Reject paths that point at device-local files (hub credentials, sync
/// metadata, locks, logs). Defense in depth — a correct producer never
/// includes any of these.
fn is_denylisted(path: &str) -> bool {
    DEVICE_LOCAL_DENYLIST.iter().any(|d| path == *d)
}

/// Apply a `settings.json` from a bundle onto the local one.
///
/// - First-pull bootstrap: if the local file doesn't exist, write the
///   bundle's bytes verbatim.
/// - Otherwise: walk the bundle's JSON tree leaf by leaf. For each leaf,
///   "shared" paths (under `SHARED_SETTING_PATHS`) overwrite the local
///   value; "per-device" paths bootstrap into local only when local doesn't
///   already have the key.
fn merge_settings_into_local(bundle_bytes: &[u8], treeline_dir: &Path) -> Result<()> {
    let local_path = treeline_dir.join("settings.json");

    if !local_path.exists() {
        atomic_write(&local_path, bundle_bytes)
            .context("Failed to bootstrap settings.json from bundle")?;
        return Ok(());
    }

    let bundle_json: serde_json::Value = serde_json::from_slice(bundle_bytes)
        .context("settings.json in bundle is not valid JSON")?;

    let local_text = fs::read_to_string(&local_path)?;
    let mut local_json: serde_json::Value = serde_json::from_str(&local_text)
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    apply_bundle_to_local(&bundle_json, &mut local_json, "");

    let merged = serde_json::to_string_pretty(&local_json)?;
    atomic_write(&local_path, merged.as_bytes()).context("Failed to write merged settings.json")?;
    Ok(())
}

/// Recursively walk `bundle` and apply leaves to `local`. `prefix` is the
/// current dotted path from the root of settings.json (e.g.,
/// `"app.experimentalFeatures"`). Both arguments must be JSON objects at
/// the root for any work to happen.
fn apply_bundle_to_local(bundle: &serde_json::Value, local: &mut serde_json::Value, prefix: &str) {
    let (Some(bundle_obj), Some(local_obj)) = (bundle.as_object(), local.as_object_mut()) else {
        return;
    };

    for (k, v) in bundle_obj {
        let path = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{}.{}", prefix, k)
        };

        if v.is_object() {
            // Descend. Ensure local has an object at this key first.
            if !local_obj.get(k).map(|x| x.is_object()).unwrap_or(false) {
                local_obj.insert(k.clone(), serde_json::Value::Object(serde_json::Map::new()));
            }
            let local_child = local_obj.get_mut(k).expect("just inserted");
            apply_bundle_to_local(v, local_child, &path);
        } else if path_under_shared(&path) {
            // Shared leaf — bundle wins.
            local_obj.insert(k.clone(), v.clone());
        } else if !local_obj.contains_key(k) {
            // Per-device leaf — bootstrap only when local has no value.
            local_obj.insert(k.clone(), v.clone());
        }
    }
}

fn path_under_shared(path: &str) -> bool {
    SHARED_SETTING_PATHS
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{}.", p)))
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
        let name = format!("{}/{}", prefix, entry.file_name().to_string_lossy());

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
        Ok(!expected.is_empty() && ct_eq(&expected, token))
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

        // Validate before touching anything. Reading the entry checks it
        // against the zip CRC (catches torn uploads); the magic check
        // catches a well-formed zip wrapping a non-DuckDB file. Works for
        // encrypted databases too — DuckDB keeps the main header magic in
        // plaintext. The hub can't open the DB to validate deeper (it may
        // not have the encryption key), so this is as far as it goes.
        if let Some(db_bytes) = SyncBundle::db_entry_bytes(data)? {
            if !looks_like_duckdb(&db_bytes) {
                anyhow::bail!("Pushed bundle's database is not a valid DuckDB file");
            }
        }

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
            let backup_service =
                BackupService::new(self.treeline_dir.clone(), self.db_filename.clone());
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

/// DuckDB main-header check: bytes 8..12 of the file are the magic "DUCK"
/// (preceded by an 8-byte checksum). Present for both plain and encrypted
/// databases — encryption starts at the block level, not the main header.
fn looks_like_duckdb(bytes: &[u8]) -> bool {
    bytes.len() > 12 && &bytes[8..12] == b"DUCK"
}

/// Compute SHA-256 hash of in-memory bytes. Matches `compute_file_hash` for
/// the same content — used to derive the hub's file hash from bundle entry
/// bytes without writing them to disk first.
pub fn compute_bytes_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
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
