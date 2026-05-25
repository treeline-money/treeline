//! CLI command implementations

pub mod backup;
pub mod compact;
pub mod demo;
pub mod doctor;
pub mod encrypt;
pub mod hub;
pub mod import;
pub mod logs;
pub mod mcp;
pub mod plugin;
pub mod query;
pub mod schema;
pub mod serve;
pub mod setup;
pub mod skills;
pub mod status;
pub mod sync;
pub mod tag;
pub mod update;

use anyhow::{Context, Result};
use std::path::PathBuf;
use treeline_core::services::{EncryptionService, KeychainService};
use treeline_core::{EntryPoint, LogEvent, LoggingService, TreelineContext};

/// Get the logging service for CLI operations
///
/// Returns None if logging fails to initialize (shouldn't block operations)
pub fn get_logger() -> Option<LoggingService> {
    let treeline_dir = get_treeline_dir();
    // Ensure directory exists
    std::fs::create_dir_all(&treeline_dir).ok()?;
    LoggingService::new(&treeline_dir, EntryPoint::Cli, env!("CARGO_PKG_VERSION")).ok()
}

/// Log an event, ignoring any errors (logging should never break the app)
pub fn log_event(logger: &Option<LoggingService>, event: LogEvent) {
    if let Some(l) = logger {
        let _ = l.log(event);
    }
}

/// Get the treeline directory from environment or default
pub fn get_treeline_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TREELINE_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::home_dir()
            .expect("Could not find home directory")
            .join(".treeline")
    }
}

/// Get or create treeline context
///
/// Key resolution priority:
/// 1. TL_DB_KEY env var (pre-derived hex key, for automation/CI)
/// 2. TL_DB_PASSWORD env var (password, needs Argon2 derivation)
/// 3. OS keychain lookup (with validation)
/// 4. Interactive prompt (CLI only — not available for MCP)
/// 5. Error with clear message offering choices
pub fn get_context() -> Result<TreelineContext> {
    let treeline_dir = get_treeline_dir();

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&treeline_dir)
        .with_context(|| format!("Failed to create treeline directory: {:?}", treeline_dir))?;

    // Determine encryption key
    let encryption_key = if let Ok(key) = std::env::var("TL_DB_KEY") {
        // Priority 1: Already derived key (used by Tauri app)
        Some(key)
    } else if let Ok(password) = std::env::var("TL_DB_PASSWORD") {
        // Priority 2: Password that needs derivation
        let config = treeline_core::config::Config::load(&treeline_dir).unwrap_or_default();
        let db_filename = if config.demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };
        let db_path = treeline_dir.join(db_filename);

        let encryption_service = EncryptionService::new(treeline_dir.clone(), db_path);
        let is_encrypted = encryption_service.is_encrypted().unwrap_or(false);

        if is_encrypted {
            match encryption_service.derive_key_for_connection(&password) {
                Ok(key) => {
                    // Best-effort: also store in keychain for future use
                    let _ = KeychainService::store_key(&key);
                    Some(key)
                }
                Err(e) => {
                    return Err(e).context("Failed to derive encryption key from password");
                }
            }
        } else {
            None
        }
    } else {
        // Priority 3+: Check if DB is encrypted, then try keychain / prompt
        let config = treeline_core::config::Config::load(&treeline_dir).unwrap_or_default();
        let db_filename = if config.demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };
        let db_path = treeline_dir.join(db_filename);

        let encryption_service = EncryptionService::new(treeline_dir.clone(), db_path);
        let is_encrypted = encryption_service.is_encrypted().unwrap_or(false);

        if is_encrypted {
            // Priority 3: Try keychain
            if let Ok(Some(key)) = KeychainService::get_key() {
                // Validate the key still works (user may have re-encrypted)
                if encryption_service.validate_key(&key).is_ok() {
                    Some(key)
                } else {
                    // Stale key — clear it and fall through to prompt/error
                    let _ = KeychainService::delete_key();
                    prompt_or_error_for_key(&encryption_service)?
                }
            } else {
                // Priority 4/5: Prompt or error
                prompt_or_error_for_key(&encryption_service)?
            }
        } else {
            None
        }
    };

    TreelineContext::new(&treeline_dir, encryption_key.as_deref())
        .context("Failed to initialize treeline context")
}

/// Get or create treeline context in non-interactive mode.
///
/// Only uses environment variables for encryption key resolution.
/// No keychain access, no interactive prompts. Suitable for servers.
///
/// Key resolution:
/// 1. TL_DB_KEY env var (pre-derived hex key)
/// 2. TL_DB_PASSWORD env var (password, needs Argon2 derivation)
/// 3. If encrypted and no key available, returns error
/// 4. If not encrypted, proceeds without a key
#[allow(dead_code)] // Exposed for future server/non-interactive callers.
pub fn get_context_non_interactive() -> Result<TreelineContext> {
    let treeline_dir = get_treeline_dir();

    std::fs::create_dir_all(&treeline_dir)
        .with_context(|| format!("Failed to create treeline directory: {:?}", treeline_dir))?;

    let encryption_key = if let Ok(key) = std::env::var("TL_DB_KEY") {
        Some(key)
    } else if let Ok(password) = std::env::var("TL_DB_PASSWORD") {
        let config = treeline_core::config::Config::load(&treeline_dir).unwrap_or_default();
        let db_filename = if config.demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };
        let db_path = treeline_dir.join(db_filename);

        let encryption_service = EncryptionService::new(treeline_dir.clone(), db_path);
        if encryption_service.is_encrypted().unwrap_or(false) {
            match encryption_service.derive_key_for_connection(&password) {
                Ok(key) => Some(key),
                Err(e) => return Err(e).context("Failed to derive encryption key from password"),
            }
        } else {
            None
        }
    } else {
        let config = treeline_core::config::Config::load(&treeline_dir).unwrap_or_default();
        let db_filename = if config.demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };
        let db_path = treeline_dir.join(db_filename);

        let encryption_service = EncryptionService::new(treeline_dir.clone(), db_path);
        if encryption_service.is_encrypted().unwrap_or(false) {
            anyhow::bail!(
                "Database is encrypted. Set TL_DB_KEY or TL_DB_PASSWORD to enable queries."
            );
        }
        None
    };

    TreelineContext::new(&treeline_dir, encryption_key.as_deref())
        .context("Failed to initialize treeline context")
}

/// Try to get the encryption key via interactive prompt, or error if not a TTY.
///
/// If running interactively (TTY), prompts for password, derives key, validates,
/// and stores in keychain for next time. If not a TTY (MCP/piped), returns a
/// clear error message.
fn prompt_or_error_for_key(encryption_service: &EncryptionService) -> Result<Option<String>> {
    if atty::is(atty::Stream::Stdin) {
        // Interactive: prompt for password
        let password = dialoguer::Password::new()
            .with_prompt("Enter encryption password")
            .interact()
            .context("Failed to read password")?;

        let key = encryption_service
            .derive_key_for_connection(&password)
            .context("Invalid password")?;

        encryption_service
            .validate_key(&key)
            .map_err(|_| anyhow::anyhow!("Invalid password"))?;

        // Store in keychain so user doesn't have to type it again
        let _ = KeychainService::store_key(&key);

        Ok(Some(key))
    } else {
        // Non-interactive (MCP, piped): clear error message
        anyhow::bail!(
            "Database is encrypted and locked. To unlock:\n\
             - Open the Treeline app and enter your password\n\
             - Or run 'tl encrypt unlock' in your terminal"
        );
    }
}
