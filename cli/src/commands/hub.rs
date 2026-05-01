//! Hub commands - link, unlink, push, pull, status
//!
//! Manages remote hub connections for database sync.

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use treeline_core::config::HubConfig;
use treeline_core::services::HubClient;
use treeline_core::services::hub_client::{
    ConflictDescription, ConflictKind, DeviceCodeLink, DeviceCodeLinkOutcome, PullOutcome,
    PushOutcome, WatchEvent, WatchObserver, WatchOptions,
};

use super::{get_context, get_treeline_dir};

#[derive(Subcommand)]
pub enum HubCommands {
    /// Link this device to a hub.
    ///
    /// Uses the device-code OAuth flow: prints a verification URL that you
    /// open in a browser, sign in (or master-paste if self-hosting), and
    /// the CLI finishes automatically. This device gets its own scoped
    /// device token; nothing privileged is stored locally.
    ///
    /// With no `--url`, defaults to Treeline Cloud
    /// (https://pro.treeline.money). Override via the TREELINE_PRO_URL
    /// env var or pass `--url <hub-url>` for a self-hosted hub.
    Link {
        /// Hub URL. Optional — defaults to Treeline Cloud.
        #[arg(long)]
        url: Option<String>,
        /// Override the device name (defaults to the OS hostname).
        #[arg(long)]
        name: Option<String>,
    },

    /// Unlink from the remote hub
    Unlink,

    /// Push local database to the hub
    Push {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Force push — skip conflict detection, overwrite hub
        #[arg(long)]
        force: bool,
    },

    /// Pull latest database from the hub
    Pull {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show hub connection status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Watch for local changes and auto-sync with the hub
    Watch {
        /// Seconds to wait after last change before pushing (default: 5)
        #[arg(long, default_value = "5")]
        debounce: u64,
        /// Seconds between hub poll checks for incoming changes (default: 15)
        #[arg(long, default_value = "15")]
        poll: u64,
    },

    /// Manage OAuth tokens issued by this hub
    Tokens {
        #[command(subcommand)]
        command: TokensCommands,
    },
}

#[derive(Subcommand)]
pub enum TokensCommands {
    /// List active access tokens issued to thin clients
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Revoke an access token by its displayed prefix
    Revoke {
        /// Token prefix (first 8 characters as shown by `tokens list`)
        prefix: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Compile-time default for `tl hub link` when no `--url` is supplied.
/// Override at runtime with `TREELINE_PRO_URL`.
const DEFAULT_PRO_URL: &str = "https://pro.treeline.money";

fn resolve_link_url(arg_url: Option<&str>) -> String {
    if let Some(u) = arg_url {
        return u.to_string();
    }
    std::env::var("TREELINE_PRO_URL").unwrap_or_else(|_| DEFAULT_PRO_URL.to_string())
}

pub fn run(command: HubCommands) -> Result<()> {
    match command {
        HubCommands::Link { url, name } => {
            let url = resolve_link_url(url.as_deref());
            run_link(&url, name.as_deref())
        }
        HubCommands::Unlink => run_unlink(),
        HubCommands::Push { json, force } => run_push(json, force),
        HubCommands::Pull { json } => run_pull(json),
        HubCommands::Status { json } => run_status(json),
        HubCommands::Watch { debounce, poll } => run_watch(debounce, poll),
        HubCommands::Tokens { command } => run_tokens(command),
    }
}

fn run_tokens(command: TokensCommands) -> Result<()> {
    match command {
        TokensCommands::List { json } => run_tokens_list(json),
        TokensCommands::Revoke { prefix, json } => run_tokens_revoke(&prefix, json),
    }
}

fn run_tokens_list(json: bool) -> Result<()> {
    use treeline_core::services::oauth::OAuthStore;
    let treeline_dir = get_treeline_dir();
    let store = OAuthStore::new(treeline_dir);
    let tokens = store.list_tokens().context("Failed to read OAuth state")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&tokens)?);
        return Ok(());
    }

    if tokens.is_empty() {
        eprintln!("No active OAuth tokens.");
        return Ok(());
    }

    use comfy_table::{Cell, Table};
    let mut table = Table::new();
    table.set_header(vec![
        "Prefix",
        "Client",
        "Scopes",
        "Issued",
        "Expires",
    ]);
    for t in &tokens {
        table.add_row(vec![
            Cell::new(&t.access_token_prefix),
            Cell::new(t.client_name.as_deref().unwrap_or("—")),
            Cell::new(t.scopes.join(", ")),
            Cell::new(t.issued_at.format("%Y-%m-%d %H:%M")),
            Cell::new(t.expires_at.format("%Y-%m-%d %H:%M")),
        ]);
    }
    println!("{}", table);
    Ok(())
}

fn run_tokens_revoke(prefix: &str, json: bool) -> Result<()> {
    use treeline_core::services::oauth::OAuthStore;
    let treeline_dir = get_treeline_dir();
    let store = OAuthStore::new(treeline_dir);

    let n = store
        .revoke_access_token_by_prefix(prefix)
        .context("Failed to revoke token")?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({ "revoked": n, "prefix": prefix })
            )?
        );
        return Ok(());
    }

    if n == 0 {
        eprintln!("No token matched prefix '{}'.", prefix);
    } else if n == 1 {
        eprintln!("{} Revoked 1 token.", "✓".green());
    } else {
        eprintln!(
            "{} Revoked {} tokens matching prefix '{}'.",
            "✓".green(),
            n,
            prefix
        );
    }
    Ok(())
}

