//! Hub client - device-side push/pull/watch against a remote hub.
//!
//! `core/src/services/hub.rs` is server-side (accept_push, get_bundle_for_pull,
//! token issuance). This module is the *device-side* counterpart: the logic a
//! linked device runs to upload its DB, download the hub's DB, and continuously
//! reconcile the two via the watch loop.
//!
//! Both the CLI (`tl hub push|pull|watch`) and the desktop app (in-process
//! background watcher) consume this module. The CLI is a thin wrapper that
//! formats outcomes for stderr; the desktop forwards them to the frontend as
//! Tauri events.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use diffy_duck::{DatabaseConfig, Diff3ChangeOrigin, Diff3RowChange, DiffOptions, Merge3Strategy};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::TreelineContext;
use crate::config::HubConfig;
use crate::services::hub::{SyncBundle, compute_file_hash};

/// Outcome of a `push` call. The caller decides how to surface each variant
/// (CLI prints, desktop emits an event).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PushOutcome {
    /// Bundle accepted by the hub on first try.
    Pushed { bytes: u64, hash: Option<String> },
    /// Hub had diverged but the diff was non-conflicting; we merged locally
    /// and pushed the merged bundle.
    AutoMerged { bytes: u64, hash: Option<String> },
    /// Hub had diverged with conflicting rows. Caller must resolve via
    /// `--force` (push) or pull.
    Conflict {
        hub_hash: String,
        conflicts: Vec<ConflictDescription>,
    },
    /// Hub had diverged but we don't have the base snapshot needed for a
    /// three-way merge. Caller must `--force` or pull.
    NoBaseSnapshot { hub_hash: String },
    /// Local DB matches the base snapshot — nothing to send. Watch returns
    /// this when an mtime change touched the file but the bytes didn't move.
    NoChanges,
}

/// Outcome of a `pull` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullOutcome {
    pub bytes: u64,
    pub hash: Option<String>,
}

/// One conflicting change detected by the three-way diff. Caller-facing
/// summary; the underlying `diffy_duck` types stay private.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictDescription {
    pub table: String,
    pub kind: ConflictKind,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Both sides modified the same row, with overlapping columns.
    ModifiedSameColumns,
    /// Both sides added a row with the same primary key.
    BothAdded,
    /// One side deleted, the other modified.
    DeletedVsModified,
}

/// Tunables for the watch loop.
#[derive(Debug, Clone)]
pub struct WatchOptions {
    /// How long to wait after the last mtime change before pushing.
    pub debounce: Duration,
    /// How often to ask the hub for its current hash.
    pub poll: Duration,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_secs(5),
            poll: Duration::from_secs(15),
        }
    }
}

/// Events emitted by the watch loop. The observer receives these in order;
/// the CLI prints them, the desktop translates to Tauri events for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WatchEvent {
    Started { hub_url: String },
    LocalChangeDetected,
    Pushing,
    Pushed { bytes: u64 },
    AutoMerged { bytes: u64 },
    Pulling,
    Pulled { bytes: u64 },
    Conflict { hub_hash: String, conflicts: usize },
    NoBaseSnapshot { hub_hash: String },
    /// Push or poll failed — watch keeps running.
    Error { message: String },
    Stopped,
}

/// Receiver for watch events. Implementors decide how to surface them.
pub trait WatchObserver: Send {
    fn on_event(&mut self, event: WatchEvent);
}

/// Device-side hub client. Holds the treeline_dir + a reusable HTTP client.
pub struct HubClient {
    treeline_dir: PathBuf,
    http: reqwest::blocking::Client,
}

impl HubClient {
    pub fn new(treeline_dir: PathBuf) -> Self {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        Self { treeline_dir, http }
    }

    pub fn treeline_dir(&self) -> &Path {
        &self.treeline_dir
    }

