//! Hub commands - link, unlink, push, pull, status
//!
//! Manages remote hub connections for database sync.

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;

use treeline_core::config::HubConfig;

use super::{get_context, get_treeline_dir};

use diffy_duck::{DatabaseConfig, Diff3ChangeOrigin, Diff3RowChange, DiffOptions, Merge3Strategy};

#[derive(Subcommand)]
pub enum HubCommands {
    /// Link this device to a hub.
    ///
    /// Uses the device-code OAuth flow: prints a verification URL that you
    /// open in a browser, paste your master token there, and the CLI
    /// finishes automatically. The master never leaves the hub or your
    /// browser; this device gets its own scoped device token.
    Link {
        /// Hub URL (e.g. https://my-hub.example.com). Required for now;
        /// in the future will default to the Treeline Cloud URL.
        #[arg(long)]
        url: String,
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

pub fn run(command: HubCommands) -> Result<()> {
    match command {
        HubCommands::Link { url, name } => run_link(&url, name.as_deref()),
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
    let url = url.trim_end_matches('/').to_string();

    let device_name = name_override
        .map(|s| s.to_string())
        .unwrap_or_else(default_device_name);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("Failed to construct HTTP client")?;

    // Sanity-check that the hub is reachable before we ask it for a code.
    let resp = client
        .get(format!("{}/health", url))
        .send()
        .with_context(|| format!("Failed to reach hub at {}. Is it running?", url))?;
    if !resp.status().is_success() {
        anyhow::bail!("Hub health check failed with status {}", resp.status());
    }

    // Step 1: ask the hub for a device code.
    let resp = client
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

    let body: serde_json::Value = resp.json().context("Failed to parse device code response")?;
    let device_code = body["device_code"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Hub response missing device_code"))?
        .to_string();
    let user_code = body["user_code"].as_str().unwrap_or("").to_string();
    let verification_uri_complete = body["verification_uri_complete"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let interval = body["interval"].as_i64().unwrap_or(2).max(1) as u64;
    let expires_in = body["expires_in"].as_i64().unwrap_or(600).max(60) as u64;

    eprintln!();
    eprintln!("Open this URL to sign in:");
    eprintln!();
    eprintln!("  {}", verification_uri_complete);
    eprintln!();
    if !user_code.is_empty() {
        eprintln!("Code: {}", user_code.bold());
        eprintln!();
    }
    eprintln!("Waiting...");

    // Step 2: poll /token until the user completes the browser-side authorize
    // step (or the device code expires).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(expires_in);
    let pair = loop {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("Device code expired before authorization completed.");
        }

        std::thread::sleep(std::time::Duration::from_secs(interval));

        let resp = client
            .post(format!("{}/token", url))
            .form(&[
                ("grant_type", "device_code"),
                ("device_code", device_code.as_str()),
            ])
            .send()
            .context("Failed to poll /token")?;

        if resp.status().is_success() {
            break resp
                .json::<serde_json::Value>()
                .context("Failed to parse token response")?;
        }

        // RFC 8628: 400 with `error` of authorization_pending / slow_down /
        // expired_token / access_denied / invalid_grant. Anything else is
        // a real failure.
        let body: serde_json::Value = resp.json().unwrap_or_default();
        let err = body["error"].as_str().unwrap_or("");
        match err {
            "authorization_pending" => continue,
            "slow_down" => {
                std::thread::sleep(std::time::Duration::from_secs(interval));
                continue;
            }
            "expired_token" => anyhow::bail!("Device code expired before authorization completed."),
            "access_denied" => anyhow::bail!("Authorization was denied."),
            other => anyhow::bail!("Token endpoint returned error: {}", other),
        }
    };

    let access_token = pair["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Token response missing access_token"))?
        .to_string();
    let refresh_token = pair["refresh_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Token response missing refresh_token"))?
        .to_string();

    let hub = HubConfig {
        url: url.clone(),
        access_token,
        refresh_token,
        device_name: device_name.clone(),
        last_push: None,
        last_pull: None,
        base_hash: None,
    };
    hub.save(&treeline_dir)?;

    eprintln!("{} Linked as \"{}\"", "✓".green(), device_name);
    Ok(())
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

/// Refresh `hub.access_token` using its `refresh_token` and persist. Returns
/// the new access token. Used after a 401 from /api/* to recover without
/// asking the user to re-link.
fn refresh_access_token(
    client: &reqwest::blocking::Client,
    treeline_dir: &std::path::Path,
    hub: &mut HubConfig,
) -> Result<()> {
    let resp = client
        .post(format!("{}/token", hub.url))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", hub.refresh_token.as_str()),
        ])
        .timeout(std::time::Duration::from_secs(15))
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
    hub.save(treeline_dir)?;
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

fn run_push(json: bool, force: bool) -> Result<()> {
    let treeline_dir = get_treeline_dir();

    let mut hub = HubConfig::load(&treeline_dir)?.ok_or_else(|| {
        anyhow::anyhow!("Not linked to a hub. Run 'tl hub link --url <url>' first.")
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
    let mut push_url = format!("{}/api/push", hub.url);
    if !force {
        if let Some(ref base_hash) = hub.base_hash {
            push_url = format!("{}?base_hash={}", push_url, base_hash);
        }
    }

    let client = reqwest::blocking::Client::new();

    // The bundle body has to be sent twice if a 401 hits and we refresh —
    // reqwest consumes the body on `.send()`. Hold onto a clone for retry.
    let send_push = |access_token: &str, body: Vec<u8>| {
        client
            .post(&push_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .timeout(std::time::Duration::from_secs(300))
            .send()
    };

    let mut resp = send_push(&hub.access_token, bundle.clone())
        .context("Failed to connect to hub")?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        refresh_access_token(&client, &treeline_dir, &mut hub)?;
        resp = send_push(&hub.access_token, bundle)
            .context("Failed to connect to hub")?;
    }

    let status = resp.status();
    let body: serde_json::Value = resp.json().context("Failed to parse hub response")?;

    if status == reqwest::StatusCode::CONFLICT {
        if json {
            let hub_hash = body["hub_hash"].as_str().unwrap_or("unknown");
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "status": "conflict",
                "hub_hash": hub_hash,
                "base_hash": hub.base_hash,
            }))?);
            return Ok(());
        }

        eprintln!(
            "{} Hub has changed since your last sync. Attempting to merge...",
            "Conflict!".yellow().bold()
        );

        // Check for base snapshot (needed for three-way merge)
        let base_db_path = treeline_dir.join(".treeline.base.duckdb");
        if !base_db_path.exists() {
            eprintln!("No base snapshot available for three-way merge.");
            eprintln!("Choose one version:");
            eprintln!("  tl hub push --force   (overwrite hub with your version)");
            eprintln!("  tl hub pull           (overwrite local with hub's version)");
            return Ok(());
        }

        // Download the hub's current bundle
        let pull_resp = client
            .get(format!("{}/api/pull", hub.url))
            .header("Authorization", format!("Bearer {}", hub.access_token))
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .context("Failed to download hub bundle for merge")?;

        if !pull_resp.status().is_success() {
            anyhow::bail!("Failed to download hub bundle: {}", pull_resp.status());
        }

        let hub_bundle = pull_resp.bytes()?;

        // Extract hub bundle to a temp directory
        let hub_temp = tempfile::TempDir::new().context("Failed to create temp dir")?;
        treeline_core::services::hub::SyncBundle::extract(&hub_bundle, hub_temp.path())?;

        let hub_db_path = hub_temp.path().join("treeline.duckdb");
        if !hub_db_path.exists() {
            anyhow::bail!("Hub bundle does not contain a database");
        }

        // Three-way diff: ancestor (base) vs local vs hub
        // All three databases share the same encryption key
        let local_db_path = treeline_dir.join("treeline.duckdb");
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
        ).map_err(|e| anyhow::anyhow!("Failed to diff databases: {}", e))?;