fn run_link(url: &str, name_override: Option<&str>) -> Result<()> {
    let treeline_dir = get_treeline_dir();
    let device_name = name_override
        .map(|s| s.to_string())
        .unwrap_or_else(default_device_name);

    let flow = DeviceCodeLink::start(url, &device_name)?;

    eprintln!();
    eprintln!("Open this URL to sign in:");
    eprintln!();
    eprintln!("  {}", flow.verification_uri_complete);
    eprintln!();
    if !flow.user_code.is_empty() {
        eprintln!("Code: {}", flow.user_code.bold());
        eprintln!();
    }
    eprintln!("Waiting...");

    loop {
        std::thread::sleep(Duration::from_secs(flow.interval));

        match flow.poll(&treeline_dir)? {
            DeviceCodeLinkOutcome::Pending => continue,
            DeviceCodeLinkOutcome::SlowDown => {
                std::thread::sleep(Duration::from_secs(flow.interval));
                continue;
            }
            DeviceCodeLinkOutcome::Linked { device_name, .. } => {
                eprintln!("{} Linked as \"{}\"", "✓".green(), device_name);
                return Ok(());
            }
            DeviceCodeLinkOutcome::Expired => {
                anyhow::bail!("Device code expired before authorization completed.")
            }
            DeviceCodeLinkOutcome::Denied => anyhow::bail!("Authorization was denied."),
        }
    }
}

/// OS hostname (best-effort), or "Treeline device" as a final fallback.
fn default_device_name() -> String {
    if let Ok(host) = std::env::var("HOST").or_else(|_| std::env::var("HOSTNAME")) {
        if !host.is_empty() {
            return host;
        }
    }
    if let Ok(output) = std::process::Command::new("hostname").output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "Treeline device".to_string()
}

fn run_unlink() -> Result<()> {
    let treeline_dir = get_treeline_dir();

    if HubConfig::load(&treeline_dir)?.is_none() {
        eprintln!("Not linked to any hub.");
        return Ok(());
    }

    HubConfig::remove(&treeline_dir)?;

    eprintln!("{} Unlinked from hub", "✓".green());
    Ok(())
}

fn run_push(json: bool, force: bool) -> Result<()> {
    let treeline_dir = get_treeline_dir();
    let ctx = get_context()?;
    let client = HubClient::new(treeline_dir.clone());

    if !json {
        eprintln!("Pushing to hub...");
    }

    let outcome = client.push(&ctx, force)?;
    let hub_url = HubConfig::load(&treeline_dir)?
        .map(|h| h.url)
        .unwrap_or_default();

    match outcome {
        PushOutcome::Pushed { bytes, hash } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "status": "ok",
                    "bytes_uploaded": bytes,
                    "hash": hash,
                }))?);
            } else {
                eprintln!("{} Pushed {} to {}", "✓".green(), format_bytes(bytes), hub_url);
            }
        }
        PushOutcome::AutoMerged { bytes, hash } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "status": "auto_merged",
                    "bytes_uploaded": bytes,
                    "hash": hash,
                }))?);
            } else {
                eprintln!(
                    "{} Hub had diverged — auto-merged and pushed {} to {}",
                    "✓".green(),
                    format_bytes(bytes),
                    hub_url
                );
            }
        }
        PushOutcome::Conflict { hub_hash, conflicts } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "status": "conflict",
                    "hub_hash": hub_hash,
                    "conflicts": conflicts,
                }))?);
            } else {
                print_conflicts(&conflicts);
                eprintln!();
                eprintln!("Resolve by choosing one version:");
                eprintln!("  tl hub push --force   (overwrite hub with your local version)");
                eprintln!("  tl hub pull           (overwrite local with hub's version)");
            }
        }
        PushOutcome::NoBaseSnapshot { hub_hash } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "status": "no_base_snapshot",
                    "hub_hash": hub_hash,
                }))?);
            } else {
                eprintln!(
                    "{} Hub has changed since your last sync, but there's no base snapshot for a three-way merge.",
                    "Conflict!".yellow().bold()
                );
                eprintln!("Choose one version:");
                eprintln!("  tl hub push --force   (overwrite hub with your version)");
                eprintln!("  tl hub pull           (overwrite local with hub's version)");
            }
        }
        PushOutcome::NoChanges => {
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "status": "no_changes",
                }))?);
            } else {
                eprintln!("{} Already up to date.", "✓".green());
            }
        }
    }

    Ok(())
}

