//! Encrypt/Decrypt commands - manage database encryption

use std::env;

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use dialoguer::{Confirm, Password};
use treeline_core::LogEvent;

use super::{get_logger, log_event};
use treeline_core::config::Config;
use treeline_core::services::{BackupService, EncryptionService, KeychainService};

/// Get password from --password flag, TREELINE_PASSWORD env var, or prompt
fn get_password_or_prompt(password_flag: Option<String>, prompt: &str) -> Result<String> {
    // 1. Check --password flag first
    if let Some(p) = password_flag {
        return Ok(p);
    }

    // 2. Check TREELINE_PASSWORD environment variable
    if let Ok(p) = env::var("TREELINE_PASSWORD") {
        return Ok(p);
    }

    // 3. Prompt interactively
    let p = Password::new().with_prompt(prompt).interact()?;
    Ok(p)
}

/// Get password with confirmation for encryption
fn get_password_with_confirm(password_flag: Option<String>) -> Result<String> {
    // 1. Check --password flag first
    if let Some(p) = password_flag {
        return Ok(p);
    }

    // 2. Check TREELINE_PASSWORD environment variable
    if let Ok(p) = env::var("TREELINE_PASSWORD") {
        return Ok(p);
    }

    // 3. Prompt interactively with confirmation
    let p1 = Password::new()
        .with_prompt("Enter encryption password")
        .interact()?;
    let p2 = Password::new()
        .with_prompt("Confirm encryption password")
        .interact()?;

    if p1 != p2 {
        anyhow::bail!("Passwords do not match");
    }
    Ok(p1)
}

#[derive(Subcommand)]
pub enum EncryptCommands {
    /// Show encryption status
    Status,
    /// Store encryption key in OS keychain (unlocks for all apps)
    Unlock {
        /// Password for key derivation
        #[arg(short, long)]
        password: Option<String>,
    },
    /// Clear encryption key from OS keychain (requires re-entry next time)
    Lock,
}