    /// Push the local DB to the hub.
    ///
    /// Compacts + checkpoints the DB, builds a `SyncBundle`, and uploads with
    /// `base_hash` for conflict detection (omitted when `force=true`). On
    /// 409 Conflict, attempts a three-way merge against `.treeline.base.duckdb`
    /// and pushes the merged result if non-conflicting; otherwise returns
    /// `Conflict` with a structured summary.
    pub fn push(&self, ctx: &TreelineContext, force: bool) -> Result<PushOutcome> {
        let mut hub = HubConfig::load(&self.treeline_dir)?
            .ok_or_else(|| anyhow::anyhow!(
                "Not linked to a hub. Run 'tl hub link' first."
            ))?;

        // Compact + checkpoint so we ship the smallest, most-consistent file.
        ctx.compact_service.compact()?;
        ctx.repository.checkpoint()?;

        let bundle = SyncBundle::create(&self.treeline_dir)?;
        let size = bundle.len() as u64;

        let base_hash_param = if force { None } else { hub.base_hash.clone() };
        let push_url = build_push_url(&hub.url, base_hash_param.as_deref());

        let send = |access_token: &str, body: Vec<u8>| {
            self.http
                .post(&push_url)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("Content-Type", "application/octet-stream")
                .body(body)
                .timeout(Duration::from_secs(300))
                .send()
        };

        let mut resp = send(&hub.access_token, bundle.clone())
            .context("Failed to connect to hub")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh_access_token(&mut hub)?;
            resp = send(&hub.access_token, bundle.clone())
                .context("Failed to connect to hub")?;
        }

        let status = resp.status();
        let body: serde_json::Value = resp.json().context("Failed to parse hub response")?;

        if status == reqwest::StatusCode::CONFLICT {
            let hub_hash = body["hub_hash"].as_str().unwrap_or("unknown").to_string();
            return self.handle_conflict(ctx, &mut hub, hub_hash);
        }

        if !status.is_success() {
            let error = body["error"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("Push failed ({}): {}", status, error);
        }

        let new_hash = body["hash"].as_str().map(|s| s.to_string());
        hub.last_push = Some(chrono::Utc::now());
        hub.base_hash = new_hash.clone();
        hub.save(&self.treeline_dir)?;
        save_base_snapshot(&self.treeline_dir)?;

        Ok(PushOutcome::Pushed { bytes: size, hash: new_hash })
    }

    /// Pull the hub's DB to local. Backs up current local state first.
    pub fn pull(&self) -> Result<PullOutcome> {
        let mut hub = HubConfig::load(&self.treeline_dir)?
            .ok_or_else(|| anyhow::anyhow!(
                "Not linked to a hub. Run 'tl hub link' first."
            ))?;

        let pull_url = format!("{}/api/pull", hub.url);
        let send = |access_token: &str| {
            self.http
                .get(&pull_url)
                .header("Authorization", format!("Bearer {}", access_token))
                .timeout(Duration::from_secs(300))
                .send()
        };

        let mut resp = send(&hub.access_token).context("Failed to connect to hub")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh_access_token(&mut hub)?;
            resp = send(&hub.access_token).context("Failed to connect to hub")?;
        }

        if !resp.status().is_success() {
            let body = resp.text().unwrap_or_default();
            anyhow::bail!("Pull failed: {}", body);
        }

        let bytes = resp.bytes()?;
        let size = bytes.len() as u64;

        // Backup before overwrite.
        let db_path = self.treeline_dir.join("treeline.duckdb");
        if db_path.exists() {
            let backup = crate::services::BackupService::new(
                self.treeline_dir.clone(),
                "treeline.duckdb".to_string(),
            );
            backup.create(Some(20))?;
        }

        SyncBundle::extract(&bytes, &self.treeline_dir)?;

        let new_hash = if db_path.exists() {
            Some(compute_file_hash(&db_path)?)
        } else {
            None
        };

        save_base_snapshot(&self.treeline_dir)?;