        let has_conflicts = diff3_report.summary.total_conflicts > 0;
        let total_changes = diff3_report.summary.total_non_conflicting
            + diff3_report.summary.total_conflicts;
        let has_new_tables = !diff3_report.a_only_tables.is_empty()
            || !diff3_report.b_only_tables.is_empty();

        if total_changes == 0 && !has_new_tables {
            eprintln!("{} Databases are identical. Nothing to push.", "✓".green());
            return Ok(());
        }

        // Show diff summary
        eprintln!();
        for table_diff in &diff3_report.tables {
            let s = &table_diff.summary;
            if s.total_changes() == 0 {
                continue;
            }
            eprintln!(
                "  {} — local: +{} -{} ~{}  hub: +{} -{} ~{}  conflicts: {}",
                table_diff.table.to_string().bold(),
                s.added_a, s.removed_a, s.modified_a,
                s.added_b, s.removed_b, s.modified_b,
                s.conflicts,
            );
        }
        eprintln!();

        // Show new tables
        for t in &diff3_report.a_only_tables {
            eprintln!("  {} — new table (local only)", t.to_string().bold());
        }
        for t in &diff3_report.b_only_tables {
            eprintln!("  {} — new table (hub only)", t.to_string().bold());
        }

        if !has_conflicts {
            // No conflicting rows — safe to auto-merge
            eprintln!("No conflicts. Auto-merging...");

            // Copy base to a temp file for merge (merge3 modifies ancestor in place)
            let merge_temp = tempfile::TempDir::new()?;
            let merge_db_path = merge_temp.path().join("merged.duckdb");
            std::fs::copy(&base_db_path, &merge_db_path)?;

            let mut merge_ancestor = DatabaseConfig::new(merge_db_path.to_string_lossy());
            if let Some(ref key) = encryption_key {
                merge_ancestor = merge_ancestor.with_key(key);
            }

            diffy_duck::merge3(
                &merge_ancestor, &local_config, &hub_config, &Merge3Strategy::FailOnConflict,
            ).map_err(|e| anyhow::anyhow!("Failed to merge: {}", e))?;

            // Replace local database with the merged result
            std::fs::copy(&merge_db_path, &local_db_path)?;

            eprintln!("{} Merged successfully. Pushing...", "✓".green());

            // Rebuild bundle and push (no base_hash — force after merge)
            let merged_bundle = treeline_core::services::hub::SyncBundle::create(&treeline_dir)?;
            let size = merged_bundle.len();

            let resp = client
                .post(format!("{}/api/push", hub.url))
                .header("Authorization", format!("Bearer {}", hub.access_token))
                .header("Content-Type", "application/octet-stream")
                .body(merged_bundle)
                .timeout(std::time::Duration::from_secs(300))
                .send()
                .context("Failed to push merged database")?;

            if !resp.status().is_success() {
                let body = resp.text().unwrap_or_default();
                anyhow::bail!("Push after merge failed: {}", body);
            }

            let body: serde_json::Value = resp.json().unwrap_or_default();
            let new_hash = body["hash"].as_str().map(|s| s.to_string());
            let mut hub = HubConfig::load(&treeline_dir)?.unwrap();
            hub.last_push = Some(chrono::Utc::now());
            hub.base_hash = new_hash;
            hub.save(&treeline_dir)?;

            save_base_snapshot(&treeline_dir)?;

            eprintln!("{} Pushed {} to {}", "✓".green(), format_bytes(size as u64), hub.url);
            return Ok(());
        }