pub fn run(command: Option<EncryptCommands>, password: Option<String>, json: bool) -> Result<()> {
    let treeline_dir = super::get_treeline_dir();
    let config = Config::load(&treeline_dir)?;

    // Determine database path (always use treeline.duckdb for encryption, not demo.duckdb)
    let db_path = treeline_dir.join("treeline.duckdb");
    let encryption_service = EncryptionService::new(treeline_dir.clone(), db_path);

    // Check demo mode for encryption operations (not status/lock/unlock)
    if command.is_none() {
        if config.demo_mode {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"error": "Cannot encrypt demo database"})
                );
            } else {
                eprintln!("{}", "Cannot encrypt demo database".red());
                eprintln!(
                    "{}",
                    "Demo mode uses a separate, unencrypted database".dimmed()
                );
            }
            std::process::exit(1);
        }
    }

    match command {
        Some(EncryptCommands::Status) => {
            let mut status = encryption_service.get_status()?;

            // Enrich with keychain info
            let keychain_available = KeychainService::is_available();
            status.keychain_available = Some(keychain_available);

            if status.encrypted {
                // locked = encrypted AND no key in keychain AND no env vars set
                let has_env_key =
                    env::var("TL_DB_KEY").is_ok() || env::var("TL_DB_PASSWORD").is_ok();
                let has_keychain_key = KeychainService::get_key().unwrap_or(None).is_some();
                status.locked = Some(!has_env_key && !has_keychain_key);
            } else {
                status.locked = Some(false);
            }

            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                if status.encrypted {
                    println!("{}", "Database is encrypted".green());
                    if status.locked == Some(true) {
                        println!("  Status: {}", "locked".red());
                        println!(
                            "  {}",
                            "Run 'tl encrypt unlock' to store key in keychain".dimmed()
                        );
                    } else {
                        println!("  Status: {}", "unlocked".green());
                    }
                } else {
                    println!("{}", "Database is not encrypted".yellow());
                }
                if keychain_available {
                    println!("  Keychain: {}", "available".green());
                } else {
                    println!("  Keychain: {}", "unavailable".yellow());
                }
            }
        }
        Some(EncryptCommands::Unlock { password }) => {
            if !encryption_service.is_encrypted()? {
                anyhow::bail!("Database is not encrypted. Nothing to unlock.");
            }

            let pwd = get_password_or_prompt(password, "Enter encryption password")?;

            // Derive key
            let key_hex = encryption_service.derive_key_for_connection(&pwd)?;

            // Validate the key actually works
            encryption_service
                .validate_key(&key_hex)
                .map_err(|_| anyhow::anyhow!("Invalid password"))?;

            // Store in keychain
            KeychainService::store_key(&key_hex)?;

            if json {
                println!(
                    "{}",
                    serde_json::json!({"unlocked": true, "keychain": true})
                );
            } else {
                println!("{}", "Database unlocked".green());
                println!("  {}", "Encryption key stored in OS keychain".dimmed());
            }
        }
        Some(EncryptCommands::Lock) => {
            if !encryption_service.is_encrypted()? {
                anyhow::bail!("Database is not encrypted. Nothing to lock.");
            }

            // Best-effort: keychain may not be available (e.g., headless Linux)
            let _ = KeychainService::delete_key();

            if json {
                println!(
                    "{}",
                    serde_json::json!({"locked": true, "keychain_cleared": true})
                );
            } else {
                println!("{}", "Database locked".green());
                println!("  {}", "Encryption key cleared from OS keychain".dimmed());
            }
        }
        None => {
            let logger = get_logger();
            log_event(
                &logger,
                LogEvent::new("encrypt_started").with_command("encrypt"),
            );

            // Encrypt the database
            if encryption_service.is_encrypted()? {
                anyhow::bail!("Database is already encrypted. Use 'tl decrypt' first.");
            }

            let pwd = get_password_with_confirm(password)?;

            // Only show confirmation if running interactively (no password provided via flag/env)
            let skip_confirm = env::var("TREELINE_PASSWORD").is_ok();
            if !skip_confirm
                && !Confirm::new()
                    .with_prompt(
                        "Are you sure you want to encrypt the database? A backup will be created.",
                    )
                    .interact()?
            {
                println!("Cancelled.");
                return Ok(());
            }

            // Create BackupService directly - don't need full context for encryption
            let backup_service =
                BackupService::new(treeline_dir.clone(), "treeline.duckdb".to_string());
            match encryption_service.encrypt(&pwd, &backup_service) {
                Ok(result) => {
                    log_event(
                        &logger,
                        LogEvent::new("encrypt_completed").with_command("encrypt"),
                    );

                    // Best-effort: store derived key in keychain for seamless access
                    if let Ok(key_hex) = encryption_service.derive_key_for_connection(&pwd) {
                        let _ = KeychainService::store_key(&key_hex);
                    }

                    if json {
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    } else {
                        println!("{}", "Database encrypted successfully".green());
                        if let Some(backup_name) = result.backup_name {
                            println!("  Backup created: {}", backup_name);
                        }
                        println!("  {}", "Encryption key stored in OS keychain".dimmed());
                    }
                }
                Err(e) => {
                    log_event(
                        &logger,
                        LogEvent::new("encrypt_failed")
                            .with_command("encrypt")
                            .with_error(&e.to_string()),
                    );
                    return Err(e);
                }
            }
        }
    }

    Ok(())
}

pub fn run_decrypt(password: Option<String>, json: bool) -> Result<()> {
    let logger = get_logger();
    log_event(
        &logger,
        LogEvent::new("decrypt_started").with_command("decrypt"),
    );

    let treeline_dir = super::get_treeline_dir();
    let config = Config::load(&treeline_dir)?;

    // Determine database path (always use treeline.duckdb for encryption, not demo.duckdb)
    let db_path = treeline_dir.join("treeline.duckdb");
    let encryption_service = EncryptionService::new(treeline_dir.clone(), db_path);

    // Check demo mode
    if config.demo_mode {
        if json {
            println!(
                "{}",
                serde_json::json!({"error": "Demo database is not encrypted"})
            );
        } else {
            eprintln!("{}", "Demo database is not encrypted".red());
        }
        std::process::exit(1);
    }

    if !encryption_service.is_encrypted()? {
        anyhow::bail!("Database is not encrypted");
    }

    let pwd = get_password_or_prompt(password, "Enter decryption password")?;

    // Create BackupService directly - don't need full context for decryption
    let backup_service = BackupService::new(treeline_dir.clone(), "treeline.duckdb".to_string());
    match encryption_service.decrypt(&pwd, &backup_service) {
        Ok(result) => {
            log_event(
                &logger,
                LogEvent::new("decrypt_completed").with_command("decrypt"),
            );

            // Best-effort: clear key from keychain since DB is no longer encrypted
            let _ = KeychainService::delete_key();

            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", "Database decrypted successfully".green());
                if let Some(backup_name) = result.backup_name {
                    println!("  Backup created: {}", backup_name);
                }
            }
        }
        Err(e) => {
            log_event(
                &logger,
                LogEvent::new("decrypt_failed")
                    .with_command("decrypt")
                    .with_error(&e.to_string()),
            );
            return Err(e);
        }
    }

    Ok(())
}
