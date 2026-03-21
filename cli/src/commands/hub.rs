//! Hub commands - link, unlink, push, pull, status
//!
//! Manages remote hub connections for database sync.

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;

use treeline_core::config::HubConfig;

use super::{get_context, get_treeline_dir};

#[derive(Subcommand)]
pub enum HubCommands {
    /// Link to a remote hub
    Link {
        /// Hub URL (e.g., http://localhost:4242)
        url: String,
        /// Auth token
        #[arg(long)]
        token: String,
    },

    /// Unlink from the remote hub
    Unlink,

    /// Push local database to the hub
    Push {
        /// Output as JSON
        #[arg(long)]
        json: bool,
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
}

pub fn run(command: HubCommands) -> Result<()> {
    match command {
        HubCommands::Link { url, token } => run_link(&url, &token),
        HubCommands::Unlink => run_unlink(),
        HubCommands::Push { json } => run_push(json),
        HubCommands::Pull { json } => run_pull(json),
        HubCommands::Status { json } => run_status(json),
    }
}

fn run_link(url: &str, token: &str) -> Result<()> {
    let treeline_dir = get_treeline_dir();

    // Normalize URL: remove trailing slash
    let url = url.trim_end_matches('/').to_string();

    // Test the connection before saving
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(format!("{}/health", url))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .context("Failed to connect to hub. Is it running?")?;

    if !resp.status().is_success() {
        anyhow::bail!("Hub health check failed with status {}", resp.status());
    }

    let hub = HubConfig {
        url: url.clone(),
        token: token.to_string(),
        last_push: None,
        last_pull: None,
        base_hash: None,
    };
    hub.save(&treeline_dir)?;

    eprintln!("{} Linked to {}", "✓".green(), url);
    Ok(())
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

fn run_push(json: bool) -> Result<()> {
    let treeline_dir = get_treeline_dir();

    let hub = HubConfig::load(&treeline_dir)?.ok_or_else(|| {
        anyhow::anyhow!("Not linked to a hub. Run 'tl hub link <url> --token <token>' first.")
    })?;

    // Compact first
    if !json {
        eprintln!("Compacting database...");
    }
    let ctx = get_context()?;
    let compact_result = ctx.compact_service.compact()?;

    if !json {
        eprintln!(
            "Compacted: {} → {}",
            format_bytes(compact_result.original_size),
            format_bytes(compact_result.compacted_size)
        );
        eprintln!("Creating bundle...");
    }

    // Checkpoint to flush WAL
    ctx.repository.checkpoint()?;

    // Build the sync bundle
    let bundle = treeline_core::services::hub::SyncBundle::create(&treeline_dir)?;
    let size = bundle.len();

    if !json {
        eprintln!("Uploading {} to hub...", format_bytes(size as u64));
    }

    // Upload to hub with base_hash for conflict detection
    let client = reqwest::blocking::Client::new();
    let mut req = client
        .post(format!("{}/api/push", hub.url))
        .header("Authorization", format!("Bearer {}", hub.token))
        .header("Content-Type", "application/octet-stream");

    if let Some(ref base_hash) = hub.base_hash {
        req = req.header("X-Treeline-Base-Hash", base_hash.as_str());
    }

    let resp = req
        .body(bundle)
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .context("Failed to connect to hub")?;

    let status = resp.status();
    let body: serde_json::Value = resp.json().context("Failed to parse hub response")?;

    if status == reqwest::StatusCode::CONFLICT {
        let hub_hash = body["hub_hash"].as_str().unwrap_or("unknown");
        if !json {
            eprintln!(
                "{} Hub has changed since your last sync.",
                "Conflict!".red().bold()
            );
            eprintln!("Run 'tl hub pull' to get the latest, then push again.");
            eprintln!("(In a future version, this will offer to merge automatically.)");
        } else {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "status": "conflict",
                "hub_hash": hub_hash,
                "base_hash": hub.base_hash,
            }))?);
        }
        // TODO: when diffy-duck is integrated, offer to pull + diff + merge here
        return Ok(());
    }

    if !status.is_success() {
        let error = body["error"].as_str().unwrap_or("Unknown error");
        anyhow::bail!("Push failed: {}", error);
    }

    // Update hub config with new hash and timestamp
    let new_hash = body["hash"].as_str().map(|s| s.to_string());
    let mut hub = HubConfig::load(&treeline_dir)?.unwrap();
    hub.last_push = Some(chrono::Utc::now());
    hub.base_hash = new_hash;
    hub.save(&treeline_dir)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        eprintln!("{} Pushed {} to {}", "✓".green(), format_bytes(size as u64), hub.url);
    }

    Ok(())
}

fn run_pull(json: bool) -> Result<()> {
    let treeline_dir = get_treeline_dir();

    let hub = HubConfig::load(&treeline_dir)?.ok_or_else(|| {
        anyhow::anyhow!("Not linked to a hub. Run 'tl hub link <url> --token <token>' first.")
    })?;

    if !json {
        eprintln!("Downloading from hub...");
    }

    // Download from hub
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(format!("{}/api/pull", hub.url))
        .header("Authorization", format!("Bearer {}", hub.token))
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .context("Failed to connect to hub")?;

    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        anyhow::bail!("Pull failed: {}", body);
    }

    let bytes = resp.bytes()?;
    let size = bytes.len();

    if !json {
        eprintln!("Creating backup...");
    }

    // Create backup of current state before replacing
    let db_path = treeline_dir.join("treeline.duckdb");
    if db_path.exists() {
        let backup_service = treeline_core::services::BackupService::new(
            treeline_dir.clone(),
            "treeline.duckdb".to_string(),
        );
        backup_service.create(Some(20))?;
    }

    // Extract the bundle
    if !json {
        eprintln!("Extracting bundle...");
    }
    treeline_core::services::hub::SyncBundle::extract(&bytes, &treeline_dir)?;

    // Compute hash of the pulled database — this is now our base
    let db_path = treeline_dir.join("treeline.duckdb");
    let new_hash = if db_path.exists() {
        Some(treeline_core::services::compute_file_hash(&db_path)?)
    } else {
        None
    };

    // Update hub config with hash and timestamp
    let mut hub = HubConfig::load(&treeline_dir)?.unwrap();
    hub.last_pull = Some(chrono::Utc::now());
    hub.base_hash = new_hash;
    hub.save(&treeline_dir)?;

    if json {
        let result = serde_json::json!({
            "status": "ok",
            "bytes_downloaded": size,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        eprintln!("{} Pulled {} from {}", "✓".green(), format_bytes(size as u64), hub.url);
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
                eprintln!("Run 'tl hub link <url> --token <token>' to connect.");
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

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
