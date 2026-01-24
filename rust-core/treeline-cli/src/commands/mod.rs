//! CLI command implementations

pub mod backup;
pub mod compact;
pub mod demo;
pub mod doctor;
pub mod encrypt;
pub mod logs;
pub mod plugin;
pub mod query;
pub mod status;
pub mod sync;
pub mod tag;

use std::path::PathBuf;
use anyhow::{Context, Result};
use treeline_core::services::EncryptionService;
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
pub fn get_context() -> Result<TreelineContext> {
    let treeline_dir = get_treeline_dir();

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&treeline_dir)
        .with_context(|| format!("Failed to create treeline directory: {:?}", treeline_dir))?;

    // Determine encryption key
    // Priority: TL_DB_KEY (pre-derived) > TL_DB_PASSWORD (needs derivation)
    let encryption_key = if let Ok(key) = std::env::var("TL_DB_KEY") {
        // Already derived key (used by Tauri app)
        eprintln!("DEBUG: Using TL_DB_KEY");
        Some(key)
    } else if let Ok(password) = std::env::var("TL_DB_PASSWORD") {
        // Password that needs derivation
        eprintln!("DEBUG: Using TL_DB_PASSWORD, deriving key...");
        let config = treeline_core::config::Config::load(&treeline_dir).unwrap_or_default();
        let db_filename = if config.demo_mode { "demo.duckdb" } else { "treeline.duckdb" };
        let db_path = treeline_dir.join(db_filename);
        eprintln!("DEBUG: db_path={:?}", db_path);

        let encryption_service = EncryptionService::new(treeline_dir.clone(), db_path);
        let is_encrypted = encryption_service.is_encrypted().unwrap_or(false);
        eprintln!("DEBUG: is_encrypted={}", is_encrypted);

        if is_encrypted {
            // Derive key from password
            match encryption_service.derive_key_for_connection(&password) {
                Ok(key) => {
                    eprintln!("DEBUG: Key derived successfully (len={})", key.len());
                    Some(key)
                }
                Err(e) => {
                    eprintln!("DEBUG: Key derivation failed: {:?}", e);
                    return Err(e).context("Failed to derive encryption key from password");
                }
            }
        } else {
            // Database not encrypted, don't need a key
            eprintln!("DEBUG: Database not encrypted, no key needed");
            None
        }
    } else {
        eprintln!("DEBUG: No encryption env var set");
        None
    };

    TreelineContext::new(&treeline_dir, encryption_key.as_deref())
        .map_err(|e| {
            eprintln!("DEBUG: TreelineContext::new failed: {:?}", e);
            e
        })
        .context("Failed to initialize treeline context")
}

