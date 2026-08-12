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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use diffy_duck::{DatabaseConfig, Diff3ChangeOrigin, Diff3RowChange, DiffOptions, Merge3Strategy};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::config::HubConfig;
use crate::services::hub::{
    acquire_db_lock, atomic_write, compute_bytes_hash, compute_file_hash, SyncBundle,
};
use crate::TreelineContext;

/// Max attempts for the conflict-merge loop. Each attempt re-downloads the
/// hub bundle and re-merges, so a hot hub (e.g. an AI session writing every
/// few seconds) either gets through once its writes pause or we surface an
/// error instead of silently clobbering it.
const MERGE_MAX_ATTEMPTS: u32 = 5;

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

/// The user's choice for resolving a sync conflict. Either way the losing
/// side only loses the *conflicting* values — its non-conflicting changes
/// still merge into the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    /// Conflicting values take this device's version.
    KeepLocal,
    /// Conflicting values take the hub's version.
    KeepHub,
}

/// Row/column-level detail of a sync conflict, for resolution UIs (desktop
/// modal, `tl hub conflicts`). Values are the raw cell values as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictReport {
    /// Hash of the hub DB this report was computed against.
    pub hub_hash: String,
    /// Rows changed only locally — merge cleanly whichever way conflicts go.
    pub local_only_changes: usize,
    /// Rows changed only on the hub — likewise merge cleanly.
    pub hub_only_changes: usize,
    pub total_conflicts: usize,
    pub tables: Vec<TableConflicts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConflicts {
    /// Table name, schema-qualified unless it's `main`.
    pub table: String,
    pub conflicts: Vec<RowConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowConflict {
    /// Both sides changed the same row; only conflicting columns are listed.
    Modified {
        /// Primary-key columns and values identifying the row.
        key: serde_json::Value,
        columns: Vec<ColumnConflict>,
    },
    /// Both sides added a row with the same key but different values.
    BothAdded {
        local_row: serde_json::Value,
        hub_row: serde_json::Value,
    },
    /// One side deleted the row while the other modified it.
    DeleteVsModify {
        /// Which side deleted (`None` only for legacy reports).
        deleted_by: Option<SyncSide>,
        deleted_row: serde_json::Value,
        modified_row: serde_json::Value,
    },
}

/// Which side of the sync a change came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncSide {
    Local,
    Hub,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnConflict {
    pub column: String,
    pub base: serde_json::Value,
    pub local: serde_json::Value,
    pub hub: serde_json::Value,
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
    Started {
        hub_url: String,
    },
    /// Initial reconcile finished: if the hub had moved while this device
    /// was away and the local DB was clean, it has been fast-forward
    /// pulled. App-level writers that run at startup (e.g. the desktop's
    /// automatic bank sync) should wait for this before writing, so they
    /// start from the hub's latest state instead of manufacturing a
    /// two-sided divergence. Always fires, even when the hub is
    /// unreachable — sync must never block local work indefinitely.
    Ready,
    LocalChangeDetected,
    Pushing,
    Pushed {
        bytes: u64,
    },
    AutoMerged {
        bytes: u64,
    },
    Pulling,
    Pulled {
        bytes: u64,
    },
    Conflict {
        hub_hash: String,
        conflicts: usize,
    },
    NoBaseSnapshot {
        hub_hash: String,
    },
    /// Push or poll failed — watch keeps running.
    Error {
        message: String,
    },
    Stopped,
}

/// Receiver for watch events. Implementors decide how to surface them.
pub trait WatchObserver: Send {
    fn on_event(&mut self, event: WatchEvent);
}

// ============================================================================
// Device-code link flow
// ============================================================================

/// In-progress device-code OAuth link.
///
/// The caller (CLI or desktop UI) holds this between `start` and the
/// successful `poll` that returns `Linked`. The flow has two visible
/// pieces: a `verification_uri_complete` to open in a browser, and a
/// short `user_code` to display alongside.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeLink {
    /// Hub URL the device code was issued against. Trimmed of trailing `/`.
    pub url: String,
    /// Friendly device name supplied at start time. Echoed in `Linked`.
    pub device_name: String,
    /// Server-issued long opaque code — used for polling, never shown.
    pub device_code: String,
    /// Short user-facing code (e.g. `T24Y-PSKJ`) shown beside the URL.
    pub user_code: String,
    /// `<hub>/authorize` — what to display if the caller wants the bare URL.
    pub verification_uri: String,
    /// `<hub>/authorize?user_code=…` — preferred URL to open in a browser
    /// because it pre-fills the form on the hub.
    pub verification_uri_complete: String,
    /// Poll interval in seconds. The hub may bump this via `slow_down`.
    pub interval: u64,
    /// When the device code stops being valid.
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Outcome of a single `DeviceCodeLink::poll` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DeviceCodeLinkOutcome {
    /// User hasn't completed the browser flow yet — poll again after `interval`.
    Pending,
    /// Hub asked us to back off — caller should add `interval` to the wait.
    SlowDown,
    /// Linked — `hub.json` was written to the supplied `treeline_dir`.
    Linked {
        hub_url: String,
        device_name: String,
    },
    /// Device code expired before authorization completed — caller restarts.
    Expired,
    /// User explicitly denied the authorization.
    Denied,
}

impl DeviceCodeLink {
    /// Start a device-code link against the given hub.
    ///
    /// Performs a `/health` check first so a typo'd URL fails fast with a
    /// clear error rather than waiting for the device-code request to hang.
    /// Returns the `DeviceCodeLink` the caller polls until `Linked`.
    pub fn start(url: &str, device_name: &str) -> Result<Self> {
        let url = url.trim_end_matches('/').to_string();
        let device_name = device_name.to_string();

        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .context("Failed to construct HTTP client")?;

        let resp = http
            .get(format!("{}/health", url))
            .send()
            .with_context(|| format!("Failed to reach hub at {}. Is it running?", url))?;
        if !resp.status().is_success() {
            anyhow::bail!("Hub health check failed with status {}", resp.status());
        }

        let resp = http
            .post(format!("{}/device/code", url))
            .form(&[
                ("scope", "pull push"),
                ("client_name", device_name.as_str()),
            ])
            .send()
            .context("Failed to request device code")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            anyhow::bail!("Hub rejected device code request ({}): {}", status, body);
        }

        let body: serde_json::Value = resp
            .json()
            .context("Failed to parse device code response")?;
        let device_code = body["device_code"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Hub response missing device_code"))?
            .to_string();
        let user_code = body["user_code"].as_str().unwrap_or("").to_string();
        let verification_uri = body["verification_uri"].as_str().unwrap_or("").to_string();
        let verification_uri_complete = body["verification_uri_complete"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let interval = body["interval"].as_i64().unwrap_or(2).max(1) as u64;
        let expires_in = body["expires_in"].as_i64().unwrap_or(600).max(60);
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in);

        Ok(Self {
            url,
            device_name,
            device_code,
            user_code,
            verification_uri,
            verification_uri_complete,
            interval,
            expires_at,
        })
    }

    /// Poll `/token` once. On `Linked`, writes `hub.json` into `treeline_dir`.
    ///
    /// Caller is responsible for sleeping `interval` between polls. Hub may
    /// also signal `SlowDown`, in which case the caller should add another
    /// `interval` to the wait before polling again.
    pub fn poll(&self, treeline_dir: &Path) -> Result<DeviceCodeLinkOutcome> {
        if chrono::Utc::now() > self.expires_at {
            return Ok(DeviceCodeLinkOutcome::Expired);
        }

        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .context("Failed to construct HTTP client")?;

        let resp = http
            .post(format!("{}/token", self.url))
            .form(&[
                ("grant_type", "device_code"),
                ("device_code", self.device_code.as_str()),
            ])
            .send()
            .context("Failed to poll /token")?;

        if resp.status().is_success() {
            let pair: serde_json::Value = resp.json().context("Failed to parse token response")?;
            let access_token = pair["access_token"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Token response missing access_token"))?
                .to_string();
            let refresh_token = pair["refresh_token"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Token response missing refresh_token"))?
                .to_string();
            // Custom OAuth extension: when an authorization server orchestrates
            // a link on behalf of a downstream hub (Treeline Cloud), the
            // /token response carries the hub's actual URL. Prefer it so sync
            // bypasses the orchestrator and goes direct.
            let hub_url = pair["hub_url"]
                .as_str()
                .map(|s| s.trim_end_matches('/').to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| self.url.clone());

            let hub = HubConfig {
                url: hub_url.clone(),
                access_token,
                refresh_token,
                device_name: self.device_name.clone(),
                last_push: None,
                last_pull: None,
                base_hash: None,
                // The URL the user pointed the modal at — preserved separately
                // because Pro's `/token` overwrites `url` with the hub's
                // direct URL (so sync can bypass Pro), which loses the signal
                // that this was a Pro-orchestrated link.
                link_origin: Some(self.url.clone()),
                extra: Default::default(),
            };
            std::fs::create_dir_all(treeline_dir)
                .with_context(|| format!("Failed to create {}", treeline_dir.display()))?;
            hub.save(treeline_dir)?;

            return Ok(DeviceCodeLinkOutcome::Linked {
                hub_url,
                device_name: self.device_name.clone(),
            });
        }

        // RFC 8628: 400 with `error` of authorization_pending / slow_down /
        // expired_token / access_denied / invalid_grant. Anything else is
        // a real failure — surface `error_description` so users see Pro's
        // actual message ("You don't have a hub yet…") instead of an opaque
        // "server_error" code.
        let body: serde_json::Value = resp.json().unwrap_or_default();
        let err = body["error"].as_str().unwrap_or("");
        match err {
            "authorization_pending" => Ok(DeviceCodeLinkOutcome::Pending),
            "slow_down" => Ok(DeviceCodeLinkOutcome::SlowDown),
            "expired_token" => Ok(DeviceCodeLinkOutcome::Expired),
            "access_denied" => Ok(DeviceCodeLinkOutcome::Denied),
            other => {
                let desc = body["error_description"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(other);
                anyhow::bail!("{}", desc);
            }
        }
    }
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
            .ok_or_else(|| anyhow::anyhow!("Not linked to a hub. Run 'tl hub link' first."))?;

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

        let mut resp =
            send(&hub.access_token, bundle.clone()).context("Failed to connect to hub")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh_access_token(&mut hub)?;
            resp = send(&hub.access_token, bundle.clone()).context("Failed to connect to hub")?;
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
        save_base_snapshot_from_bundle(&self.treeline_dir, &bundle)?;

        Ok(PushOutcome::Pushed {
            bytes: size,
            hash: new_hash,
        })
    }

    /// Pull the hub's DB to local. Backs up current local state first.
    pub fn pull(&self) -> Result<PullOutcome> {
        let mut hub = HubConfig::load(&self.treeline_dir)?
            .ok_or_else(|| anyhow::anyhow!("Not linked to a hub. Run 'tl hub link' first."))?;

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

        // Hash + base snapshot come from the bundle's own DB bytes — the
        // same bytes the hub hashed — not from the live file, which a local
        // write may already have moved past by now.
        let new_hash = SyncBundle::db_entry_bytes(&bytes)?
            .as_deref()
            .map(compute_bytes_hash);
        save_base_snapshot_from_bundle(&self.treeline_dir, &bytes)?;

        let mut hub = HubConfig::load(&self.treeline_dir)?.unwrap();
        hub.last_pull = Some(chrono::Utc::now());
        hub.base_hash = new_hash.clone();
        hub.save(&self.treeline_dir)?;

        Ok(PullOutcome {
            bytes: size,
            hash: new_hash,
        })
    }

    /// Best-effort RFC 7009 revoke of this device's refresh token. The hub
    /// cascades the revoke to every access token minted from it, so the
    /// device is fully de-authorized after a successful call. Returns Err
    /// on network failure or non-2xx status; callers (typically `unlink_hub`)
    /// should log the failure and continue with local cleanup since the
    /// user's intent is clear.
    pub fn revoke_tokens(&self) -> Result<()> {
        let hub = HubConfig::load(&self.treeline_dir)?
            .ok_or_else(|| anyhow::anyhow!("Not linked to a hub."))?;

        let resp = self
            .http
            .post(format!("{}/revoke", hub.url))
            .form(&[
                ("token", hub.refresh_token.as_str()),
                ("token_type_hint", "refresh_token"),
            ])
            .timeout(Duration::from_secs(10))
            .send()
            .context("Failed to reach /revoke")?;

        if !resp.status().is_success() {
            anyhow::bail!("Revoke returned {}", resp.status());
        }
        Ok(())
    }

    /// Ask the hub for its current bundle hash. Used by the watch loop to
    /// decide whether a pull is needed.
    pub fn poll_hub_hash(&self) -> Result<Option<String>> {
        let hub = match HubConfig::load(&self.treeline_dir)? {
            Some(h) => h,
            None => return Ok(None),
        };

        let resp = self
            .http
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
            .ok_or_else(|| anyhow::anyhow!("Not linked to a hub. Run 'tl hub link' first."))?;

        let _lock = WatchLock::acquire(&self.treeline_dir)
            .context("Another watcher is already running for this directory")?;

        observer.on_event(WatchEvent::Started {
            hub_url: hub.url.clone(),
        });

        let db_path = self.treeline_dir.join("treeline.duckdb");
        let mut last_mtime = db_path.metadata().and_then(|m| m.modified()).ok();
        let mut last_poll = Instant::now();
        let mut last_polled_hub_hash: Option<String> = None;

        // Initial reconcile: fast-forward pull if the hub moved while this
        // device was away and the local DB is clean, then signal Ready.
        // Poll failures fall through to Ready — an unreachable hub must
        // never block startup.
        if let Ok(hub_hash) = self.poll_hub_hash() {
            if hub_hash.is_some() {
                last_polled_hub_hash = hub_hash.clone();
                if let Ok(Some(hub_cfg)) = HubConfig::load(&self.treeline_dir) {
                    let hub_moved = hub_hash.as_deref() != hub_cfg.base_hash.as_deref();
                    // A missing base means we've never synced — the device is
                    // the source of truth and must not be clobbered by a pull.
                    let local_clean = match hub_cfg.base_hash.as_deref() {
                        Some(base) => compute_file_hash(&db_path)
                            .map(|h| h == base)
                            .unwrap_or(false),
                        None => false,
                    };
                    if hub_moved && local_clean {
                        observer.on_event(WatchEvent::Pulling);
                        match self.pull() {
                            Ok(out) => {
                                last_polled_hub_hash = out.hash.clone();
                                observer.on_event(WatchEvent::Pulled { bytes: out.bytes });
                            }
                            Err(e) => observer.on_event(WatchEvent::Error {
                                message: e.to_string(),
                            }),
                        }
                        last_mtime = db_path.metadata().and_then(|m| m.modified()).ok();
                    }
                }
            }
        }
        observer.on_event(WatchEvent::Ready);

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
                let synced = match self.push(ctx, false) {
                    Ok(PushOutcome::Pushed { bytes, .. }) => {
                        observer.on_event(WatchEvent::Pushed { bytes });
                        true
                    }
                    Ok(PushOutcome::AutoMerged { bytes, .. }) => {
                        observer.on_event(WatchEvent::AutoMerged { bytes });
                        true
                    }
                    Ok(PushOutcome::Conflict {
                        hub_hash,
                        conflicts,
                    }) => {
                        observer.on_event(WatchEvent::Conflict {
                            hub_hash,
                            conflicts: conflicts.len(),
                        });
                        false
                    }
                    Ok(PushOutcome::NoBaseSnapshot { hub_hash }) => {
                        observer.on_event(WatchEvent::NoBaseSnapshot { hub_hash });
                        false
                    }
                    Ok(PushOutcome::NoChanges) => true,
                    Err(e) => {
                        observer.on_event(WatchEvent::Error {
                            message: e.to_string(),
                        });
                        false
                    }
                };
                last_poll = Instant::now();
                last_mtime = if synced {
                    rearm_after_sync(&db_path, &self.treeline_dir)
                } else {
                    db_path.metadata().and_then(|m| m.modified()).ok()
                };
                continue;
            }

            if last_poll.elapsed() >= opts.poll {
                last_poll = Instant::now();

                let hub_hash = match self.poll_hub_hash() {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                // Quiescence check: only act once the hub hash has been
                // stable across two consecutive polls. A hash that's still
                // moving means someone (e.g. an MCP session) is actively
                // writing — pulling or merging now would race them and
                // re-enter the merge cycle every poll.
                let hub_stable = hub_hash.is_some() && hub_hash == last_polled_hub_hash;
                last_polled_hub_hash = hub_hash.clone();

                let hub_cfg = match HubConfig::load(&self.treeline_dir)? {
                    Some(h) => h,
                    None => continue,
                };

                let needs_pull = match (hub_hash.as_deref(), hub_cfg.base_hash.as_deref()) {
                    (Some(hub), Some(base)) => hub != base,
                    (Some(_), None) => true,
                    _ => false,
                };
                if !needs_pull || !hub_stable {
                    continue;
                }

                let has_local_changes = match hub_cfg.base_hash.as_deref() {
                    Some(base) => compute_file_hash(&db_path)
                        .map(|h| h.as_str() != base)
                        .unwrap_or(false),
                    // No base means we've never synced (e.g. just linked). The
                    // device is the source of truth — treat any local DB as
                    // "has changes to push" so we don't clobber it with a pull
                    // from a hub that may have stale or different data.
                    None => db_path.exists(),
                };

                let synced = if has_local_changes {
                    // Both sides moved — push triggers the three-way merge.
                    observer.on_event(WatchEvent::Pushing);
                    match self.push(ctx, false) {
                        Ok(PushOutcome::Pushed { bytes, .. }) => {
                            observer.on_event(WatchEvent::Pushed { bytes });
                            true
                        }
                        Ok(PushOutcome::AutoMerged { bytes, .. }) => {
                            observer.on_event(WatchEvent::AutoMerged { bytes });
                            true
                        }
                        Ok(PushOutcome::Conflict {
                            hub_hash,
                            conflicts,
                        }) => {
                            observer.on_event(WatchEvent::Conflict {
                                hub_hash,
                                conflicts: conflicts.len(),
                            });
                            false
                        }
                        Ok(PushOutcome::NoBaseSnapshot { hub_hash }) => {
                            observer.on_event(WatchEvent::NoBaseSnapshot { hub_hash });
                            false
                        }
                        Ok(PushOutcome::NoChanges) => true,
                        Err(e) => {
                            observer.on_event(WatchEvent::Error {
                                message: e.to_string(),
                            });
                            false
                        }
                    }
                } else {
                    observer.on_event(WatchEvent::Pulling);
                    match self.pull() {
                        Ok(out) => {
                            observer.on_event(WatchEvent::Pulled { bytes: out.bytes });
                            true
                        }
                        Err(e) => {
                            observer.on_event(WatchEvent::Error {
                                message: e.to_string(),
                            });
                            false
                        }
                    }
                };

                last_mtime = if synced {
                    rearm_after_sync(&db_path, &self.treeline_dir)
                } else {
                    db_path.metadata().and_then(|m| m.modified()).ok()
                };
            }
        }

        observer.on_event(WatchEvent::Stopped);
        Ok(())
    }

    /// Refresh `hub.access_token` using its `refresh_token` and persist.
    fn refresh_access_token(&self, hub: &mut HubConfig) -> Result<()> {
        let resp = self
            .http
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
    ///
    /// The merged result is pushed with `base_hash` set to the hash of the
    /// hub state the merge was computed against (compare-and-swap). If the
    /// hub or the local DB moved during the merge, the whole attempt is
    /// retried against the new state — never force-pushed, so writes that
    /// land mid-merge (e.g. an AI session tagging through the hub MCP) are
    /// picked up by the re-merge instead of being clobbered.
    fn handle_conflict(
        &self,
        ctx: &TreelineContext,
        hub: &mut HubConfig,
        hub_hash: String,
    ) -> Result<PushOutcome> {
        self.merge_with_retry(ctx, hub, hub_hash, Merge3Strategy::FailOnConflict)
    }

    /// Resolve an outstanding conflict by choosing which side wins the
    /// conflicting values. Runs the same CAS'd merge as the watcher, but
    /// with `AcceptA`/`AcceptB` instead of `FailOnConflict` — so both
    /// sides' non-conflicting changes are preserved and only the truly
    /// conflicting values take the chosen side.
    pub fn resolve_conflicts(
        &self,
        ctx: &TreelineContext,
        resolution: ConflictResolution,
    ) -> Result<PushOutcome> {
        let mut hub = HubConfig::load(&self.treeline_dir)?
            .ok_or_else(|| anyhow::anyhow!("Not linked to a hub. Run 'tl hub link' first."))?;

        // Diff order is diff3(base, local, hub): A = local, B = hub.
        let strategy = match resolution {
            ConflictResolution::KeepLocal => Merge3Strategy::AcceptA,
            ConflictResolution::KeepHub => Merge3Strategy::AcceptB,
        };

        // Same pre-flight as push(): ship the smallest, most-consistent file.
        ctx.compact_service.compact()?;
        ctx.repository.checkpoint()?;

        let hub_hash = self.poll_hub_hash()?.unwrap_or_default();
        self.merge_with_retry(ctx, &mut hub, hub_hash, strategy)
    }

    /// Compute the row/column-level conflict detail between local and hub
    /// state, for display before resolving. Read-only — touches nothing.
    pub fn conflict_report(&self, ctx: &TreelineContext) -> Result<ConflictReport> {
        let mut hub = HubConfig::load(&self.treeline_dir)?
            .ok_or_else(|| anyhow::anyhow!("Not linked to a hub. Run 'tl hub link' first."))?;

        let base_db_path = self.treeline_dir.join(".treeline.base.duckdb");
        if !base_db_path.exists() {
            anyhow::bail!(
                "No base snapshot — cannot compute conflict details. \
                 Resolve via 'tl hub push --force' or 'tl hub pull'."
            );
        }

        let hub_snap = self.download_hub_snapshot(&mut hub)?;
        let local_snap = self.snapshot_local_db()?;
        let report =
            self.diff_against_hub(ctx, &base_db_path, &local_snap.db_path, &hub_snap.db_path)?;
        Ok(build_conflict_report(&report, hub_snap.db_hash))
    }

    fn merge_with_retry(
        &self,
        ctx: &TreelineContext,
        hub: &mut HubConfig,
        hub_hash: String,
        strategy: Merge3Strategy,
    ) -> Result<PushOutcome> {
        let base_db_path = self.treeline_dir.join(".treeline.base.duckdb");
        if !base_db_path.exists() {
            return Ok(PushOutcome::NoBaseSnapshot { hub_hash });
        }

        for attempt in 0..MERGE_MAX_ATTEMPTS {
            if attempt > 0 {
                // One side is actively being written; give it a moment to
                // go quiet before downloading and merging again.
                std::thread::sleep(Duration::from_secs(1 << attempt.min(3)));
            }
            match self.merge_and_push(ctx, hub, &base_db_path, &strategy)? {
                MergeAttempt::Done(outcome) => return Ok(outcome),
                MergeAttempt::HubMoved | MergeAttempt::LocalMoved => {}
            }
        }
        anyhow::bail!(
            "Hub kept changing during merge — gave up after {} attempts. \
             Will retry on the next sync cycle.",
            MERGE_MAX_ATTEMPTS
        )
    }

    /// One conflict-merge attempt: download the hub bundle, three-way merge
    /// against the base snapshot, apply the result locally through the
    /// locked atomic path, and CAS-push it back to the hub.
    fn merge_and_push(
        &self,
        ctx: &TreelineContext,
        hub: &mut HubConfig,
        base_db_path: &Path,
        strategy: &Merge3Strategy,
    ) -> Result<MergeAttempt> {
        // The CAS base for the merged push is the hash of the *downloaded*
        // DB bytes, not the hash from the 409 — the hub may have moved
        // again in between.
        let hub_snap = self.download_hub_snapshot(hub)?;
        let local_snap = self.snapshot_local_db()?;
        let local_hash_at_merge = local_snap.hash.clone();

        let diff3_report =
            self.diff_against_hub(ctx, base_db_path, &local_snap.db_path, &hub_snap.db_path)?;

        let total_changes =
            diff3_report.summary.total_non_conflicting + diff3_report.summary.total_conflicts;
        let has_new_tables =
            !diff3_report.a_only_tables.is_empty() || !diff3_report.b_only_tables.is_empty();

        if total_changes == 0 && !has_new_tables {
            // Logically identical to the hub even though the file bytes
            // differ (DuckDB layout isn't deterministic). Adopt the hub's
            // state as the new base so the watcher stops seeing a phantom
            // divergence every poll.
            let mut hub_cfg = HubConfig::load(&self.treeline_dir)?.unwrap();
            hub_cfg.base_hash = Some(hub_snap.db_hash);
            hub_cfg.save(&self.treeline_dir)?;
            save_base_snapshot_from_bundle(&self.treeline_dir, &hub_snap.bundle)?;
            return Ok(MergeAttempt::Done(PushOutcome::NoChanges));
        }

        if matches!(strategy, Merge3Strategy::FailOnConflict)
            && diff3_report.summary.total_conflicts > 0
        {
            let conflicts = collect_conflicts(&diff3_report);
            return Ok(MergeAttempt::Done(PushOutcome::Conflict {
                hub_hash: hub_snap.db_hash,
                conflicts,
            }));
        }

        // Merge against a copy of the base. With AcceptA/AcceptB, conflicts
        // take the chosen side; non-conflicting changes merge from both.
        let encryption_key = ctx.repository.encryption_key().map(|s| s.to_string());
        let merge_temp = tempfile::TempDir::new()?;
        let merge_db_path = merge_temp.path().join("merged.duckdb");
        std::fs::copy(base_db_path, &merge_db_path)?;

        let mut merge_ancestor = DatabaseConfig::new(merge_db_path.to_string_lossy());
        let mut local_config = DatabaseConfig::new(local_snap.db_path.to_string_lossy());
        let mut hub_config = DatabaseConfig::new(hub_snap.db_path.to_string_lossy());
        if let Some(ref key) = encryption_key {
            merge_ancestor = merge_ancestor.with_key(key);
            local_config = local_config.with_key(key);
            hub_config = hub_config.with_key(key);
        }

        diffy_duck::merge3(&merge_ancestor, &local_config, &hub_config, strategy)
            .map_err(|e| anyhow::anyhow!("Failed to merge: {}", e))?;
        let hub_bundle_hash = hub_snap.db_hash;

        // Apply the merged DB to the live file under the DB lock, atomically
        // — never a bare copy onto a file an open connection may be reading.
        // If a local write landed while we were merging, don't clobber it;
        // retry the merge against the new local state instead.
        let local_db_path = self.treeline_dir.join("treeline.duckdb");
        let merged_bytes = std::fs::read(&merge_db_path)?;
        {
            let _lock = acquire_db_lock(&self.treeline_dir)?;
            if compute_file_hash(&local_db_path)? != local_hash_at_merge {
                return Ok(MergeAttempt::LocalMoved);
            }
            atomic_write(&local_db_path, &merged_bytes)?;
        }

        let merged_bundle = SyncBundle::create(&self.treeline_dir)?;
        let size = merged_bundle.len() as u64;

        let push_url = build_push_url(&hub.url, Some(&hub_bundle_hash));
        let resp = self
            .http
            .post(&push_url)
            .header("Authorization", format!("Bearer {}", hub.access_token))
            .header("Content-Type", "application/octet-stream")
            .body(merged_bundle.clone())
            .timeout(Duration::from_secs(300))
            .send()
            .context("Failed to push merged database")?;

        if resp.status() == reqwest::StatusCode::CONFLICT {
            return Ok(MergeAttempt::HubMoved);
        }
        if !resp.status().is_success() {
            let body = resp.text().unwrap_or_default();
            anyhow::bail!("Push after merge failed: {}", body);
        }

        let body: serde_json::Value = resp.json().unwrap_or_default();
        let new_hash = body["hash"].as_str().map(|s| s.to_string());
        let mut hub_cfg = HubConfig::load(&self.treeline_dir)?.unwrap();
        hub_cfg.last_push = Some(chrono::Utc::now());
        hub_cfg.base_hash = new_hash.clone();
        hub_cfg.save(&self.treeline_dir)?;
        save_base_snapshot_from_bundle(&self.treeline_dir, &merged_bundle)?;

        Ok(MergeAttempt::Done(PushOutcome::AutoMerged {
            bytes: size,
            hash: new_hash,
        }))
    }
}

impl HubClient {
    /// Download the hub's current bundle, extract it to a temp dir, and
    /// compute the hash of its DB bytes. Refreshes the access token on 401.
    fn download_hub_snapshot(&self, hub: &mut HubConfig) -> Result<HubSnapshot> {
        let pull_url = format!("{}/api/pull", hub.url);
        let send = |access_token: &str| {
            self.http
                .get(&pull_url)
                .header("Authorization", format!("Bearer {}", access_token))
                .timeout(Duration::from_secs(300))
                .send()
        };

        let mut resp = send(&hub.access_token).context("Failed to download hub bundle")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh_access_token(hub)?;
            resp = send(&hub.access_token).context("Failed to download hub bundle")?;
        }
        if !resp.status().is_success() {
            anyhow::bail!("Failed to download hub bundle: {}", resp.status());
        }

        let bundle: Vec<u8> = resp.bytes()?.into();
        let db_bytes = SyncBundle::db_entry_bytes(&bundle)?
            .ok_or_else(|| anyhow::anyhow!("Hub bundle does not contain a database"))?;
        let db_hash = compute_bytes_hash(&db_bytes);

        let temp = tempfile::TempDir::new().context("Failed to create temp dir")?;
        SyncBundle::extract(&bundle, temp.path())?;
        let db_path = temp.path().join("treeline.duckdb");

        Ok(HubSnapshot {
            bundle,
            db_hash,
            db_path,
            _temp: temp,
        })
    }

    /// Copy the live DB to a temp file under the DB lock, so diffs read a
    /// stable file rather than one desktop connections are writing to.
    fn snapshot_local_db(&self) -> Result<LocalSnapshot> {
        let local_db_path = self.treeline_dir.join("treeline.duckdb");
        let temp = tempfile::TempDir::new()?;
        let db_path = temp.path().join("local.duckdb");
        let hash = {
            let _lock = acquire_db_lock(&self.treeline_dir)?;
            std::fs::copy(&local_db_path, &db_path)?;
            compute_file_hash(&db_path)?
        };
        Ok(LocalSnapshot {
            db_path,
            hash,
            _temp: temp,
        })
    }

    /// Three-way diff: base (ancestor) vs local (A) vs hub (B).
    fn diff_against_hub(
        &self,
        ctx: &TreelineContext,
        base_db_path: &Path,
        local_db_path: &Path,
        hub_db_path: &Path,
    ) -> Result<diffy_duck::Diff3Report> {
        let encryption_key = ctx.repository.encryption_key().map(|s| s.to_string());

        let mut ancestor_config = DatabaseConfig::new(base_db_path.to_string_lossy());
        let mut local_config = DatabaseConfig::new(local_db_path.to_string_lossy());
        let mut hub_config = DatabaseConfig::new(hub_db_path.to_string_lossy());
        if let Some(ref key) = encryption_key {
            ancestor_config = ancestor_config.with_key(key);
            local_config = local_config.with_key(key);
            hub_config = hub_config.with_key(key);
        }

        diffy_duck::diff3(
            &ancestor_config,
            &local_config,
            &hub_config,
            &DiffOptions::default(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to diff databases: {}", e))
    }
}

/// The hub's state at a point in time: bundle bytes, its DB extracted to a
/// temp dir, and the DB hash (the CAS base for a merged push).
struct HubSnapshot {
    bundle: Vec<u8>,
    db_hash: String,
    db_path: PathBuf,
    _temp: tempfile::TempDir,
}

/// A stable copy of the live local DB, taken under the DB lock.
struct LocalSnapshot {
    db_path: PathBuf,
    hash: String,
    _temp: tempfile::TempDir,
}

/// Map a diffy-duck three-way report to the user-facing `ConflictReport`.
/// In our diff order A = local, B = hub.
fn build_conflict_report(report: &diffy_duck::Diff3Report, hub_hash: String) -> ConflictReport {
    let to_value = |row: &diffy_duck::Row| serde_json::to_value(row).unwrap_or(serde_json::Value::Null);

    let mut tables = Vec::new();
    let mut local_only = 0usize;
    let mut hub_only = 0usize;
    let mut total_conflicts = 0usize;

    for table_diff in &report.tables {
        let s = &table_diff.summary;
        local_only += s.added_a + s.removed_a + s.modified_a;
        hub_only += s.added_b + s.removed_b + s.modified_b;

        let mut conflicts = Vec::new();
        for change in &table_diff.changes {
            match change {
                Diff3RowChange::Modified { key, column_changes } => {
                    let columns: Vec<ColumnConflict> = column_changes
                        .iter()
                        .filter(|c| c.origin == Diff3ChangeOrigin::Conflict)
                        .map(|c| ColumnConflict {
                            column: c.column.clone(),
                            base: c.ancestor_value.clone(),
                            local: c.a_value.clone(),
                            hub: c.b_value.clone(),
                        })
                        .collect();
                    if !columns.is_empty() {
                        conflicts.push(RowConflict::Modified {
                            key: to_value(key),
                            columns,
                        });
                    }
                }
                Diff3RowChange::Added {
                    row,
                    origin: Diff3ChangeOrigin::Conflict,
                    other_row,
                } => {
                    conflicts.push(RowConflict::BothAdded {
                        local_row: to_value(row),
                        hub_row: other_row
                            .as_ref()
                            .map(&to_value)
                            .unwrap_or(serde_json::Value::Null),
                    });
                }
                Diff3RowChange::Removed {
                    row,
                    origin: Diff3ChangeOrigin::Conflict,
                    modified_row,
                    removed_by,
                } => {
                    conflicts.push(RowConflict::DeleteVsModify {
                        deleted_by: removed_by.map(|s| match s {
                            diffy_duck::Diff3Side::A => SyncSide::Local,
                            diffy_duck::Diff3Side::B => SyncSide::Hub,
                        }),
                        deleted_row: to_value(row),
                        modified_row: modified_row
                            .as_ref()
                            .map(&to_value)
                            .unwrap_or(serde_json::Value::Null),
                    });
                }
                _ => {}
            }
        }

        if !conflicts.is_empty() {
            total_conflicts += conflicts.len();
            let t = &table_diff.table;
            let table = if t.schema == "main" {
                t.name.clone()
            } else {
                format!("{}.{}", t.schema, t.name)
            };
            tables.push(TableConflicts { table, conflicts });
        }
    }

    ConflictReport {
        hub_hash,
        local_only_changes: local_only,
        hub_only_changes: hub_only,
        total_conflicts,
        tables,
    }
}

/// Outcome of a single `merge_and_push` attempt.
enum MergeAttempt {
    /// Terminal — surface this outcome to the caller.
    Done(PushOutcome),
    /// The hub accepted new writes while we merged (CAS push 409'd).
    HubMoved,
    /// The local DB changed while we merged (desktop write mid-merge).
    LocalMoved,
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
                Diff3RowChange::Added {
                    origin: Diff3ChangeOrigin::Conflict,
                    ..
                } => {
                    out.push(ConflictDescription {
                        table: table_diff.table.to_string(),
                        kind: ConflictKind::BothAdded,
                        detail: "both sides added a row with the same key".to_string(),
                    });
                }
                Diff3RowChange::Removed {
                    origin: Diff3ChangeOrigin::Conflict,
                    ..
                } => {
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

/// Re-arm the watch loop's change detector after a successful sync
/// operation. The operation itself bumps the DB file's mtime (compact,
/// checkpoint, extract), which must be absorbed — but a *user* write that
/// landed mid-operation must not be absorbed with it, or it strands
/// unsynced until the next unrelated edit. Content is the tiebreaker: if
/// the live DB no longer matches the freshly-synced base, return `None`
/// so the next tick treats the file as changed and syncs again.
fn rearm_after_sync(db_path: &Path, treeline_dir: &Path) -> Option<SystemTime> {
    let mtime = db_path.metadata().and_then(|m| m.modified()).ok();
    let base_hash = HubConfig::load(treeline_dir)
        .ok()
        .flatten()
        .and_then(|c| c.base_hash);
    match (compute_file_hash(db_path), base_hash) {
        (Ok(h), Some(base)) if h == base => mtime,
        _ => None,
    }
}

/// Persist `.treeline.base.duckdb` from the exact DB bytes that were pushed
/// or pulled. Snapshotting the live file instead would bake in any local
/// write that landed during the upload — a write the hub never saw — and
/// every future three-way merge would then read it as "hub deleted this"
/// and delete it locally.
fn save_base_snapshot_from_bundle(treeline_dir: &Path, bundle: &[u8]) -> Result<()> {
    if let Some(db_bytes) = SyncBundle::db_entry_bytes(bundle)? {
        let base_path = treeline_dir.join(".treeline.base.duckdb");
        atomic_write(&base_path, &db_bytes).context("Failed to save base snapshot")?;
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
    fn conflict_report_maps_row_and_column_detail() {
        let dir = tempfile::TempDir::new().unwrap();
        let make_db = |name: &str, rows: &[(i32, &str)]| {
            let path = dir.path().join(name);
            let conn = duckdb::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE txns (id INTEGER PRIMARY KEY, tag TEXT)")
                .unwrap();
            for (id, tag) in rows {
                conn.execute("INSERT INTO txns VALUES (?, ?)", duckdb::params![id, tag])
                    .unwrap();
            }
            drop(conn);
            path
        };

        // Base: rows 1, 2. Local: modifies 1, adds 3. Hub: modifies 1
        // (differently), adds 4. Row 1 tag = genuine conflict.
        let base = make_db("base.duckdb", &[(1, "a"), (2, "x")]);
        let local = make_db("local.duckdb", &[(1, "local-tag"), (2, "x"), (3, "l")]);
        let hub = make_db("hub.duckdb", &[(1, "hub-tag"), (2, "x"), (4, "h")]);

        let report = diffy_duck::diff3(
            &DatabaseConfig::new(base.to_string_lossy()),
            &DatabaseConfig::new(local.to_string_lossy()),
            &DatabaseConfig::new(hub.to_string_lossy()),
            &DiffOptions::default(),
        )
        .unwrap();

        let cr = build_conflict_report(&report, "hash123".into());
        assert_eq!(cr.hub_hash, "hash123");
        assert_eq!(cr.total_conflicts, 1);
        assert_eq!(cr.local_only_changes, 1, "row 3 added locally");
        assert_eq!(cr.hub_only_changes, 1, "row 4 added on hub");
        assert_eq!(cr.tables.len(), 1);
        assert_eq!(cr.tables[0].table, "txns");
        match &cr.tables[0].conflicts[0] {
            RowConflict::Modified { key, columns } => {
                assert_eq!(key["id"], 1);
                assert_eq!(columns.len(), 1);
                assert_eq!(columns[0].column, "tag");
                assert_eq!(columns[0].base, "a");
                assert_eq!(columns[0].local, "local-tag");
                assert_eq!(columns[0].hub, "hub-tag");
            }
            other => panic!("expected Modified conflict, got {:?}", other),
        }
    }

    #[test]
    fn side_resolution_keeps_losing_sides_non_conflicting_changes() {
        // The resolution UI promises: the losing side only loses the
        // conflicting values — its other changes still merge in. Verify
        // that AcceptB (keep hub) preserves a local-only added row.
        let dir = tempfile::TempDir::new().unwrap();
        let make_db = |name: &str, rows: &[(i32, &str)]| {
            let path = dir.path().join(name);
            let conn = duckdb::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE txns (id INTEGER PRIMARY KEY, tag TEXT)")
                .unwrap();
            for (id, tag) in rows {
                conn.execute("INSERT INTO txns VALUES (?, ?)", duckdb::params![id, tag])
                    .unwrap();
            }
            drop(conn);
            path
        };

        let base = make_db("base.duckdb", &[(1, "a"), (2, "x")]);
        let local = make_db("local.duckdb", &[(1, "local-tag"), (2, "x"), (3, "l")]);
        let hub = make_db("hub.duckdb", &[(1, "hub-tag"), (2, "x"), (4, "h")]);

        // Merge into a copy of base, hub winning conflicts.
        let merged = dir.path().join("merged.duckdb");
        std::fs::copy(&base, &merged).unwrap();
        diffy_duck::merge3(
            &DatabaseConfig::new(merged.to_string_lossy()),
            &DatabaseConfig::new(local.to_string_lossy()),
            &DatabaseConfig::new(hub.to_string_lossy()),
            &Merge3Strategy::AcceptB,
        )
        .unwrap();

        let conn = duckdb::Connection::open(&merged).unwrap();
        let rows: Vec<(i32, String)> = conn
            .prepare("SELECT id, tag FROM txns ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(
            rows,
            vec![
                (1, "hub-tag".to_string()), // conflict → hub wins
                (2, "x".to_string()),       // untouched
                (3, "l".to_string()),       // local-only add SURVIVES
                (4, "h".to_string()),       // hub-only add merges
            ]
        );
    }

    #[test]
    fn base_snapshot_comes_from_bundle_bytes_not_live_file() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("treeline.duckdb"), b"bundled-state").unwrap();
        let bundle = SyncBundle::create(dir.path()).unwrap();

        // A local write lands while the bundle is uploading — it must NOT
        // end up in the base snapshot, since the hub never saw it.
        std::fs::write(
            dir.path().join("treeline.duckdb"),
            b"local-write-during-upload",
        )
        .unwrap();

        save_base_snapshot_from_bundle(dir.path(), &bundle).unwrap();
        let base = std::fs::read(dir.path().join(".treeline.base.duckdb")).unwrap();
        assert_eq!(base, b"bundled-state");
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