fn run_pull(json: bool) -> Result<()> {
    let treeline_dir = get_treeline_dir();
    let client = HubClient::new(treeline_dir.clone());

    if !json {
        eprintln!("Pulling from hub...");
    }

    let PullOutcome { bytes, .. } = client.pull()?;
    let hub_url = HubConfig::load(&treeline_dir)?
        .map(|h| h.url)
        .unwrap_or_default();

    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "status": "ok",
            "bytes_downloaded": bytes,
        }))?);
    } else {
        eprintln!("{} Pulled {} from {}", "✓".green(), format_bytes(bytes), hub_url);
    }

    Ok(())
}

fn run_status(json: bool) -> Result<()> {
    let treeline_dir = get_treeline_dir();

    match HubConfig::load(&treeline_dir)? {
        None => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "linked": false,
                    }))?
                );
            } else {
                eprintln!("Not linked to any hub.");
                eprintln!("Run 'tl hub link --url <url>' to connect.");
            }
        }
        Some(hub) => {
            // Check if hub is reachable
            let reachable = reqwest::blocking::Client::new()
                .get(format!("{}/health", hub.url))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .map(|r| r.status().is_success())
                .unwrap_or(false);

            if json {
                let result = serde_json::json!({
                    "linked": true,
                    "url": hub.url,
                    "reachable": reachable,
                    "last_push": hub.last_push,
                    "last_pull": hub.last_pull,
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                eprintln!("{} {}", "Hub:".bold(), hub.url);
                eprintln!(
                    "{} {}",
                    "Status:".bold(),
                    if reachable {
                        "reachable".green().to_string()
                    } else {
                        "unreachable".red().to_string()
                    }
                );
                eprintln!(
                    "{} {}",
                    "Last push:".bold(),
                    hub.last_push
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| "never".to_string())
                );
                eprintln!(
                    "{} {}",
                    "Last pull:".bold(),
                    hub.last_pull
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| "never".to_string())
                );
            }
        }
    }

    Ok(())
}

fn run_watch(debounce_secs: u64, poll_secs: u64) -> Result<()> {
    let treeline_dir = get_treeline_dir();
    let ctx = get_context().context("Failed to unlock database")?;
    let client = HubClient::new(treeline_dir);

    let opts = WatchOptions {
        debounce: Duration::from_secs(debounce_secs),
        poll: Duration::from_secs(poll_secs),
    };

    // Ctrl+C terminates the process; the watch loop's stop flag is here for
    // the desktop integration (which signals stop on app quit).
    let stop = Arc::new(AtomicBool::new(false));

    eprintln!(
        "Watching for changes (debounce: {}s, poll: {}s). Press Ctrl+C to stop.",
        debounce_secs, poll_secs
    );

    let mut observer = StderrWatchObserver;
    client.watch(&ctx, opts, &mut observer, stop)
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn print_conflicts(conflicts: &[ConflictDescription]) {
    eprintln!(
        "{} {} conflicting changes:",
        "Conflicts:".red().bold(),
        conflicts.len()
    );
    for c in conflicts {
        let kind_label = match c.kind {
            ConflictKind::ModifiedSameColumns => "modified same columns",
            ConflictKind::BothAdded => "both sides added",
            ConflictKind::DeletedVsModified => "deleted vs. modified",
        };
        eprintln!("  {} ({}): {}", c.table.bold(), kind_label, c.detail);
    }
}

/// Watch observer that prints events to stderr in a format consistent with
/// the rest of the CLI (one-line `[watch] ...` updates).
struct StderrWatchObserver;

impl WatchObserver for StderrWatchObserver {
    fn on_event(&mut self, event: WatchEvent) {
        match event {
            WatchEvent::Started { hub_url } => {
                eprintln!("[watch] Hub: {}", hub_url);
            }
            WatchEvent::LocalChangeDetected => {
                eprintln!("[watch] Local change detected, debouncing...");
            }
            WatchEvent::Pushing => {
                eprintln!("[watch] Pushing...");
            }
            WatchEvent::Pushed { bytes } => {
                eprintln!("[watch] {} Pushed {}", "✓".green(), format_bytes(bytes));
            }
            WatchEvent::AutoMerged { bytes } => {
                eprintln!(
                    "[watch] {} Auto-merged and pushed {}",
                    "✓".green(),
                    format_bytes(bytes)
                );
            }
            WatchEvent::Pulling => {
                eprintln!("[watch] Hub changed, pulling...");
            }
            WatchEvent::Pulled { bytes } => {
                eprintln!("[watch] {} Pulled {}", "✓".green(), format_bytes(bytes));
            }
            WatchEvent::Conflict { hub_hash, conflicts } => {
                eprintln!(
                    "[watch] {} {} conflicts vs hub hash {}. Resolve via 'tl hub push --force' or 'tl hub pull'.",
                    "Conflict:".red().bold(),
                    conflicts,
                    &hub_hash[..hub_hash.len().min(12)]
                );
            }
            WatchEvent::NoBaseSnapshot { .. } => {
                eprintln!(
                    "[watch] {} Hub diverged but no base snapshot. Resolve via 'tl hub push --force' or 'tl hub pull'.",
                    "Conflict:".red().bold()
                );
            }
            WatchEvent::Error { message } => {
                eprintln!("[watch] {} {}", "Error:".red(), message);
            }
            WatchEvent::Stopped => {
                eprintln!("[watch] Stopped.");
            }
        }
    }
}