        // Has conflicts — show them and ask user
        eprintln!(
            "{} {} conflicting changes found:",
            "Conflicts:".red().bold(),
            diff3_report.summary.total_conflicts,
        );
        for table_diff in &diff3_report.tables {
            for change in &table_diff.changes {
                match change {
                    Diff3RowChange::Modified { key, column_changes } => {
                        let conflict_cols: Vec<_> = column_changes.iter()
                            .filter(|c| c.origin == Diff3ChangeOrigin::Conflict)
                            .collect();
                        if conflict_cols.is_empty() {
                            continue;
                        }
                        eprintln!("  {} (table: {})", "Row:".bold(), table_diff.table);
                        for c in conflict_cols {
                            eprintln!(
                                "    {} — local: {}, hub: {}",
                                c.column.bold(), c.a_value, c.b_value,
                            );
                        }
                    }
                    Diff3RowChange::Added { origin: Diff3ChangeOrigin::Conflict, row, other_row, .. } => {
                        eprintln!("  {} Both sides added a row with the same key (table: {})", "Row:".bold(), table_diff.table);
                        eprintln!("    local: {:?}", row);
                        if let Some(other) = other_row {
                            eprintln!("    hub:   {:?}", other);
                        }
                    }
                    Diff3RowChange::Removed { origin: Diff3ChangeOrigin::Conflict, .. } => {
                        eprintln!("  {} One side deleted, other modified (table: {})", "Row:".bold(), table_diff.table);
                    }
                    _ => {}
                }
            }
        }
        eprintln!();
        eprintln!("Resolve conflicts by choosing one version:");
        eprintln!("  tl hub push --force   (overwrite hub with your local version)");
        eprintln!("  tl hub pull           (overwrite local with hub's version)");

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