        let mut hub = HubConfig::load(&self.treeline_dir)?.unwrap();
        hub.last_pull = Some(chrono::Utc::now());
        hub.base_hash = new_hash.clone();
        hub.save(&self.treeline_dir)?;

        Ok(PullOutcome { bytes: size, hash: new_hash })
    }

    /// Ask the hub for its current bundle hash. Used by the watch loop to
    /// decide whether a pull is needed.
    pub fn poll_hub_hash(&self) -> Result<Option<String>> {
        let hub = match HubConfig::load(&self.treeline_dir)? {
            Some(h) => h,
            None => return Ok(None),
        };

        let resp = self.http
            .get(format!("{}/api/hash", hub.url))
            .header("Authorization", format!("Bearer {}", hub.access_token))
            .timeout(Duration::from_secs(5))
            .send();

        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            _ => return Ok(None),
        };

        let body: serde_json::Value = match resp.json() {
            Ok(b) => b,
            Err(_) => return Ok(None),
        };

        Ok(body["hash"].as_str().map(|s| s.to_string()))
    }

    /// Run the watch loop until `stop` flips to true.
    ///
    /// Acquires an exclusive lock on `~/.treeline/hub.lock` so a second
    /// watcher (e.g. CLI started while desktop is running) can't fight over
    /// the same DB. Returns `Err` immediately if the lock is held; caller
    /// decides whether to retry.
    pub fn watch(
        &self,
        ctx: &TreelineContext,
        opts: WatchOptions,
        observer: &mut dyn WatchObserver,
        stop: Arc<AtomicBool>,
    ) -> Result<()> {
        let hub = HubConfig::load(&self.treeline_dir)?
            .ok_or_else(|| anyhow::anyhow!(
                "Not linked to a hub. Run 'tl hub link' first."
            ))?;

        let _lock = WatchLock::acquire(&self.treeline_dir)
            .context("Another watcher is already running for this directory")?;

        observer.on_event(WatchEvent::Started { hub_url: hub.url.clone() });

        let db_path = self.treeline_dir.join("treeline.duckdb");
        let mut last_mtime = db_path.metadata().and_then(|m| m.modified()).ok();
        let mut last_poll = Instant::now();

        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));

            let current_mtime = db_path.metadata().and_then(|m| m.modified()).ok();
            if current_mtime != last_mtime {
                last_mtime = current_mtime;
                observer.on_event(WatchEvent::LocalChangeDetected);

                // Wait for mtime to stabilize before pushing.
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(opts.debounce);
                    let new_mtime = db_path.metadata().and_then(|m| m.modified()).ok();
                    if new_mtime == last_mtime {
                        break;
                    }
                    last_mtime = new_mtime;
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                // Skip the push if the bytes haven't actually moved relative
                // to our base — mtime can bump from idle DuckDB activity.
                let hub_cfg = HubConfig::load(&self.treeline_dir)?.unwrap();
                if let Some(ref base_hash) = hub_cfg.base_hash {
                    if let Ok(current_hash) = compute_file_hash(&db_path) {
                        if &current_hash == base_hash {
                            last_mtime = db_path.metadata().and_then(|m| m.modified()).ok();
                            continue;
                        }
                    }
                }

                observer.on_event(WatchEvent::Pushing);
                match self.push(ctx, false) {
                    Ok(PushOutcome::Pushed { bytes, .. }) => {
                        observer.on_event(WatchEvent::Pushed { bytes });
                    }
                    Ok(PushOutcome::AutoMerged { bytes, .. }) => {
                        observer.on_event(WatchEvent::AutoMerged { bytes });
                    }
                    Ok(PushOutcome::Conflict { hub_hash, conflicts }) => {
                        observer.on_event(WatchEvent::Conflict {
                            hub_hash,
                            conflicts: conflicts.len(),
                        });
                    }
                    Ok(PushOutcome::NoBaseSnapshot { hub_hash }) => {
                        observer.on_event(WatchEvent::NoBaseSnapshot { hub_hash });
                    }
                    Ok(PushOutcome::NoChanges) => {}
                    Err(e) => {
                        observer.on_event(WatchEvent::Error { message: e.to_string() });
                    }
                }
                last_poll = Instant::now();
                last_mtime = db_path.metadata().and_then(|m| m.modified()).ok();
                continue;
            }

            if last_poll.elapsed() >= opts.poll {
                last_poll = Instant::now();

                let hub_hash = match self.poll_hub_hash() {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                let hub_cfg = match HubConfig::load(&self.treeline_dir)? {
                    Some(h) => h,
                    None => continue,
                };

                let needs_pull = match (hub_hash.as_deref(), hub_cfg.base_hash.as_deref()) {
                    (Some(hub), Some(base)) => hub != base,
                    (Some(_), None) => true,
                    _ => false,
                };
                if !needs_pull {
                    continue;
                }

                let has_local_changes = match hub_cfg.base_hash.as_deref() {
                    Some(base) => compute_file_hash(&db_path)
                        .map(|h| h.as_str() != base)
                        .unwrap_or(false),
                    None => false,
                };

                if has_local_changes {
                    // Both sides moved — push triggers the three-way merge.
                    observer.on_event(WatchEvent::Pushing);
                    match self.push(ctx, false) {
                        Ok(PushOutcome::Pushed { bytes, .. }) => {
                            observer.on_event(WatchEvent::Pushed { bytes });
                        }
                        Ok(PushOutcome::AutoMerged { bytes, .. }) => {
                            observer.on_event(WatchEvent::AutoMerged { bytes });
                        }
                        Ok(PushOutcome::Conflict { hub_hash, conflicts }) => {
                            observer.on_event(WatchEvent::Conflict {
                                hub_hash,
                                conflicts: conflicts.len(),
                            });
                        }
                        Ok(PushOutcome::NoBaseSnapshot { hub_hash }) => {
                            observer.on_event(WatchEvent::NoBaseSnapshot { hub_hash });
                        }
                        Ok(PushOutcome::NoChanges) => {}
                        Err(e) => {
                            observer.on_event(WatchEvent::Error { message: e.to_string() });
                        }
                    }
                } else {
                    observer.on_event(WatchEvent::Pulling);
                    match self.pull() {
                        Ok(out) => observer.on_event(WatchEvent::Pulled { bytes: out.bytes }),
                        Err(e) => observer.on_event(WatchEvent::Error { message: e.to_string() }),
                    }
                }

                last_mtime = db_path.metadata().and_then(|m| m.modified()).ok();
            }
        }

        observer.on_event(WatchEvent::Stopped);
        Ok(())
    }

    /// Refresh `hub.access_token` using its `refresh_token` and persist.
    fn refresh_access_token(&self, hub: &mut HubConfig) -> Result<()> {
        let resp = self.http
            .post(format!("{}/token", hub.url))
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", hub.refresh_token.as_str()),
            ])
            .timeout(Duration::from_secs(15))
            .send()
            .context("Failed to refresh access token")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            anyhow::bail!(
                "Token refresh failed ({}): {}. Re-link this device with `tl hub link`.",
                status,
                body
            );
        }

        let body: serde_json::Value = resp.json().context("Failed to parse refresh response")?;
        let new_access = body["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Refresh response missing access_token"))?
            .to_string();
        let new_refresh = body["refresh_token"].as_str().map(|s| s.to_string());

        hub.access_token = new_access;
        if let Some(r) = new_refresh {
            hub.refresh_token = r;
        }
        hub.save(&self.treeline_dir)?;
        Ok(())
    }

    /// Handle a 409 from the hub: download hub bundle, three-way diff,
    /// auto-merge if non-conflicting, otherwise return structured conflicts.
    fn handle_conflict(
        &self,
        ctx: &TreelineContext,
        hub: &mut HubConfig,
        hub_hash: String,
    ) -> Result<PushOutcome> {
        let base_db_path = self.treeline_dir.join(".treeline.base.duckdb");
        if !base_db_path.exists() {
            return Ok(PushOutcome::NoBaseSnapshot { hub_hash });
        }

        // Download the hub's current bundle into a temp dir.
        let pull_resp = self.http
            .get(format!("{}/api/pull", hub.url))
            .header("Authorization", format!("Bearer {}", hub.access_token))
            .timeout(Duration::from_secs(300))
            .send()
            .context("Failed to download hub bundle for merge")?;

        if !pull_resp.status().is_success() {
            anyhow::bail!("Failed to download hub bundle: {}", pull_resp.status());
        }
        let hub_bundle = pull_resp.bytes()?;
        let hub_temp = tempfile::TempDir::new().context("Failed to create temp dir")?;
        SyncBundle::extract(&hub_bundle, hub_temp.path())?;

        let hub_db_path = hub_temp.path().join("treeline.duckdb");
        if !hub_db_path.exists() {
            anyhow::bail!("Hub bundle does not contain a database");
        }

        let local_db_path = self.treeline_dir.join("treeline.duckdb");
        let encryption_key = ctx.repository.encryption_key().map(|s| s.to_string());

        let mut ancestor_config = DatabaseConfig::new(base_db_path.to_string_lossy());
        let mut local_config = DatabaseConfig::new(local_db_path.to_string_lossy());
        let mut hub_config = DatabaseConfig::new(hub_db_path.to_string_lossy());
        if let Some(ref key) = encryption_key {
            ancestor_config = ancestor_config.with_key(key);
            local_config = local_config.with_key(key);
            hub_config = hub_config.with_key(key);
        }

        let diff3_report = diffy_duck::diff3(
            &ancestor_config, &local_config, &hub_config, &DiffOptions::default(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to diff databases: {}", e))?;

        let total_changes = diff3_report.summary.total_non_conflicting
            + diff3_report.summary.total_conflicts;
        let has_new_tables = !diff3_report.a_only_tables.is_empty()
            || !diff3_report.b_only_tables.is_empty();

        if total_changes == 0 && !has_new_tables {
            return Ok(PushOutcome::NoChanges);
        }

        if diff3_report.summary.total_conflicts > 0 {
            let conflicts = collect_conflicts(&diff3_report);
            return Ok(PushOutcome::Conflict { hub_hash, conflicts });
        }

        // No conflicts — auto-merge against a copy of the base, push the result.
        let merge_temp = tempfile::TempDir::new()?;
        let merge_db_path = merge_temp.path().join("merged.duckdb");
        std::fs::copy(&base_db_path, &merge_db_path)?;

        let mut merge_ancestor = DatabaseConfig::new(merge_db_path.to_string_lossy());
        if let Some(ref key) = encryption_key {
            merge_ancestor = merge_ancestor.with_key(key);
        }

        diffy_duck::merge3(
            &merge_ancestor, &local_config, &hub_config, &Merge3Strategy::FailOnConflict,
        )
        .map_err(|e| anyhow::anyhow!("Failed to merge: {}", e))?;

        std::fs::copy(&merge_db_path, &local_db_path)?;

        let merged_bundle = SyncBundle::create(&self.treeline_dir)?;
        let size = merged_bundle.len() as u64;

        let resp = self.http
            .post(format!("{}/api/push", hub.url))
            .header("Authorization", format!("Bearer {}", hub.access_token))
            .header("Content-Type", "application/octet-stream")
            .body(merged_bundle)
            .timeout(Duration::from_secs(300))
            .send()
            .context("Failed to push merged database")?;

        if !resp.status().is_success() {
            let body = resp.text().unwrap_or_default();
            anyhow::bail!("Push after merge failed: {}", body);
        }

        let body: serde_json::Value = resp.json().unwrap_or_default();
        let new_hash = body["hash"].as_str().map(|s| s.to_string());
        let mut hub = HubConfig::load(&self.treeline_dir)?.unwrap();
        hub.last_push = Some(chrono::Utc::now());
        hub.base_hash = new_hash.clone();
        hub.save(&self.treeline_dir)?;
        save_base_snapshot(&self.treeline_dir)?;

        Ok(PushOutcome::AutoMerged { bytes: size, hash: new_hash })
    }
}

fn build_push_url(base: &str, base_hash: Option<&str>) -> String {
    match base_hash {
        Some(h) => format!("{}/api/push?base_hash={}", base, h),
        None => format!("{}/api/push", base),
    }
}

fn collect_conflicts(report: &diffy_duck::Diff3Report) -> Vec<ConflictDescription> {
    let mut out = Vec::new();
    for table_diff in &report.tables {
        for change in &table_diff.changes {
            match change {
                Diff3RowChange::Modified { column_changes, .. } => {
                    let cols: Vec<&str> = column_changes
                        .iter()
                        .filter(|c| c.origin == Diff3ChangeOrigin::Conflict)
                        .map(|c| c.column.as_str())
                        .collect();
                    if cols.is_empty() {
                        continue;
                    }
                    out.push(ConflictDescription {
                        table: table_diff.table.to_string(),
                        kind: ConflictKind::ModifiedSameColumns,
                        detail: format!("columns: {}", cols.join(", ")),
                    });
                }
                Diff3RowChange::Added { origin: Diff3ChangeOrigin::Conflict, .. } => {
                    out.push(ConflictDescription {
                        table: table_diff.table.to_string(),
                        kind: ConflictKind::BothAdded,
                        detail: "both sides added a row with the same key".to_string(),
                    });
                }
                Diff3RowChange::Removed { origin: Diff3ChangeOrigin::Conflict, .. } => {
                    out.push(ConflictDescription {
                        table: table_diff.table.to_string(),
                        kind: ConflictKind::DeletedVsModified,
                        detail: "one side deleted, other modified".to_string(),
                    });
                }
                _ => {}
            }
        }
    }
    out
}

fn save_base_snapshot(treeline_dir: &Path) -> Result<()> {
    let db_path = treeline_dir.join("treeline.duckdb");
    let base_path = treeline_dir.join(".treeline.base.duckdb");
    if db_path.exists() {
        std::fs::copy(&db_path, &base_path)
            .context("Failed to save base snapshot")?;
    }
    Ok(())
}

/// Cross-process exclusive lock held for the duration of a watch loop.
///
/// Two watchers on the same `treeline_dir` (e.g. desktop in-process + a CLI
/// `tl hub watch` started from a terminal) would race each other on push
/// and on `hub.json` writes. The lock makes second-comer fail fast.
struct WatchLock {
    _file: std::fs::File,
}

impl WatchLock {
    fn acquire(treeline_dir: &Path) -> Result<Self> {
        let path = treeline_dir.join("hub.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("Failed to open {}", path.display()))?;
        file.try_lock_exclusive()
            .map_err(|e| anyhow::anyhow!("Failed to acquire watch lock: {}", e))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_lock_blocks_second_acquire() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = WatchLock::acquire(dir.path()).expect("first lock");
        let second = WatchLock::acquire(dir.path());
        assert!(second.is_err(), "second concurrent lock must fail");
        drop(first);
        let third = WatchLock::acquire(dir.path()).expect("third lock after release");
        drop(third);
    }

    #[test]
    fn watch_lock_creates_lock_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let lock_path = dir.path().join("hub.lock");
        assert!(!lock_path.exists());
        let lock = WatchLock::acquire(dir.path()).expect("lock");
        assert!(lock_path.exists(), "hub.lock created on acquire");
        drop(lock);
    }

    #[test]
    fn build_push_url_with_base_hash() {
        assert_eq!(
            build_push_url("http://h", Some("abc")),
            "http://h/api/push?base_hash=abc"
        );
        assert_eq!(build_push_url("http://h", None), "http://h/api/push");
    }
}