    // Save base snapshot for future three-way merge
    save_base_snapshot(&treeline_dir)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        eprintln!("{} Pushed {} to {}", "✓".green(), format_bytes(size as u64), hub.url);
    }

    Ok(())
}

fn run_pull(json: bool) -> Result<()> {
    let treeline_dir = get_treeline_dir();

    let mut hub = HubConfig::load(&treeline_dir)?.ok_or_else(|| {
        anyhow::anyhow!("Not linked to a hub. Run 'tl hub link --url <url>' first.")
    })?;

    if !json {
        eprintln!("Downloading from hub...");
    }

    // Download from hub
    let client = reqwest::blocking::Client::new();
    let pull_url = format!("{}/api/pull", hub.url);
    let send_pull = |access_token: &str| {
        client
            .get(&pull_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .timeout(std::time::Duration::from_secs(300))
            .send()
    };

    let mut resp = send_pull(&hub.access_token).context("Failed to connect to hub")?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        refresh_access_token(&client, &treeline_dir, &mut hub)?;
        resp = send_pull(&hub.access_token).context("Failed to connect to hub")?;
    }

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

    // Save base snapshot for future three-way merge
    save_base_snapshot(&treeline_dir)?;

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

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Save a snapshot of the current database as the base for three-way merge.
/// Called after every successful push or pull.
fn save_base_snapshot(treeline_dir: &std::path::Path) -> Result<()> {
    let db_path = treeline_dir.join("treeline.duckdb");
    let base_path = treeline_dir.join(".treeline.base.duckdb");
    if db_path.exists() {
        std::fs::copy(&db_path, &base_path)
            .context("Failed to save base snapshot")?;
    }
    Ok(())
}

/// Watch for local database changes and auto-sync with the hub.
///
/// Uses file mtime to detect changes. Debounces rapid edits.
/// Also polls the hub for incoming changes.
fn run_watch(debounce_secs: u64, poll_secs: u64) -> Result<()> {
    let treeline_dir = get_treeline_dir();

    let hub = HubConfig::load(&treeline_dir)?.ok_or_else(|| {
        anyhow::anyhow!("Not linked to a hub. Run 'tl hub link --url <url>' first.")
    })?;

    // Unlock the database up front so the watch loop doesn't stall on the
    // first change with an unexpected password prompt. If the DB is encrypted,
    // this prompts once; the derived key is cached in the keychain so the
    // subsequent get_context() inside run_push() succeeds silently.
    let _unlock = get_context().context("Failed to unlock database")?;
    drop(_unlock);

    eprintln!("Watching for changes (debounce: {}s, poll: {}s)", debounce_secs, poll_secs);
    eprintln!("Hub: {}", hub.url);
    eprintln!("Press Ctrl+C to stop.");

    let db_path = treeline_dir.join("treeline.duckdb");
    let debounce = std::time::Duration::from_secs(debounce_secs);
    let poll_interval = std::time::Duration::from_secs(poll_secs);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_default();

    let mut last_mtime = db_path
        .metadata()
        .and_then(|m| m.modified())
        .ok();
    let mut last_poll = std::time::Instant::now();

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Check for local changes via mtime
        let current_mtime = db_path
            .metadata()
            .and_then(|m| m.modified())
            .ok();

        if current_mtime != last_mtime {
            // File changed — debounce
            last_mtime = current_mtime;
            eprintln!("[watch] Change detected, waiting {}s...", debounce_secs);

            // Keep checking until stable
            loop {
                std::thread::sleep(debounce);
                let new_mtime = db_path
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok();
                if new_mtime == last_mtime {
                    break; // Stable — ready to push
                }
                last_mtime = new_mtime;
            }

            // Check if the file actually differs from our base
            let hub_config = HubConfig::load(&treeline_dir)?.unwrap();
            if let Some(ref base_hash) = hub_config.base_hash {
                if let Ok(current_hash) = treeline_core::services::compute_file_hash(&db_path) {
                    if &current_hash == base_hash {
                        continue; // No actual content change
                    }
                }
            }

            eprintln!("[watch] Pushing...");
            match run_push(false, false) {
                Ok(()) => {}
                Err(e) => eprintln!("[watch] Push failed: {}", e),
            }
            last_poll = std::time::Instant::now();
            // Update mtime after push/merge to avoid re-triggering
            last_mtime = db_path.metadata().and_then(|m| m.modified()).ok();
            continue;
        }

        // Poll hub for incoming changes
        if last_poll.elapsed() >= poll_interval {
            last_poll = std::time::Instant::now();

            let hub_config = match HubConfig::load(&treeline_dir)? {
                Some(h) => h,
                None => continue,
            };

            // Get hub hash
            let hub_hash = client
                .get(format!("{}/api/hash", hub_config.url))
                .header("Authorization", format!("Bearer {}", hub_config.access_token))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .ok()
                .filter(|r| r.status().is_success())
                .and_then(|r| r.json::<serde_json::Value>().ok())
                .and_then(|b| b["hash"].as_str().map(|s| s.to_string()));

            let needs_pull = match (hub_hash.as_deref(), hub_config.base_hash.as_deref()) {
                (Some(hub), Some(base)) => hub != base,
                (Some(_), None) => true,
                _ => false,
            };

            if needs_pull {
                // Check if we also have local changes
                let has_local_changes = if let Some(ref base_hash) = hub_config.base_hash {
                    treeline_core::services::compute_file_hash(&db_path)
                        .map(|h| &h != base_hash)
                        .unwrap_or(false)
                } else {
                    false
                };

                if has_local_changes {
                    // Both sides changed — push will trigger three-way merge
                    eprintln!("[watch] Hub and local both changed, merging...");
                    match run_push(false, false) {
                        Ok(()) => {}
                        Err(e) => eprintln!("[watch] Merge/push failed: {}", e),
                    }
                } else {
                    // Only hub changed — safe to pull
                    eprintln!("[watch] Hub changed, pulling...");
                    match run_pull(false) {
                        Ok(()) => {}
                        Err(e) => eprintln!("[watch] Pull failed: {}", e),
                    }
                }
                // Update mtime so we don't re-trigger on the file we just wrote
                last_mtime = db_path
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok();
            }
        }
    }
}
