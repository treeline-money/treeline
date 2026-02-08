use duckdb::Connection;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

// treeline-core integration for direct library calls (replaces CLI subprocess)
// NOTE: Only import services and config - NEVER import adapters or ports directly
use treeline_core::config::ColumnMappings;
use treeline_core::services::{
    BackfillExecuteResult, BackupService, BalanceSnapshotPreview, DemoService, EncryptionService,
    EntryPoint, ImportOptions, LogEvent, LoggingService, NumberFormat, PluginService,
};
use treeline_core::TreelineContext;

mod permissions;
use permissions::PluginContext;

/// Compare CalVer versions (YY.M.DDRR format)
///
/// CalVer format: YY.M.DDRR where:
/// - YY: Year (e.g., 26 for 2026)
/// - M: Month (1-12, no leading zero)
/// - DDRR: Day (01-31) + Release number (01-99)
///
/// Examples: 26.1.3001, 26.2.101, 26.12.3115
///
/// Returns true if remote version is newer than current version.
fn calver_comparator(current: &str, remote: &str) -> bool {
    let parse = |v: &str| -> Option<(u32, u32, u32)> {
        // Strip leading 'v' if present
        let v = v.trim_start_matches('v');
        let parts: Vec<&str> = v.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ))
    };
    match (parse(current), parse(remote)) {
        (Some(c), Some(r)) => r > c,
        _ => false,
    }
}

/// App state holding the encryption key for database access
pub struct EncryptionState {
    /// The derived encryption key (hex-encoded), if database is encrypted and unlocked
    key: Mutex<Option<String>>,
}

impl Default for EncryptionState {
    fn default() -> Self {
        Self {
            key: Mutex::new(None),
        }
    }
}

/// App state holding the shared TreelineContext
///
/// This ensures all Tauri commands share one database connection,
/// avoiding lock contention and improving performance.
pub struct TreelineContextState {
    /// The shared context (created on first use, held for app lifetime)
    context: Mutex<Option<TreelineContext>>,
    /// The encryption key used to create the current context (for invalidation)
    context_key: Mutex<Option<String>>,
}

impl Default for TreelineContextState {
    fn default() -> Self {
        Self {
            context: Mutex::new(None),
            context_key: Mutex::new(None),
        }
    }
}

impl TreelineContextState {
    /// Invalidate the cached context (e.g., after encryption change or restore)
    pub fn invalidate(&self) {
        let mut ctx = self.context.lock().unwrap();
        let mut key = self.context_key.lock().unwrap();
        *ctx = None;
        *key = None;
    }
}

/// App state tracking devtools visibility
/// (needed because is_devtools_open() is not supported on Windows)
pub struct DevtoolsState {
    open: AtomicBool,
}

impl Default for DevtoolsState {
    fn default() -> Self {
        Self {
            open: AtomicBool::new(false),
        }
    }
}

/// App state holding the available update for download/install
/// Uses tauri async Mutex since Update must be used in async context
pub struct AppUpdateState {
    update: tauri::async_runtime::Mutex<Option<tauri_plugin_updater::Update>>,
}

impl Default for AppUpdateState {
    fn default() -> Self {
        Self {
            update: tauri::async_runtime::Mutex::new(None),
        }
    }
}

/// App state holding the logging service for structured event logging
pub struct LoggingState {
    /// The logging service (initialized on app startup, None if init failed)
    logger: Mutex<Option<LoggingService>>,
}

impl Default for LoggingState {
    fn default() -> Self {
        Self {
            logger: Mutex::new(None),
        }
    }
}

/// App state holding the file watcher for plugin hot-reload
pub struct PluginWatcherState {
    /// The debounced file watcher handle (dropping it stops the watcher)
    watcher: Mutex<Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>>,
}

impl Default for PluginWatcherState {
    fn default() -> Self {
        Self {
            watcher: Mutex::new(None),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PluginManifest {
    id: String,
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default = "default_main")]
    main: String,
    #[serde(default)]
    permissions: Option<serde_json::Value>,
    #[serde(default)]
    source: Option<String>,
}

fn default_main() -> String {
    "index.js".to_string()
}

#[derive(Debug, Serialize)]
struct ExternalPlugin {
    manifest: PluginManifest,
    path: String,
}

// QueryResult is returned from treeline_core and serialized to JSON

/// Encryption metadata stored in encryption.json
#[derive(Debug, Serialize, Deserialize)]
struct EncryptionMetadata {
    encrypted: bool,
    salt: String, // Base64-encoded
    algorithm: String,
    version: i32,
    argon2_params: Argon2Params,
}

#[derive(Debug, Serialize, Deserialize)]
struct Argon2Params {
    time_cost: u32,
    memory_cost: u32,
    parallelism: u32,
    hash_len: u32,
}

/// Encryption status for frontend
#[derive(Debug, Serialize)]
struct EncryptionStatus {
    encrypted: bool,
    locked: bool, // true if encrypted but no key in memory
    algorithm: Option<String>,
    version: Option<i32>,
}

/// Read encryption metadata from encryption.json
/// Returns None in demo mode since demo.duckdb is never encrypted
fn read_encryption_metadata() -> Option<EncryptionMetadata> {
    // Demo mode uses demo.duckdb which is never encrypted
    // Treat as if encryption.json doesn't exist
    if get_demo_mode() {
        return None;
    }

    let treeline_dir = get_treeline_dir().ok()?;
    let encryption_path = treeline_dir.join("encryption.json");

    if !encryption_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&encryption_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Derive encryption key from password using Argon2id
fn derive_key(password: &str, salt: &[u8], params: &Argon2Params) -> Result<Vec<u8>, String> {
    let argon2_params = Params::new(
        params.memory_cost,
        params.time_cost,
        params.parallelism,
        Some(params.hash_len as usize),
    )
    .map_err(|e| format!("Invalid Argon2 params: {}", e))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);

    let mut key = vec![0u8; params.hash_len as usize];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Key derivation failed: {}", e))?;

    Ok(key)
}

/// Get the path to the DuckDB database file.
/// Centralized location for database path logic.
fn get_db_path() -> Result<PathBuf, String> {
    let treeline_dir = get_treeline_dir()?;

    // Check for demo mode (uses same logic as get_demo_mode)
    let demo_mode = get_demo_mode();

    let db_filename = if demo_mode {
        "demo.duckdb"
    } else {
        "treeline.duckdb"
    };
    let db_path = treeline_dir.join(db_filename);

    Ok(db_path)
}

/// Execute a SQL query using treeline-core
/// All database access now goes through TreelineContext for unified connection management
/// Uses spawn_blocking to avoid blocking the UI thread
#[tauri::command]
async fn execute_query(
    query: String,
    readonly: Option<bool>, // Kept for API compatibility, but no longer used
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<String, String> {
    let _ = readonly; // Suppress unused warning - treeline-core handles read/write internally

    let key = get_encryption_key(&encryption_state)?;

    // Clone the shared repository Arc - drop the mutex guard before spawning
    let repository = {
        let ctx_guard = get_or_create_context(&context_state, key)?;
        let ctx = ctx_guard.as_ref().unwrap();
        ctx.repository.clone()
    };
    // Mutex guard dropped here - UI thread is free

    tauri::async_runtime::spawn_blocking(move || {
        let query_service = treeline_core::services::QueryService::new(repository);
        let result = query_service
            .execute_sql(&query)
            .map_err(|e| format!("Failed to execute query: {}", e))?;
        serde_json::to_string(&result).map_err(|e| format!("Failed to serialize result: {}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Execute a parameterized SQL query using treeline-core - SAFE from SQL injection
/// Parameters are bound using ? placeholders
/// If plugin_context is provided, validates query permissions before execution
/// Uses spawn_blocking to avoid blocking the UI thread
#[tauri::command]
async fn execute_query_with_params(
    query: String,
    params: Vec<serde_json::Value>,
    readonly: Option<bool>, // Kept for API compatibility, but no longer used
    plugin_context: Option<PluginContext>,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<String, String> {
    let _ = readonly; // Suppress unused warning - treeline-core handles read/write internally

    // If plugin context provided, validate permissions before executing
    if let Some(ref pctx) = plugin_context {
        permissions::validate_query_permissions(&query, pctx)?;
    }

    let key = get_encryption_key(&encryption_state)?;

    // Clone the shared repository Arc - drop the mutex guard before spawning
    let repository = {
        let ctx_guard = get_or_create_context(&context_state, key)?;
        let ctx = ctx_guard.as_ref().unwrap();
        ctx.repository.clone()
    };
    // Mutex guard dropped here - UI thread is free

    tauri::async_runtime::spawn_blocking(move || {
        let query_service = treeline_core::services::QueryService::new(repository);
        let result = query_service
            .execute_sql_with_params(&query, &params)
            .map_err(|e| format!("Failed to execute query: {}", e))?;
        serde_json::to_string(&result).map_err(|e| format!("Failed to serialize result: {}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
fn get_plugins_dir() -> Result<String, String> {
    let treeline_dir = get_treeline_dir()?;
    let plugins_dir = treeline_dir.join("plugins");

    // Create directory if it doesn't exist
    if !plugins_dir.exists() {
        fs::create_dir_all(&plugins_dir)
            .map_err(|e| format!("Failed to create plugins directory: {}", e))?;
    }

    plugins_dir
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Invalid plugins directory path".to_string())
}

#[tauri::command]
fn get_treeline_dir_display() -> Result<String, String> {
    let treeline_dir = get_treeline_dir()?;
    treeline_dir
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Invalid treeline directory path".to_string())
}

/// Get the path to the treeline directory.
///
/// Uses `TREELINE_DIR` environment variable if set, otherwise defaults to `~/.treeline`.
/// This allows testing with isolated data directories.
fn get_treeline_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("TREELINE_DIR") {
        let path = PathBuf::from(&dir);
        // Create directory if it doesn't exist
        if !path.exists() {
            std::fs::create_dir_all(&path)
                .map_err(|e| format!("Failed to create TREELINE_DIR '{}': {}", dir, e))?;
        }
        return Ok(path);
    }
    let home_dir = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home_dir.join(".treeline"))
}

/// Check if staging updates are enabled.
///
/// If `~/.treeline/use-staging-updates` exists, the app will check for updates
/// from `latest-staging.json` instead of `latest.json`. This allows testing
/// release candidates before promoting them to production.
fn use_staging_updates() -> bool {
    get_treeline_dir()
        .map(|dir| dir.join("use-staging-updates").exists())
        .unwrap_or(false)
}

/// Response from check_for_app_update command
#[derive(Serialize)]
struct AppUpdateInfo {
    version: String,
    body: Option<String>,
    date: Option<String>,
}

/// Check for app updates with staging endpoint support.
///
/// This command uses UpdaterBuilder to dynamically configure the endpoint
/// based on whether staging updates are enabled (`~/.treeline/use-staging-updates`).
/// The Update object is stored in app state for later download/install.
#[tauri::command]
async fn check_for_app_update(
    app: AppHandle,
    update_state: State<'_, AppUpdateState>,
) -> Result<Option<AppUpdateInfo>, String> {
    let mut builder = app.updater_builder();

    // Override endpoint if staging updates are enabled
    if use_staging_updates() {
        builder = builder.endpoints(vec![
            "https://github.com/treeline-money/treeline/releases/latest/download/latest-staging.json"
                .parse()
                .map_err(|e| format!("Invalid staging URL: {}", e))?
        ]).map_err(|e| format!("Failed to set endpoints: {}", e))?;
    }

    let updater = builder
        .build()
        .map_err(|e| format!("Failed to build updater: {}", e))?;

    match updater.check().await {
        Ok(Some(update)) => {
            let info = AppUpdateInfo {
                version: update.version.clone(),
                body: update.body.clone(),
                date: update.date.map(|d| d.to_string()),
            };
            // Store update for later download/install
            *update_state.update.lock().await = Some(update);
            Ok(Some(info))
        }
        Ok(None) => {
            *update_state.update.lock().await = None;
            Ok(None)
        }
        Err(e) => Err(format!("Update check failed: {}", e)),
    }
}

/// Download and install the available update.
/// Must call check_for_app_update first to find an available update.
/// Creates a backup before updating to protect against update failures.
#[tauri::command]
async fn download_and_install_app_update(
    app: AppHandle,
    update_state: State<'_, AppUpdateState>,
) -> Result<(), String> {
    let mut guard = update_state.update.lock().await;
    let update = guard
        .take()
        .ok_or("No update available. Call check_for_app_update first.")?;

    // Create backup before applying update
    let treeline_dir = get_treeline_dir()?;
    let demo_mode = get_demo_mode();
    let db_filename = if demo_mode {
        "demo.duckdb"
    } else {
        "treeline.duckdb"
    };
    let backup_service = BackupService::new(treeline_dir, db_filename.to_string());
    // Create backup with rotation (keep last 10)
    if let Err(e) = backup_service.create(Some(10)) {
        eprintln!("Warning: Failed to create pre-update backup: {}", e);
        // Continue with update even if backup fails
    }

    // Download and install the update
    // The callbacks could be used to report progress, but for simplicity we just await
    update
        .download_and_install(|_bytes, _total| {}, || {})
        .await
        .map_err(|e| format!("Failed to download/install update: {}", e))?;

    // Emit an event so the frontend knows to restart
    app.emit("update-installed", ()).ok();

    Ok(())
}

/// Get encryption key from EncryptionState (None if not encrypted or not unlocked)
fn get_encryption_key(encryption_state: &EncryptionState) -> Result<Option<String>, String> {
    let key_guard = encryption_state
        .key
        .lock()
        .map_err(|_| "Failed to lock encryption state")?;
    Ok(key_guard.clone())
}

/// Get the shared TreelineContext, creating it if necessary.
///
/// This ensures all commands share one database connection, avoiding lock contention.
/// The context is cached and reused for the lifetime of the app.
/// If the encryption key changes, the context is invalidated and recreated.
fn get_or_create_context<'a>(
    context_state: &'a TreelineContextState,
    encryption_key: Option<String>,
) -> Result<std::sync::MutexGuard<'a, Option<TreelineContext>>, String> {
    // Check if we need to invalidate due to key change
    {
        let current_key = context_state
            .context_key
            .lock()
            .map_err(|_| "Failed to lock context key state")?;
        if *current_key != encryption_key {
            // Key changed, need to drop current context and create new one
            drop(current_key);
            context_state.invalidate();
        }
    }

    let mut ctx_guard = context_state
        .context
        .lock()
        .map_err(|_| "Failed to lock context state")?;

    if ctx_guard.is_none() {
        // Create new context
        let treeline_dir = get_treeline_dir()?;
        let ctx = TreelineContext::new(&treeline_dir, encryption_key.as_deref())
            .map_err(|e| e.to_string())?;
        *ctx_guard = Some(ctx);

        // Store the key used
        let mut key_guard = context_state
            .context_key
            .lock()
            .map_err(|_| "Failed to lock context key state")?;
        *key_guard = encryption_key;
    }

    Ok(ctx_guard)
}

/// Read the unified settings.json file
#[tauri::command]
fn read_settings() -> Result<String, String> {
    let treeline_dir = get_treeline_dir()?;
    let settings_path = treeline_dir.join("settings.json");

    if !settings_path.exists() {
        // Return default settings structure
        let default_settings = serde_json::json!({
            "app": {
                "theme": "dark",
                "lastSyncDate": null,
                "autoSyncOnStartup": true
            },
            "plugins": {}
        });
        return Ok(default_settings.to_string());
    }

    fs::read_to_string(&settings_path).map_err(|e| format!("Failed to read settings: {}", e))
}

/// Write the unified settings.json file
#[tauri::command]
fn write_settings(content: String) -> Result<(), String> {
    let treeline_dir = get_treeline_dir()?;

    // Ensure treeline directory exists
    if !treeline_dir.exists() {
        fs::create_dir_all(&treeline_dir)
            .map_err(|e| format!("Failed to create treeline directory: {}", e))?;
    }

    let settings_path = treeline_dir.join("settings.json");

    // Validate JSON before writing
    serde_json::from_str::<JsonValue>(&content).map_err(|e| format!("Invalid JSON: {}", e))?;

    fs::write(&settings_path, content).map_err(|e| format!("Failed to write settings: {}", e))
}

// ============================================================================
// Backup & Compact Commands
// ============================================================================

/// List all backups
#[tauri::command]
fn list_backups() -> Result<String, String> {
    let treeline_dir = get_treeline_dir()?;
    let demo_mode = get_demo_mode();
    let db_filename = if demo_mode {
        "demo.duckdb"
    } else {
        "treeline.duckdb"
    };

    let backup_service = BackupService::new(treeline_dir, db_filename.to_string());
    let backups = backup_service.list().map_err(|e| e.to_string())?;

    serde_json::to_string(&backups).map_err(|e| e.to_string())
}

/// Create a new backup
#[tauri::command]
async fn create_backup(max_backups: Option<usize>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let demo_mode = get_demo_mode();
        let db_filename = if demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };

        let backup_service = BackupService::new(treeline_dir, db_filename.to_string());
        let result = backup_service
            .create(max_backups)
            .map_err(|e| e.to_string())?;

        serde_json::to_string(&result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Restore from a backup
#[tauri::command]
async fn restore_backup(
    backup_name: String,
    context_state: State<'_, TreelineContextState>,
) -> Result<(), String> {
    // Invalidate the shared context first to release the database connection
    // This allows the BackupService to get exclusive access for restore
    context_state.invalidate();

    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let demo_mode = get_demo_mode();
        let db_filename = if demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };

        let backup_service = BackupService::new(treeline_dir, db_filename.to_string());
        backup_service
            .restore(&backup_name)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Delete a backup
#[tauri::command]
async fn delete_backup(backup_name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let backups_dir = treeline_dir.join("backups");
        let backup_path = backups_dir.join(&backup_name);

        if !backup_path.exists() {
            return Err(format!("Backup not found: {}", backup_name));
        }

        fs::remove_file(&backup_path).map_err(|e| format!("Failed to delete backup: {}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Clear all backups
#[tauri::command]
async fn clear_backups() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let demo_mode = get_demo_mode();
        let db_filename = if demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };

        let backup_service = BackupService::new(treeline_dir, db_filename.to_string());
        let result = backup_service.clear().map_err(|e| e.to_string())?;

        serde_json::to_string(&result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Compact the database (CHECKPOINT + VACUUM)
#[tauri::command]
fn compact_database(
    encryption_state: State<EncryptionState>,
    context_state: State<TreelineContextState>,
) -> Result<String, String> {
    let key = get_encryption_key(&encryption_state)?;
    let ctx_guard = get_or_create_context(&context_state, key)?;
    let ctx = ctx_guard.as_ref().unwrap();

    let result = ctx.compact_service.compact().map_err(|e| e.to_string())?;
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

// ============================================================================
// Theme System
// ============================================================================

/// Theme definition loaded from JSON files
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ThemeDefinition {
    id: String,
    name: String,
    extends: Option<String>,
    variables: std::collections::HashMap<String, String>,
}

/// Default themes embedded at compile time
const DEFAULT_THEMES: &[(&str, &str)] = &[
    ("dark.json", include_str!("../themes/dark.json")),
    ("light.json", include_str!("../themes/light.json")),
];

/// Ensure default themes exist in ~/.treeline/themes/
fn ensure_default_themes(themes_dir: &std::path::Path) -> Result<(), String> {
    fs::create_dir_all(themes_dir)
        .map_err(|e| format!("Failed to create themes directory: {}", e))?;

    // Only write defaults if folder is empty
    let is_empty = fs::read_dir(themes_dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true);

    if is_empty {
        for (name, content) in DEFAULT_THEMES {
            fs::write(themes_dir.join(name), content)
                .map_err(|e| format!("Failed to write default theme {}: {}", name, e))?;
        }
    }

    Ok(())
}

/// List all available themes from ~/.treeline/themes/
#[tauri::command]
fn list_themes() -> Result<Vec<ThemeDefinition>, String> {
    let treeline_dir = get_treeline_dir()?;
    let themes_dir = treeline_dir.join("themes");

    // Ensure default themes exist
    ensure_default_themes(&themes_dir)?;

    let mut themes = Vec::new();

    for entry in
        fs::read_dir(&themes_dir).map_err(|e| format!("Failed to read themes directory: {}", e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<ThemeDefinition>(&content) {
                    Ok(theme) => themes.push(theme),
                    Err(e) => eprintln!("Invalid theme {}: {}", path.display(), e),
                },
                Err(e) => eprintln!("Failed to read {}: {}", path.display(), e),
            }
        }
    }

    // Sort themes by name for consistent ordering
    themes.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(themes)
}

/// Set DevTools visibility (for plugin development)
/// If `open` is None, toggles the current state
/// Note: We track state ourselves because is_devtools_open() and close_devtools()
/// are not supported on Windows
#[tauri::command]
fn set_devtools(
    app: tauri::AppHandle,
    devtools_state: State<DevtoolsState>,
    open: Option<bool>,
) -> Result<bool, String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    let currently_open = devtools_state.open.load(Ordering::SeqCst);
    let should_open = open.unwrap_or(!currently_open);

    if should_open && !currently_open {
        window.open_devtools();
        devtools_state.open.store(true, Ordering::SeqCst);
    } else if !should_open && currently_open {
        // Note: close_devtools() is not supported on Windows, but we call it anyway
        // On Windows this will be a no-op
        window.close_devtools();
        devtools_state.open.store(false, Ordering::SeqCst);
    }

    Ok(should_open)
}

/// Read plugin-specific state file (for runtime state, not user settings)
#[tauri::command]
fn read_plugin_state(plugin_id: String) -> Result<String, String> {
    let treeline_dir = get_treeline_dir()?;
    let state_path = treeline_dir
        .join("plugins")
        .join(&plugin_id)
        .join("state.json");

    if !state_path.exists() {
        return Ok("null".to_string());
    }

    fs::read_to_string(&state_path).map_err(|e| format!("Failed to read plugin state: {}", e))
}

/// Write plugin-specific state file (for runtime state, not user settings)
#[tauri::command]
fn write_plugin_state(plugin_id: String, content: String) -> Result<(), String> {
    let treeline_dir = get_treeline_dir()?;
    let plugin_dir = treeline_dir.join("plugins").join(&plugin_id);

    // Create plugin directory if it doesn't exist
    if !plugin_dir.exists() {
        fs::create_dir_all(&plugin_dir)
            .map_err(|e| format!("Failed to create plugin directory: {}", e))?;
    }

    let state_path = plugin_dir.join("state.json");

    fs::write(&state_path, content).map_err(|e| format!("Failed to write plugin state: {}", e))
}

/// Get current demo mode status from settings.json
#[tauri::command]
fn get_demo_mode() -> bool {
    // First check env var (for CI/testing)
    if let Ok(env_val) = std::env::var("TREELINE_DEMO_MODE") {
        let lower = env_val.to_lowercase();
        if lower == "true" || lower == "1" || lower == "yes" {
            return true;
        }
        if lower == "false" || lower == "0" || lower == "no" {
            return false;
        }
    }

    // Fall back to settings file (shared with CLI)
    let settings_path = match get_treeline_dir() {
        Ok(dir) => dir.join("settings.json"),
        Err(_) => return false,
    };

    if !settings_path.exists() {
        return false;
    }

    match fs::read_to_string(&settings_path) {
        Ok(content) => {
            if let Ok(settings) = serde_json::from_str::<JsonValue>(&content) {
                settings
                    .get("app")
                    .and_then(|app| app.get("demoMode"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Set demo mode in settings.json (shared with CLI)
#[tauri::command]
fn set_demo_mode(enabled: bool) -> Result<(), String> {
    let treeline_dir = get_treeline_dir()?;

    // Ensure directory exists
    if !treeline_dir.exists() {
        fs::create_dir_all(&treeline_dir)
            .map_err(|e| format!("Failed to create treeline directory: {}", e))?;
    }

    let settings_path = treeline_dir.join("settings.json");

    // Read existing settings or create new with default structure
    let mut settings: serde_json::Map<String, JsonValue> = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Failed to read settings: {}", e))?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        serde_json::Map::new()
    };

    // Ensure "app" key exists
    if !settings.contains_key("app") {
        settings.insert("app".to_string(), JsonValue::Object(serde_json::Map::new()));
    }

    // Update demoMode in app settings
    if let Some(JsonValue::Object(app)) = settings.get_mut("app") {
        app.insert("demoMode".to_string(), JsonValue::Bool(enabled));
    }

    // Write back
    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&settings_path, content).map_err(|e| format!("Failed to write settings: {}", e))?;

    Ok(())
}

/// Run sync using treeline-core SyncService directly
/// Uses spawn_blocking to avoid blocking the UI thread
/// Creates a backup before syncing to protect against sync issues
#[tauri::command]
async fn run_sync(
    dry_run: Option<bool>,
    balances_only: Option<bool>,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
    logging_state: State<'_, LoggingState>,
) -> Result<String, String> {
    let key = get_encryption_key(&encryption_state)?;
    let dry_run = dry_run.unwrap_or(false);
    let balances_only = balances_only.unwrap_or(false);

    // Log sync started
    {
        if let Ok(guard) = logging_state.logger.lock() {
            if let Some(logger) = guard.as_ref() {
                let _ = logger.log(LogEvent::new("sync_started"));
            }
        }
    }

    // Clone the shared repository Arc - this allows sync to use the same
    // database connection without holding the file lock during network I/O.
    // The repository's internal Mutex is only held during actual DB operations.
    let (repository, treeline_dir) = {
        let ctx_guard = get_or_create_context(&context_state, key)?;
        let ctx = ctx_guard.as_ref().unwrap();
        (ctx.repository.clone(), get_treeline_dir()?)
    };
    // Mutex guard dropped here - other operations can proceed

    // Run blocking treeline-core operation in a background thread
    let result = tauri::async_runtime::spawn_blocking(move || {
        // Create backup before sync (skip for dry runs)
        if !dry_run {
            let demo_mode = get_demo_mode();
            let db_filename = if demo_mode {
                "demo.duckdb"
            } else {
                "treeline.duckdb"
            };
            let backup_service = BackupService::new(treeline_dir.clone(), db_filename.to_string());
            // Create backup with rotation (keep last 10)
            if let Err(e) = backup_service.create(Some(10)) {
                eprintln!("Warning: Failed to create pre-sync backup: {}", e);
                // Continue with sync even if backup fails
            }
        }

        // Create SyncService with the SHARED repository (not a new context)
        let sync_service =
            treeline_core::services::SyncService::new(repository, treeline_dir.into());
        let sync_result = sync_service
            .sync(None, dry_run, balances_only)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&sync_result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    // Log sync results per integration
    {
        if let Ok(guard) = logging_state.logger.lock() {
            if let Some(logger) = guard.as_ref() {
                // Parse the result to log per-integration results
                if let Ok(sync_result) = serde_json::from_str::<serde_json::Value>(&result) {
                    if let Some(results) = sync_result.get("results").and_then(|r| r.as_array()) {
                        for r in results {
                            let integration = r
                                .get("integration")
                                .and_then(|i| i.as_str())
                                .unwrap_or("unknown");
                            if let Some(error) = r.get("error").and_then(|e| e.as_str()) {
                                let _ = logger.log(
                                    LogEvent::new("sync_failed")
                                        .with_integration(integration)
                                        .with_error(error),
                                );
                            } else {
                                let _ = logger.log(
                                    LogEvent::new("sync_completed").with_integration(integration),
                                );
                            }
                            // Log any auto-tag rule failures
                            if let Some(failures) =
                                r.get("auto_tag_failures").and_then(|f| f.as_array())
                            {
                                for failure in failures {
                                    let rule_name = failure
                                        .get("rule_name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("unknown");
                                    let error_msg = failure
                                        .get("error")
                                        .and_then(|e| e.as_str())
                                        .unwrap_or("unknown error");
                                    let _ = logger.log(
                                        LogEvent::new("auto_tag_rule_failed")
                                            .with_error(&format!("{}: {}", rule_name, error_msg)),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(result)
}

/// Enable demo mode (sets up demo integration and syncs demo data)
/// Uses treeline-core DemoService directly instead of CLI subprocess
#[tauri::command]
async fn enable_demo(context_state: State<'_, TreelineContextState>) -> Result<(), String> {
    // Invalidate the shared context - we're switching to demo.duckdb
    context_state.invalidate();

    // Run blocking treeline-core operation in a background thread
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let demo_service = DemoService::new(&treeline_dir);
        demo_service.enable().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Disable demo mode
/// Uses treeline-core DemoService directly instead of CLI subprocess
#[tauri::command]
async fn disable_demo(context_state: State<'_, TreelineContextState>) -> Result<(), String> {
    // Invalidate the shared context - we're switching back to treeline.duckdb
    context_state.invalidate();

    // Run blocking treeline-core operation in a background thread
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let demo_service = DemoService::new(&treeline_dir);
        demo_service.disable(false).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Install a plugin from GitHub URL using treeline-core
#[tauri::command]
async fn install_plugin(url: String, version: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let plugin_service = PluginService::new(&treeline_dir);
        let result = plugin_service
            .install_plugin(&url, version.as_deref(), false)
            .map_err(|e| e.to_string())?;

        if !result.success {
            return Err(result.error.unwrap_or_else(|| "Unknown error".to_string()));
        }

        serde_json::to_string(&result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Uninstall a plugin using treeline-core
#[tauri::command]
async fn uninstall_plugin(plugin_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let plugin_service = PluginService::new(&treeline_dir);
        let result = plugin_service
            .uninstall_plugin(&plugin_id)
            .map_err(|e| e.to_string())?;

        if !result.success {
            return Err(result.error.unwrap_or_else(|| "Unknown error".to_string()));
        }

        serde_json::to_string(&result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Upgrade a plugin to latest version using treeline-core
///
/// The frontend creates a database backup via createBackup() before
/// calling this command, protecting against breaking schema migrations.
#[tauri::command]
async fn upgrade_plugin(plugin_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let plugin_service = PluginService::new(&treeline_dir);
        let result = plugin_service
            .upgrade_plugin(&plugin_id)
            .map_err(|e| e.to_string())?;

        if !result.success {
            return Err(result.error.unwrap_or_else(|| "Unknown error".to_string()));
        }

        serde_json::to_string(&result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Check if a plugin has an update available using treeline-core
#[tauri::command]
async fn check_plugin_update(plugin_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let plugin_service = PluginService::new(&treeline_dir);
        let result = plugin_service
            .check_update(&plugin_id)
            .map_err(|e| e.to_string())?;

        serde_json::to_string(&result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Fetch plugin manifest from GitHub release (for install preview) using treeline-core
#[tauri::command]
async fn fetch_plugin_manifest(url: String, version: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let plugin_service = PluginService::new(&treeline_dir);
        let (manifest, release_version) = plugin_service
            .fetch_manifest(&url, version.as_deref())
            .map_err(|e| e.to_string())?;

        // Return combined manifest + version info as JSON
        let result = serde_json::json!({
            "manifest": manifest,
            "version": release_version
        });

        serde_json::to_string(&result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Preview CSV import using treeline-core ImportService
/// Returns JSON with detected columns and preview transactions
/// Format matches frontend ImportPreviewResult interface
/// Uses spawn_blocking to avoid blocking the UI thread
#[tauri::command]
async fn import_csv_preview(
    file_path: String,
    account_id: String,
    date_column: Option<String>,
    amount_column: Option<String>,
    description_column: Option<String>,
    debit_column: Option<String>,
    credit_column: Option<String>,
    balance_column: Option<String>,
    flip_signs: bool,
    debit_negative: bool,
    skip_rows: Option<u32>,
    number_format: Option<String>,
    anchor_balance: Option<f64>,
    anchor_date: Option<String>,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<String, String> {
    let key = get_encryption_key(&encryption_state)?;

    // Clone the shared repository Arc - drop the mutex guard before spawning
    let repository = {
        let ctx_guard = get_or_create_context(&context_state, key)?;
        let ctx = ctx_guard.as_ref().unwrap();
        ctx.repository.clone()
    };
    // Mutex guard dropped here - UI thread is free
    let treeline_dir = get_treeline_dir()?;

    let result = tauri::async_runtime::spawn_blocking(move || {
        let import_service =
            treeline_core::services::ImportService::new(repository, treeline_dir);

        let mappings = ColumnMappings {
            date: date_column.unwrap_or_else(|| "Date".to_string()),
            amount: amount_column.unwrap_or_else(|| "Amount".to_string()),
            description: description_column,
            debit: debit_column,
            credit: credit_column,
            balance: balance_column,
        };

        let skip_rows_val = skip_rows.unwrap_or(0);
        let number_format_val = number_format.unwrap_or_else(|| "us".to_string());

        // Parse anchor balance and date for preview balance calculation
        let parsed_anchor_balance =
            anchor_balance.map(|b| rust_decimal::Decimal::from_f64_retain(b).unwrap_or_default());
        let parsed_anchor_date = match anchor_date {
            Some(d) => Some(
                chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                    .map_err(|e| format!("Invalid anchor date '{}': {}", d, e))?,
            ),
            None => None,
        };

        let options = ImportOptions {
            flip_signs,
            debit_negative,
            skip_rows: skip_rows_val,
            number_format: NumberFormat::from_str(&number_format_val),
            anchor_balance: parsed_anchor_balance,
            anchor_date: parsed_anchor_date,
        };

        let result = import_service
            .import(
                std::path::Path::new(&file_path),
                &account_id,
                &mappings,
                &options,
                true, // preview_only
            )
            .map_err(|e| e.to_string())?;

        // Transform treeline-core ImportResult to frontend ImportPreviewResult format
        let preview_transactions: Vec<serde_json::Value> = result
            .transactions
            .unwrap_or_default()
            .into_iter()
            .map(|tx| {
                let amount: f64 = tx.amount.parse().unwrap_or(0.0);
                let balance: Option<f64> = tx.balance.as_ref().and_then(|b| b.parse().ok());
                serde_json::json!({
                    "date": tx.date,
                    "description": tx.description,
                    "amount": amount,
                    "balance": balance
                })
            })
            .collect();

        let preview_result = serde_json::json!({
            "file": file_path,
            "flip_signs": flip_signs,
            "debit_negative": debit_negative,
            "skip_rows": skip_rows_val,
            "number_format": number_format_val,
            "preview": preview_transactions
        });

        serde_json::to_string(&preview_result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    Ok(result)
}

/// Execute CSV import using treeline-core ImportService
/// Uses spawn_blocking to avoid blocking the UI thread
#[tauri::command]
async fn import_csv_execute(
    file_path: String,
    account_id: String,
    date_column: Option<String>,
    amount_column: Option<String>,
    description_column: Option<String>,
    debit_column: Option<String>,
    credit_column: Option<String>,
    balance_column: Option<String>,
    flip_signs: bool,
    debit_negative: bool,
    skip_rows: Option<u32>,
    number_format: Option<String>,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<String, String> {
    let key = get_encryption_key(&encryption_state)?;

    // Clone the shared repository Arc - drop the mutex guard before spawning
    let repository = {
        let ctx_guard = get_or_create_context(&context_state, key)?;
        let ctx = ctx_guard.as_ref().unwrap();
        ctx.repository.clone()
    };
    // Mutex guard dropped here - UI thread is free
    let treeline_dir = get_treeline_dir()?;

    let result = tauri::async_runtime::spawn_blocking(move || {
        let import_service =
            treeline_core::services::ImportService::new(repository, treeline_dir);

        let mappings = ColumnMappings {
            date: date_column.unwrap_or_else(|| "Date".to_string()),
            amount: amount_column.unwrap_or_else(|| "Amount".to_string()),
            description: description_column,
            debit: debit_column,
            credit: credit_column,
            balance: balance_column,
        };

        let options = ImportOptions {
            flip_signs,
            debit_negative,
            skip_rows: skip_rows.unwrap_or(0),
            number_format: NumberFormat::from_str(
                &number_format.unwrap_or_else(|| "us".to_string()),
            ),
            anchor_balance: None, // Not used for execute
            anchor_date: None,    // Not used for execute
        };

        let result = import_service
            .import(
                std::path::Path::new(&file_path),
                &account_id,
                &mappings,
                &options,
                false, // preview_only = false, actually execute
            )
            .map_err(|e| e.to_string())?;

        serde_json::to_string(&result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    Ok(result)
}

/// Open file picker dialog for CSV files
#[tauri::command]
async fn pick_csv_file(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let file = app
        .dialog()
        .file()
        .add_filter("CSV Files", &["csv"])
        .blocking_pick_file();

    Ok(file.map(|f| f.to_string()))
}

// ============================================================================
// CSV Utilities (extracted for testability)
// ============================================================================

/// Detect the most likely CSV delimiter from a line of text.
/// Supports comma (US standard), semicolon (EU standard), and tab delimiters.
/// Returns the delimiter as a byte.
fn detect_csv_delimiter(line: &str) -> u8 {
    let semicolons = line.matches(';').count();
    let commas = line.matches(',').count();
    let tabs = line.matches('\t').count();

    if semicolons > commas && semicolons > tabs {
        b';'
    } else if tabs > commas && tabs > semicolons {
        b'\t'
    } else {
        b','
    }
}

/// Parse a header line into individual column names.
/// Handles trimming whitespace and removing leading '#' characters.
fn parse_csv_headers(line: &str, delimiter: u8) -> Result<Vec<String>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(delimiter)
        .from_reader(line.as_bytes());

    let headers: Vec<String> = rdr
        .records()
        .next()
        .ok_or("Empty header line")?
        .map_err(|e| format!("Failed to parse headers: {}", e))?
        .iter()
        .map(|h| h.trim().trim_start_matches('#').to_string())
        .collect();

    Ok(headers)
}

/// Get CSV headers for column mapping
/// Supports skip_rows to skip leading non-header rows (e.g., bank letterhead)
#[tauri::command]
async fn get_csv_headers(file_path: String, skip_rows: Option<u32>) -> Result<Vec<String>, String> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let file = File::open(&file_path).map_err(|e| format!("Failed to open file: {}", e))?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    // Skip leading rows if specified
    let skip = skip_rows.unwrap_or(0);
    for _ in 0..skip {
        lines.next();
    }

    let header_line = lines
        .next()
        .ok_or("CSV file is empty or skip_rows too high")?
        .map_err(|e| format!("Failed to read header line: {}", e))?;

    let delimiter = detect_csv_delimiter(&header_line);
    parse_csv_headers(&header_line, delimiter)
}

// ============================================================================
// Watch Folder Commands
// ============================================================================

/// Pending import file info
#[derive(Debug, Serialize)]
struct PendingImportFile {
    path: String,
    filename: String,
    size_bytes: u64,
}

/// List CSV files waiting in the imports folder
#[tauri::command]
fn list_pending_imports() -> Result<Vec<PendingImportFile>, String> {
    let treeline_dir = get_treeline_dir()?;
    let imports_dir = treeline_dir.join("imports");

    // Create imports directory if it doesn't exist
    if !imports_dir.exists() {
        fs::create_dir_all(&imports_dir)
            .map_err(|e| format!("Failed to create imports directory: {}", e))?;
        return Ok(Vec::new());
    }

    let mut files = Vec::new();

    for entry in fs::read_dir(&imports_dir)
        .map_err(|e| format!("Failed to read imports directory: {}", e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();

        // Only include CSV files (not directories, not the "imported" subfolder)
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext.to_str().map(|s| s.to_lowercase()) == Some("csv".to_string()) {
                    let metadata = fs::metadata(&path)
                        .map_err(|e| format!("Failed to read file metadata: {}", e))?;

                    files.push(PendingImportFile {
                        path: path.to_string_lossy().to_string(),
                        filename: path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        size_bytes: metadata.len(),
                    });
                }
            }
        }
    }

    // Sort by filename for consistent ordering
    files.sort_by(|a, b| a.filename.cmp(&b.filename));

    Ok(files)
}

/// Move an imported file to the "imported" subfolder
#[tauri::command]
fn move_imported_file(file_path: String) -> Result<(), String> {
    let source = PathBuf::from(&file_path);

    if !source.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    let treeline_dir = get_treeline_dir()?;
    let imported_dir = treeline_dir.join("imports").join("imported");

    // Create imported directory if it doesn't exist
    if !imported_dir.exists() {
        fs::create_dir_all(&imported_dir)
            .map_err(|e| format!("Failed to create imported directory: {}", e))?;
    }

    // Get the filename
    let filename = source.file_name().ok_or("Invalid file path")?;

    let destination = imported_dir.join(filename);

    // If destination already exists, add timestamp to make it unique
    let final_destination = if destination.exists() {
        let stem = destination
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = destination
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("csv");
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        imported_dir.join(format!("{}_{}.{}", stem, timestamp, ext))
    } else {
        destination
    };

    fs::rename(&source, &final_destination).map_err(|e| format!("Failed to move file: {}", e))?;

    Ok(())
}

/// Preview balance backfill - shows what snapshots would be created/updated
/// Returns a list of calculated end-of-day balances without persisting them
/// Uses spawn_blocking to avoid blocking the UI thread
#[tauri::command]
async fn backfill_preview(
    account_id: String,
    known_balance: f64,
    known_date: String,
    start_date: Option<String>,
    end_date: Option<String>,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<Vec<BalanceSnapshotPreview>, String> {
    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    let key = get_encryption_key(&encryption_state)?;

    // Clone the shared repository Arc - drop the mutex guard before spawning
    let repository = {
        let ctx_guard = get_or_create_context(&context_state, key)?;
        let ctx = ctx_guard.as_ref().unwrap();
        ctx.repository.clone()
    };
    // Mutex guard dropped here - UI thread is free

    // Parse parameters before spawning (cheap, no I/O)
    let date = NaiveDate::parse_from_str(&known_date, "%Y-%m-%d")
        .map_err(|e| format!("Invalid date format: {}", e))?;
    let start = start_date
        .map(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| format!("Invalid start_date format: {}", e))?;
    let end = end_date
        .map(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| format!("Invalid end_date format: {}", e))?;
    let balance =
        Decimal::try_from(known_balance).map_err(|e| format!("Invalid balance: {}", e))?;

    tauri::async_runtime::spawn_blocking(move || {
        let balance_service = treeline_core::services::BalanceService::new(repository);
        balance_service
            .backfill_preview(&account_id, balance, date, start, end)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Execute balance backfill - creates/updates balance snapshots
/// Replaces all existing snapshots in range with calculated values
/// Uses spawn_blocking to avoid blocking the UI thread
#[tauri::command]
async fn backfill_execute(
    account_id: String,
    known_balance: f64,
    known_date: String,
    start_date: Option<String>,
    end_date: Option<String>,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<BackfillExecuteResult, String> {
    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    let key = get_encryption_key(&encryption_state)?;

    // Clone the shared repository Arc - drop the mutex guard before spawning
    let repository = {
        let ctx_guard = get_or_create_context(&context_state, key)?;
        let ctx = ctx_guard.as_ref().unwrap();
        ctx.repository.clone()
    };
    // Mutex guard dropped here - UI thread is free

    // Parse parameters before spawning (cheap, no I/O)
    let date = NaiveDate::parse_from_str(&known_date, "%Y-%m-%d")
        .map_err(|e| format!("Invalid date format: {}", e))?;
    let start = start_date
        .map(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| format!("Invalid start_date format: {}", e))?;
    let end = end_date
        .map(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| format!("Invalid end_date format: {}", e))?;
    let balance =
        Decimal::try_from(known_balance).map_err(|e| format!("Invalid balance: {}", e))?;

    tauri::async_runtime::spawn_blocking(move || {
        let balance_service = treeline_core::services::BalanceService::new(repository);
        balance_service
            .backfill_execute(&account_id, balance, date, start, end)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Setup SimpleFIN integration using treeline-core SyncService
#[tauri::command]
async fn setup_simplefin(
    token: String,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<String, String> {
    let key = get_encryption_key(&encryption_state)?;

    // Clone the shared repository Arc for setup
    let (repository, treeline_dir) = {
        let ctx_guard = get_or_create_context(&context_state, key)?;
        let ctx = ctx_guard.as_ref().unwrap();
        (ctx.repository.clone(), get_treeline_dir()?)
    };

    tauri::async_runtime::spawn_blocking(move || {
        let sync_service =
            treeline_core::services::SyncService::new(repository, treeline_dir.into());
        sync_service
            .setup_simplefin(&token)
            .map_err(|e| e.to_string())?;

        Ok("SimpleFIN integration configured successfully".to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Setup Lunchflow integration using treeline-core SyncService
///
/// Lunchflow is a multi-provider bank aggregator supporting global banks
/// (20,000+ banks across 40+ countries).
#[tauri::command]
async fn setup_lunchflow(
    api_key: String,
    base_url: Option<String>,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<String, String> {
    let key = get_encryption_key(&encryption_state)?;

    // Clone the shared repository Arc for setup
    let (repository, treeline_dir) = {
        let ctx_guard = get_or_create_context(&context_state, key)?;
        let ctx = ctx_guard.as_ref().unwrap();
        (ctx.repository.clone(), get_treeline_dir()?)
    };

    tauri::async_runtime::spawn_blocking(move || {
        let sync_service =
            treeline_core::services::SyncService::new(repository, treeline_dir.into());
        sync_service
            .setup_lunchflow(&api_key, base_url.as_deref())
            .map_err(|e| e.to_string())?;

        Ok("Lunchflow integration configured successfully".to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ============================================================================
// Encryption Commands
// ============================================================================

/// Get encryption status - checks if database is encrypted and if we have a key
#[tauri::command]
fn get_encryption_status(
    encryption_state: State<EncryptionState>,
) -> Result<EncryptionStatus, String> {
    let metadata = read_encryption_metadata();

    match metadata {
        Some(m) if m.encrypted => {
            // Check if we have a key (either in state or keychain)
            let has_key = {
                let key_guard = encryption_state
                    .key
                    .lock()
                    .map_err(|_| "Failed to lock encryption state")?;
                key_guard.is_some()
            };

            Ok(EncryptionStatus {
                encrypted: true,
                locked: !has_key,
                algorithm: Some(m.algorithm),
                version: Some(m.version),
            })
        }
        _ => Ok(EncryptionStatus {
            encrypted: false,
            locked: false,
            algorithm: None,
            version: None,
        }),
    }
}

/// Try to auto-unlock using keychain key (called on app startup)
#[tauri::command]
fn try_auto_unlock(encryption_state: State<EncryptionState>) -> Result<bool, String> {
    // Check if database is encrypted (returns None in demo mode)
    let _metadata = match read_encryption_metadata() {
        Some(m) if m.encrypted => m,
        _ => return Ok(true), // Not encrypted, nothing to unlock
    };

    // Check if already unlocked (key in memory from this session)
    let key_guard = encryption_state
        .key
        .lock()
        .map_err(|_| "Failed to lock encryption state")?;

    if key_guard.is_some() {
        return Ok(true); // Already unlocked
    }

    // Database is encrypted and no key in memory - need password
    Ok(false)
}

/// Unlock database with password
#[tauri::command]
fn unlock_database(
    password: String,
    encryption_state: State<EncryptionState>,
) -> Result<(), String> {
    let metadata = read_encryption_metadata().ok_or("Database is not encrypted")?;

    if !metadata.encrypted {
        return Err("Database is not encrypted".to_string());
    }

    // Decode salt
    let salt = BASE64
        .decode(&metadata.salt)
        .map_err(|e| format!("Failed to decode salt: {}", e))?;

    // Derive key
    let key_bytes = derive_key(&password, &salt, &metadata.argon2_params)?;
    let key_hex = hex::encode(&key_bytes);

    // Validate key by trying to open database
    // IMPORTANT: Disable extension autoloading to avoid macOS code signing issues
    let db_path = get_db_path()?;
    let config = duckdb::Config::default()
        .enable_autoload_extension(false)
        .map_err(|e| format!("Failed to configure database: {}", e))?;
    let conn = Connection::open_in_memory_with_flags(config)
        .map_err(|e| format!("Failed to open in-memory database: {}", e))?;

    conn.execute(
        &format!(
            "ATTACH '{}' AS test_db (ENCRYPTION_KEY '{}', READ_ONLY)",
            db_path.display(),
            key_hex
        ),
        [],
    )
    .map_err(|_| "Invalid password")?;

    // Verify we can actually read from the database
    conn.execute("USE test_db", [])
        .map_err(|_| "Invalid password")?;
    conn.execute(
        "SELECT table_name FROM information_schema.tables LIMIT 1",
        [],
    )
    .map_err(|_| "Invalid password")?;

    // Store key in memory for this session
    let mut key_guard = encryption_state
        .key
        .lock()
        .map_err(|_| "Failed to lock encryption state")?;
    *key_guard = Some(key_hex);

    Ok(())
}

/// Enable encryption using treeline-core EncryptionService
#[tauri::command]
async fn enable_encryption(
    password: String,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<(), String> {
    // Invalidate the shared context first to release the database connection
    // This allows the EncryptionService to get exclusive access
    context_state.invalidate();

    // Clone password for use in spawn_blocking
    let password_clone = password.clone();

    // Run encryption in blocking task
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let demo_mode = get_demo_mode();

        let db_filename = if demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };
        let db_path = treeline_dir.join(db_filename);

        let encryption_service = EncryptionService::new(treeline_dir.clone(), db_path);
        let backup_service = BackupService::new(treeline_dir, db_filename.to_string());

        encryption_service
            .encrypt(&password_clone, &backup_service)
            .map_err(|e| format!("{:#}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    // After successful encryption, derive key and store in memory
    // so user doesn't need to re-enter password immediately (this session only)
    let metadata =
        read_encryption_metadata().ok_or("Encryption succeeded but couldn't read metadata")?;

    let salt = BASE64
        .decode(&metadata.salt)
        .map_err(|e| format!("Failed to decode salt: {}", e))?;

    let key_bytes = derive_key(&password, &salt, &metadata.argon2_params)?;
    let key_hex = hex::encode(&key_bytes);

    // Store in memory for this session
    let mut key_guard = encryption_state
        .key
        .lock()
        .map_err(|_| "Failed to lock encryption state")?;
    *key_guard = Some(key_hex);

    Ok(())
}

/// Disable encryption using treeline-core EncryptionService
#[tauri::command]
async fn disable_encryption(
    password: String,
    encryption_state: State<'_, EncryptionState>,
    context_state: State<'_, TreelineContextState>,
) -> Result<(), String> {
    // Invalidate the shared context first to release the database connection
    // This allows the EncryptionService to get exclusive access
    context_state.invalidate();

    // Run decryption in blocking task
    tauri::async_runtime::spawn_blocking(move || {
        let treeline_dir = get_treeline_dir()?;
        let demo_mode = get_demo_mode();

        let db_filename = if demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };
        let db_path = treeline_dir.join(db_filename);

        let encryption_service = EncryptionService::new(treeline_dir.clone(), db_path);
        let backup_service = BackupService::new(treeline_dir, db_filename.to_string());

        encryption_service
            .decrypt(&password, &backup_service)
            .map_err(|e| format!("{:#}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    // Clear encryption key from memory
    let mut key_guard = encryption_state
        .key
        .lock()
        .map_err(|_| "Failed to lock encryption state")?;
    *key_guard = None;

    Ok(())
}

#[tauri::command]
fn read_plugin_config(plugin_id: String, filename: String) -> Result<String, String> {
    let treeline_dir = get_treeline_dir()?;
    let config_path = treeline_dir
        .join("plugins")
        .join(&plugin_id)
        .join(&filename);

    if !config_path.exists() {
        return Ok("null".to_string());
    }

    fs::read_to_string(&config_path).map_err(|e| format!("Failed to read config: {}", e))
}

#[tauri::command]
fn write_plugin_config(plugin_id: String, filename: String, content: String) -> Result<(), String> {
    let treeline_dir = get_treeline_dir()?;
    let plugin_dir = treeline_dir.join("plugins").join(&plugin_id);

    // Create plugin directory if it doesn't exist
    if !plugin_dir.exists() {
        fs::create_dir_all(&plugin_dir)
            .map_err(|e| format!("Failed to create plugin directory: {}", e))?;
    }

    let config_path = plugin_dir.join(&filename);

    // Create parent directories if filename contains subdirectories (e.g., "months/2025-12.json")
    if let Some(parent) = config_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
    }

    fs::write(&config_path, content).map_err(|e| format!("Failed to write config: {}", e))
}

#[tauri::command]
fn discover_plugins() -> Result<Vec<ExternalPlugin>, String> {
    let treeline_dir = get_treeline_dir()?;
    let plugins_dir = treeline_dir.join("plugins");

    // Create directory if it doesn't exist
    if !plugins_dir.exists() {
        fs::create_dir_all(&plugins_dir)
            .map_err(|e| format!("Failed to create plugins directory: {}", e))?;
        return Ok(Vec::new());
    }

    let mut plugins = Vec::new();

    // Read all subdirectories in plugins directory
    let entries = fs::read_dir(&plugins_dir)
        .map_err(|e| format!("Failed to read plugins directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.is_dir() {
            let manifest_path = path.join("manifest.json");

            if manifest_path.exists() {
                // Read and parse manifest
                let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| {
                    format!("Failed to read manifest at {:?}: {}", manifest_path, e)
                })?;

                let manifest: PluginManifest =
                    serde_json::from_str(&manifest_content).map_err(|e| {
                        format!("Failed to parse manifest at {:?}: {}", manifest_path, e)
                    })?;

                // Get the plugin directory name
                let plugin_dir_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| format!("Invalid plugin directory name: {:?}", path))?;

                plugins.push(ExternalPlugin {
                    manifest,
                    path: format!("plugins/{}/{}", plugin_dir_name, "index.js"),
                });
            }
        }
    }

    Ok(plugins)
}

/// Start watching the plugins directory for file changes (hot-reload).
/// Emits "plugin-file-changed" events with the plugin ID when index.js or manifest.json change.
#[tauri::command]
fn watch_plugins_dir(
    app: AppHandle,
    watcher_state: State<'_, PluginWatcherState>,
) -> Result<(), String> {
    let treeline_dir = get_treeline_dir()?;
    let plugins_dir = treeline_dir.join("plugins");

    if !plugins_dir.exists() {
        fs::create_dir_all(&plugins_dir)
            .map_err(|e| format!("Failed to create plugins directory: {}", e))?;
    }

    let plugins_dir_clone = plugins_dir.clone();
    let debouncer = new_debouncer(Duration::from_millis(500), move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
        match res {
            Ok(events) => {
                for event in events {
                    if event.kind != DebouncedEventKind::Any {
                        continue;
                    }
                    let path = &event.path;
                    // Only react to index.js or manifest.json changes
                    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                        if filename != "index.js" && filename != "manifest.json" {
                            continue;
                        }
                    } else {
                        continue;
                    }

                    // Extract plugin ID from path: plugins_dir/<plugin-id>/filename
                    if let Ok(relative) = path.strip_prefix(&plugins_dir_clone) {
                        if let Some(plugin_id) = relative.iter().next().and_then(|c| c.to_str()) {
                            let _ = app.emit("plugin-file-changed", plugin_id.to_string());
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Plugin watcher error: {:?}", e);
            }
        }
    })
    .map_err(|e| format!("Failed to create file watcher: {}", e))?;

    let mut watcher_lock = watcher_state.watcher.lock().unwrap();

    // Use the watcher from the debouncer to add the watch path
    let debouncer = {
        let mut d = debouncer;
        d.watcher()
            .watch(&plugins_dir, notify::RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch plugins directory: {}", e))?;
        d
    };

    *watcher_lock = Some(debouncer);
    Ok(())
}

/// Stop watching the plugins directory.
#[tauri::command]
fn unwatch_plugins_dir(watcher_state: State<'_, PluginWatcherState>) -> Result<(), String> {
    let mut watcher_lock = watcher_state.watcher.lock().unwrap();
    // Dropping the debouncer stops the watcher
    *watcher_lock = None;
    Ok(())
}

/// Delete an account and all associated data (transactions, balance snapshots)
/// This is a cascading delete - all transactions and snapshots for the account are removed
#[tauri::command]
fn delete_account(
    account_id: String,
    encryption_state: State<EncryptionState>,
    context_state: State<TreelineContextState>,
) -> Result<(), String> {
    let key = get_encryption_key(&encryption_state)?;
    let ctx_guard = get_or_create_context(&context_state, key)?;
    let ctx = ctx_guard.as_ref().unwrap();

    ctx.repository
        .delete_account(&account_id)
        .map_err(|e| format!("Failed to delete account: {}", e))
}

/// Run database migrations using treeline-core
/// Called on app startup to ensure schema is up to date
#[tauri::command]
fn run_migrations(
    encryption_state: State<EncryptionState>,
    context_state: State<TreelineContextState>,
) -> Result<(), String> {
    let key = get_encryption_key(&encryption_state)?;
    // TreelineContext::new() calls ensure_schema() which runs migrations
    // Getting or creating the shared context ensures migrations run
    let _ctx_guard = get_or_create_context(&context_state, key)?;
    Ok(())
}

// ============================================================================
// Logging Commands
// ============================================================================

/// Log a page/view navigation from the frontend
/// Privacy: Only logs the view name, never any user data
#[tauri::command]
fn log_page(page: String, logging_state: State<LoggingState>) -> Result<(), String> {
    let guard = logging_state
        .logger
        .lock()
        .map_err(|_| "Lock failed".to_string())?;
    if let Some(logger) = guard.as_ref() {
        let _ = logger.log_page(&page); // Silently ignore errors
    }
    Ok(())
}

/// Log a user action from the frontend
/// Privacy: Only logs action/component names, never any user data
#[tauri::command]
fn log_action(
    action: String,
    component: String,
    logging_state: State<LoggingState>,
) -> Result<(), String> {
    let guard = logging_state
        .logger
        .lock()
        .map_err(|_| "Lock failed".to_string())?;
    if let Some(logger) = guard.as_ref() {
        let event = LogEvent::new(&action).with_page(&component);
        let _ = logger.log(event); // Silently ignore errors
    }
    Ok(())
}

/// Log an error from the frontend
/// Privacy: Error messages should be sanitized by the frontend before logging
#[tauri::command]
fn log_error(
    event: String,
    message: String,
    details: Option<String>,
    logging_state: State<LoggingState>,
) -> Result<(), String> {
    let guard = logging_state
        .logger
        .lock()
        .map_err(|_| "Lock failed".to_string())?;
    if let Some(logger) = guard.as_ref() {
        let _ = logger.log_error(&event, &message, details.as_deref());
    }
    Ok(())
}

/// Get the path to the logs database for support purposes
#[tauri::command]
fn get_logs_path(logging_state: State<LoggingState>) -> Result<Option<String>, String> {
    let guard = logging_state
        .logger
        .lock()
        .map_err(|_| "Lock failed".to_string())?;
    Ok(guard
        .as_ref()
        .map(|l| l.db_path().to_string_lossy().to_string()))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // Key Derivation Tests (Security Critical)
    // ============================================================================

    #[test]
    fn test_derive_key_basic() {
        let password = "test_password";
        let salt = b"16_byte_salt____"; // 16 bytes
        let params = Argon2Params {
            time_cost: 2,
            memory_cost: 19456, // ~19 MB (reduced for tests)
            parallelism: 1,
            hash_len: 32,
        };

        let key = derive_key(password, salt, &params).expect("Key derivation should succeed");
        assert_eq!(key.len(), 32, "Key should be 32 bytes");
    }

    #[test]
    fn test_derive_key_deterministic() {
        // Same password + salt + params should always produce same key
        let password = "test_password";
        let salt = b"16_byte_salt____";
        let params = Argon2Params {
            time_cost: 2,
            memory_cost: 19456,
            parallelism: 1,
            hash_len: 32,
        };

        let key1 = derive_key(password, salt, &params).unwrap();
        let key2 = derive_key(password, salt, &params).unwrap();
        assert_eq!(key1, key2, "Same inputs should produce same key");
    }

    #[test]
    fn test_derive_key_different_passwords() {
        let salt = b"16_byte_salt____";
        let params = Argon2Params {
            time_cost: 2,
            memory_cost: 19456,
            parallelism: 1,
            hash_len: 32,
        };

        let key1 = derive_key("password1", salt, &params).unwrap();
        let key2 = derive_key("password2", salt, &params).unwrap();
        assert_ne!(
            key1, key2,
            "Different passwords should produce different keys"
        );
    }

    #[test]
    fn test_derive_key_different_salts() {
        let password = "test_password";
        let params = Argon2Params {
            time_cost: 2,
            memory_cost: 19456,
            parallelism: 1,
            hash_len: 32,
        };

        let key1 = derive_key(password, b"salt_one________", &params).unwrap();
        let key2 = derive_key(password, b"salt_two________", &params).unwrap();
        assert_ne!(key1, key2, "Different salts should produce different keys");
    }

    #[test]
    fn test_derive_key_custom_hash_length() {
        let password = "test_password";
        let salt = b"16_byte_salt____";
        let params = Argon2Params {
            time_cost: 2,
            memory_cost: 19456,
            parallelism: 1,
            hash_len: 64, // 64 byte key
        };

        let key = derive_key(password, salt, &params).unwrap();
        assert_eq!(key.len(), 64, "Key should be 64 bytes");
    }

    #[test]
    fn test_derive_key_empty_password() {
        // Empty password should still work (Argon2 handles it)
        let salt = b"16_byte_salt____";
        let params = Argon2Params {
            time_cost: 2,
            memory_cost: 19456,
            parallelism: 1,
            hash_len: 32,
        };

        let result = derive_key("", salt, &params);
        assert!(result.is_ok(), "Empty password should still derive a key");
    }

    #[test]
    fn test_derive_key_unicode_password() {
        // Unicode passwords should work correctly
        let password = "пароль密码🔐";
        let salt = b"16_byte_salt____";
        let params = Argon2Params {
            time_cost: 2,
            memory_cost: 19456,
            parallelism: 1,
            hash_len: 32,
        };

        let key = derive_key(password, salt, &params).expect("Unicode password should work");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_derive_key_long_password() {
        // Very long password should work
        let password = "a".repeat(10000);
        let salt = b"16_byte_salt____";
        let params = Argon2Params {
            time_cost: 2,
            memory_cost: 19456,
            parallelism: 1,
            hash_len: 32,
        };

        let key = derive_key(&password, salt, &params).expect("Long password should work");
        assert_eq!(key.len(), 32);
    }

    // ============================================================================
    // Settings Tests
    // ============================================================================

    #[test]
    fn test_default_settings_structure() {
        // Verify default settings JSON structure
        let default_settings = serde_json::json!({
            "app": {
                "theme": "dark",
                "lastSyncDate": null,
                "autoSyncOnStartup": true
            },
            "plugins": {}
        });

        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&default_settings.to_string())
            .expect("Default settings should be valid JSON");

        assert!(parsed["app"].is_object());
        assert_eq!(parsed["app"]["theme"], "dark");
        assert!(parsed["plugins"].is_object());
    }

    // ============================================================================
    // EncryptionMetadata Tests
    // ============================================================================

    #[test]
    fn test_encryption_metadata_serde() {
        let metadata = EncryptionMetadata {
            encrypted: true,
            salt: "dGVzdF9zYWx0".to_string(), // base64 for "test_salt"
            algorithm: "argon2id".to_string(),
            version: 1,
            argon2_params: Argon2Params {
                time_cost: 3,
                memory_cost: 65536,
                parallelism: 4,
                hash_len: 32,
            },
        };

        // Serialize
        let json = serde_json::to_string(&metadata).expect("Should serialize");

        // Deserialize
        let parsed: EncryptionMetadata = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(parsed.encrypted, true);
        assert_eq!(parsed.salt, "dGVzdF9zYWx0");
        assert_eq!(parsed.algorithm, "argon2id");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.argon2_params.time_cost, 3);
    }

    // ============================================================================
    // EncryptionStatus Tests
    // ============================================================================

    #[test]
    fn test_encryption_status_unencrypted() {
        let status = EncryptionStatus {
            encrypted: false,
            locked: false,
            algorithm: None,
            version: None,
        };

        let json = serde_json::to_string(&status).expect("Should serialize");
        assert!(json.contains("\"encrypted\":false"));
        assert!(json.contains("\"locked\":false"));
    }

    #[test]
    fn test_encryption_status_encrypted_locked() {
        let status = EncryptionStatus {
            encrypted: true,
            locked: true,
            algorithm: Some("argon2id".to_string()),
            version: Some(1),
        };

        let json = serde_json::to_string(&status).expect("Should serialize");
        assert!(json.contains("\"encrypted\":true"));
        assert!(json.contains("\"locked\":true"));
    }

    // ============================================================================
    // PluginManifest Tests
    // ============================================================================

    #[test]
    fn test_plugin_manifest_minimal() {
        let json = r#"{"id": "test", "name": "Test Plugin"}"#;
        let manifest: PluginManifest = serde_json::from_str(json).expect("Should parse");

        assert_eq!(manifest.id, "test");
        assert_eq!(manifest.name, "Test Plugin");
        assert_eq!(manifest.main, "index.js"); // default
        assert!(manifest.version.is_empty()); // default empty
    }

    #[test]
    fn test_plugin_manifest_full() {
        let json = r#"{
            "id": "budget",
            "name": "Budget Plugin",
            "version": "1.0.0",
            "description": "Budget tracking",
            "author": "Test Author",
            "main": "src/main.js",
            "permissions": {"reads": ["accounts"]},
            "source": "https://github.com/test/budget"
        }"#;

        let manifest: PluginManifest = serde_json::from_str(json).expect("Should parse");

        assert_eq!(manifest.id, "budget");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.main, "src/main.js");
        assert!(manifest.source.is_some());
    }

    // ============================================================================
    // ThemeDefinition Tests
    // ============================================================================

    #[test]
    fn test_theme_definition_serde() {
        let theme = ThemeDefinition {
            id: "dark".to_string(),
            name: "Dark Theme".to_string(),
            extends: None,
            variables: {
                let mut vars = std::collections::HashMap::new();
                vars.insert("--bg-primary".to_string(), "#1a1a1a".to_string());
                vars.insert("--text-primary".to_string(), "#ffffff".to_string());
                vars
            },
        };

        let json = serde_json::to_string(&theme).expect("Should serialize");
        let parsed: ThemeDefinition = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(parsed.id, "dark");
        assert_eq!(parsed.name, "Dark Theme");
        assert_eq!(parsed.variables.len(), 2);
    }

    #[test]
    fn test_theme_definition_with_extends() {
        let json = r##"{
            "id": "custom",
            "name": "Custom Theme",
            "extends": "dark",
            "variables": {"--accent-primary": "#ff0000"}
        }"##;

        let theme: ThemeDefinition = serde_json::from_str(json).expect("Should parse");
        assert_eq!(theme.extends, Some("dark".to_string()));
    }

    // ============================================================================
    // PendingImportFile Tests
    // ============================================================================

    #[test]
    fn test_pending_import_file_serde() {
        let file = PendingImportFile {
            path: "/home/user/.treeline/imports/test.csv".to_string(),
            filename: "test.csv".to_string(),
            size_bytes: 1024,
        };

        let json = serde_json::to_string(&file).expect("Should serialize");
        assert!(json.contains("test.csv"));
        assert!(json.contains("1024"));
    }

    // ============================================================================
    // AppUpdateInfo Tests
    // ============================================================================

    #[test]
    fn test_app_update_info_serde() {
        let info = AppUpdateInfo {
            version: "1.2.3".to_string(),
            body: Some("Bug fixes and improvements".to_string()),
            date: Some("2024-01-15".to_string()),
        };

        let json = serde_json::to_string(&info).expect("Should serialize");
        assert!(json.contains("1.2.3"));
        assert!(json.contains("Bug fixes"));
    }

    #[test]
    fn test_app_update_info_minimal() {
        let info = AppUpdateInfo {
            version: "1.0.0".to_string(),
            body: None,
            date: None,
        };

        let json = serde_json::to_string(&info).expect("Should serialize");
        assert!(json.contains("1.0.0"));
        assert!(json.contains("null"));
    }

    // ============================================================================
    // TreelineContextState Tests
    // ============================================================================

    #[test]
    fn test_treeline_context_state_default() {
        let state = TreelineContextState::default();

        // Context should start as None
        let ctx = state.context.lock().unwrap();
        assert!(ctx.is_none());

        let key = state.context_key.lock().unwrap();
        assert!(key.is_none());
    }

    #[test]
    fn test_treeline_context_state_invalidate() {
        let state = TreelineContextState::default();

        // Set some value to context_key
        {
            let mut key = state.context_key.lock().unwrap();
            *key = Some("test_key".to_string());
        }

        // Invalidate
        state.invalidate();

        // Should be None now
        let key = state.context_key.lock().unwrap();
        assert!(key.is_none());
    }

    // ============================================================================
    // EncryptionState Tests
    // ============================================================================

    #[test]
    fn test_encryption_state_default() {
        let state = EncryptionState::default();

        let key = state.key.lock().unwrap();
        assert!(key.is_none());
    }

    // ============================================================================
    // DevtoolsState Tests
    // ============================================================================

    #[test]
    fn test_devtools_state_default() {
        let state = DevtoolsState::default();
        assert!(!state.open.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_devtools_state_toggle() {
        let state = DevtoolsState::default();

        state.open.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(state.open.load(std::sync::atomic::Ordering::SeqCst));

        state.open.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(!state.open.load(std::sync::atomic::Ordering::SeqCst));
    }

    // ============================================================================
    // Demo Mode Tests (Environment Variable Parsing)
    // ============================================================================

    // Note: These tests verify the parsing logic for demo mode
    // The actual get_demo_mode function reads from files/env which is harder to test

    #[test]
    fn test_demo_mode_env_parsing() {
        // Test the boolean parsing logic used in get_demo_mode
        let parse_bool = |s: &str| -> Option<bool> {
            let lower = s.to_lowercase();
            if lower == "true" || lower == "1" || lower == "yes" {
                Some(true)
            } else if lower == "false" || lower == "0" || lower == "no" {
                Some(false)
            } else {
                None
            }
        };

        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("TRUE"), Some(true));
        assert_eq!(parse_bool("True"), Some(true));
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("yes"), Some(true));
        assert_eq!(parse_bool("YES"), Some(true));

        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("FALSE"), Some(false));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("no"), Some(false));
        assert_eq!(parse_bool("NO"), Some(false));

        assert_eq!(parse_bool("maybe"), None);
        assert_eq!(parse_bool(""), None);
    }

    // ============================================================================
    // CSV Delimiter Detection Tests
    // ============================================================================

    #[test]
    fn test_detect_csv_delimiter_comma() {
        // US-style CSV with commas
        assert_eq!(detect_csv_delimiter("Date,Amount,Description"), b',');
        assert_eq!(detect_csv_delimiter("a,b,c,d,e"), b',');
    }

    #[test]
    fn test_detect_csv_delimiter_semicolon() {
        // EU-style CSV with semicolons
        assert_eq!(detect_csv_delimiter("Date;Amount;Description"), b';');
        assert_eq!(detect_csv_delimiter("a;b;c;d;e"), b';');
    }

    #[test]
    fn test_detect_csv_delimiter_tab() {
        // Tab-separated values
        assert_eq!(detect_csv_delimiter("Date\tAmount\tDescription"), b'\t');
        assert_eq!(detect_csv_delimiter("a\tb\tc\td\te"), b'\t');
    }

    #[test]
    fn test_detect_csv_delimiter_mixed_prefers_most_common() {
        // When mixed, should prefer the most common delimiter
        // 3 semicolons vs 1 comma
        assert_eq!(detect_csv_delimiter("a;b;c;d,e"), b';');
        // 3 commas vs 1 semicolon
        assert_eq!(detect_csv_delimiter("a,b,c,d;e"), b',');
        // 3 tabs vs 2 commas
        assert_eq!(detect_csv_delimiter("a\tb\tc\td,e,f"), b'\t');
    }

    #[test]
    fn test_detect_csv_delimiter_no_delimiters() {
        // No delimiters - defaults to comma
        assert_eq!(detect_csv_delimiter("SingleColumn"), b',');
        assert_eq!(detect_csv_delimiter(""), b',');
    }

    #[test]
    fn test_detect_csv_delimiter_equal_counts() {
        // When counts are equal, comma wins (US default)
        assert_eq!(detect_csv_delimiter("a,b;c"), b','); // 1 comma, 1 semicolon
        assert_eq!(detect_csv_delimiter("a,b\tc"), b','); // 1 comma, 1 tab
    }

    #[test]
    fn test_detect_csv_delimiter_with_quoted_values() {
        // Delimiters inside quotes should still be counted
        // (This is a limitation - we count all occurrences, not just structural ones)
        // But in practice, the structural delimiters usually outnumber quoted ones
        assert_eq!(
            detect_csv_delimiter(r#""Hello, World",Value1,Value2"#),
            b','
        );
    }

    #[test]
    fn test_detect_csv_delimiter_real_world_us_bank() {
        // Real-world US bank export format
        let line = "Transaction Date,Post Date,Description,Category,Type,Amount,Memo";
        assert_eq!(detect_csv_delimiter(line), b',');
    }

    #[test]
    fn test_detect_csv_delimiter_real_world_eu_bank() {
        // Real-world EU bank export format (German style)
        let line = "Buchungstag;Wertstellung;Buchungstext;Auftraggeber;Verwendungszweck;Betrag";
        assert_eq!(detect_csv_delimiter(line), b';');
    }

    #[test]
    fn test_detect_csv_delimiter_real_world_tsv() {
        // Real-world TSV export
        let line = "Date\tPayee\tCategory\tMemo\tOutflow\tInflow";
        assert_eq!(detect_csv_delimiter(line), b'\t');
    }

    // ============================================================================
    // CSV Header Parsing Tests
    // ============================================================================

    #[test]
    fn test_parse_csv_headers_basic() {
        let headers = parse_csv_headers("Date,Amount,Description", b',').unwrap();
        assert_eq!(headers, vec!["Date", "Amount", "Description"]);
    }

    #[test]
    fn test_parse_csv_headers_with_whitespace() {
        let headers = parse_csv_headers("  Date  ,  Amount  ,  Description  ", b',').unwrap();
        assert_eq!(headers, vec!["Date", "Amount", "Description"]);
    }

    #[test]
    fn test_parse_csv_headers_with_hash_prefix() {
        // Some exports have # prefix on first column
        let headers = parse_csv_headers("#Date,Amount,Description", b',').unwrap();
        assert_eq!(headers, vec!["Date", "Amount", "Description"]);
    }

    #[test]
    fn test_parse_csv_headers_semicolon() {
        let headers = parse_csv_headers("Datum;Betrag;Beschreibung", b';').unwrap();
        assert_eq!(headers, vec!["Datum", "Betrag", "Beschreibung"]);
    }

    #[test]
    fn test_parse_csv_headers_tab() {
        let headers = parse_csv_headers("Date\tAmount\tDescription", b'\t').unwrap();
        assert_eq!(headers, vec!["Date", "Amount", "Description"]);
    }

    #[test]
    fn test_parse_csv_headers_quoted() {
        let headers = parse_csv_headers(r#""Date","Amount","Description""#, b',').unwrap();
        assert_eq!(headers, vec!["Date", "Amount", "Description"]);
    }

    #[test]
    fn test_parse_csv_headers_quoted_with_comma() {
        let headers =
            parse_csv_headers(r#""Date, Time",Amount,"Description, Notes""#, b',').unwrap();
        assert_eq!(headers, vec!["Date, Time", "Amount", "Description, Notes"]);
    }

    #[test]
    fn test_parse_csv_headers_single_column() {
        let headers = parse_csv_headers("SingleColumn", b',').unwrap();
        assert_eq!(headers, vec!["SingleColumn"]);
    }

    #[test]
    fn test_parse_csv_headers_empty_columns() {
        let headers = parse_csv_headers("Date,,Description", b',').unwrap();
        assert_eq!(headers, vec!["Date", "", "Description"]);
    }

    #[test]
    fn test_parse_csv_headers_unicode() {
        let headers = parse_csv_headers("日期,金额,描述", b',').unwrap();
        assert_eq!(headers, vec!["日期", "金额", "描述"]);
    }

    #[test]
    fn test_parse_csv_headers_real_world() {
        // Real Chase bank export header
        let line = "Transaction Date,Post Date,Description,Category,Type,Amount,Memo";
        let headers = parse_csv_headers(line, b',').unwrap();
        assert_eq!(
            headers,
            vec![
                "Transaction Date",
                "Post Date",
                "Description",
                "Category",
                "Type",
                "Amount",
                "Memo"
            ]
        );
    }

    // ============================================================================
    // Date Parsing Tests (used in backfill commands)
    // ============================================================================

    #[test]
    fn test_date_parsing_valid() {
        use chrono::{Datelike, NaiveDate};

        let date = NaiveDate::parse_from_str("2024-01-15", "%Y-%m-%d");
        assert!(date.is_ok());
        let d = date.unwrap();
        assert_eq!(d.year(), 2024);
        assert_eq!(d.month(), 1);
        assert_eq!(d.day(), 15);
    }

    #[test]
    fn test_date_parsing_invalid_format() {
        use chrono::NaiveDate;

        // US format (should fail with %Y-%m-%d)
        assert!(NaiveDate::parse_from_str("01/15/2024", "%Y-%m-%d").is_err());
        // EU format (should fail)
        assert!(NaiveDate::parse_from_str("15-01-2024", "%Y-%m-%d").is_err());
        // Note: chrono is lenient - "2024-1-5" parses successfully with %Y-%m-%d
    }

    #[test]
    fn test_date_parsing_edge_cases() {
        use chrono::NaiveDate;

        // Leap year
        assert!(NaiveDate::parse_from_str("2024-02-29", "%Y-%m-%d").is_ok());
        // Non-leap year Feb 29 should fail
        assert!(NaiveDate::parse_from_str("2023-02-29", "%Y-%m-%d").is_err());
        // End of year
        assert!(NaiveDate::parse_from_str("2024-12-31", "%Y-%m-%d").is_ok());
        // Invalid month
        assert!(NaiveDate::parse_from_str("2024-13-01", "%Y-%m-%d").is_err());
        // Invalid day
        assert!(NaiveDate::parse_from_str("2024-01-32", "%Y-%m-%d").is_err());
    }

    // ============================================================================
    // Decimal Conversion Tests (used in backfill commands)
    // ============================================================================

    #[test]
    fn test_decimal_from_f64_basic() {
        use rust_decimal::Decimal;

        let d = Decimal::try_from(100.50f64);
        assert!(d.is_ok());
    }

    #[test]
    fn test_decimal_from_f64_negative() {
        use rust_decimal::Decimal;

        let d = Decimal::try_from(-1234.56f64);
        assert!(d.is_ok());
        let val = d.unwrap();
        assert!(val.is_sign_negative());
    }

    #[test]
    fn test_decimal_from_f64_zero() {
        use rust_decimal::Decimal;

        let d = Decimal::try_from(0.0f64);
        assert!(d.is_ok());
        assert!(d.unwrap().is_zero());
    }

    #[test]
    fn test_decimal_from_f64_large_value() {
        use rust_decimal::Decimal;

        // Large but valid balance
        let d = Decimal::try_from(999_999_999.99f64);
        assert!(d.is_ok());
    }

    #[test]
    fn test_decimal_from_f64_small_precision() {
        use rust_decimal::Decimal;

        // Typical currency precision
        let d = Decimal::try_from(0.01f64);
        assert!(d.is_ok());
    }

    #[test]
    fn test_decimal_from_f64_infinity_fails() {
        use rust_decimal::Decimal;

        // Infinity should fail conversion
        let d = Decimal::try_from(f64::INFINITY);
        assert!(d.is_err());

        let d = Decimal::try_from(f64::NEG_INFINITY);
        assert!(d.is_err());
    }

    #[test]
    fn test_decimal_from_f64_nan_fails() {
        use rust_decimal::Decimal;

        // NaN should fail conversion
        let d = Decimal::try_from(f64::NAN);
        assert!(d.is_err());
    }

    // ============================================================================
    // Plugin Migration Tests
    // ============================================================================
    // These tests verify the database operations that happen during plugin
    // installation. They use TreelineContext directly, which is the same code
    // path that the Tauri commands use.

    /// Test a full plugin migration scenario using TreelineContext.
    /// This mirrors what happens in plugins/index.ts when a plugin is installed:
    /// 1. Create plugin schema
    /// 2. Create migration tracking table
    /// 3. Create plugin's data table (DDL)
    /// 4. Record migration
    /// 5. Run CHECKPOINT to flush WAL
    #[test]
    fn test_plugin_migration_scenario() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");

        // Create TreelineContext (same as what Tauri commands use)
        let ctx =
            TreelineContext::new(temp_dir.path(), None).expect("Failed to create TreelineContext");

        let plugin_id = "subscriptions";
        let schema_name = format!("plugin_{}", plugin_id);

        // Step 1: Create schema (mirrors plugins/index.ts line 51)
        let result = ctx
            .query_service
            .execute_sql(&format!("CREATE SCHEMA IF NOT EXISTS {}", schema_name));
        assert!(
            result.is_ok(),
            "Create schema should succeed: {:?}",
            result.err()
        );

        // Step 2: Create schema_migrations table (mirrors plugins/index.ts lines 55-61)
        let result = ctx.query_service.execute_sql(&format!(
            "CREATE TABLE IF NOT EXISTS {}.schema_migrations (
                version INTEGER PRIMARY KEY,
                name VARCHAR NOT NULL,
                executed_at TIMESTAMP
            )",
            schema_name
        ));
        assert!(
            result.is_ok(),
            "Create migrations table should succeed: {:?}",
            result.err()
        );

        // Step 3: Run plugin's migration DDL (simulating the subscriptions plugin)
        let result = ctx.query_service.execute_sql(&format!(
            "CREATE TABLE IF NOT EXISTS {}.subscriptions (
                merchant_key VARCHAR PRIMARY KEY,
                hidden_at TIMESTAMP
            )",
            schema_name
        ));
        assert!(
            result.is_ok(),
            "Create plugin table should succeed: {:?}",
            result.err()
        );

        // Step 4: Record the migration (mirrors plugins/index.ts lines 93-97)
        let result = ctx.query_service.execute_sql(&format!(
            "INSERT INTO {}.schema_migrations (version, name, executed_at)
             VALUES (1, 'create_subscriptions_table', CURRENT_TIMESTAMP)",
            schema_name
        ));
        assert!(
            result.is_ok(),
            "Record migration should succeed: {:?}",
            result.err()
        );

        // Step 5: CHECKPOINT to flush WAL (mirrors plugins/index.ts line 109)
        // THIS WAS THE BUG - CHECKPOINT was being rejected by SQL validation
        let result = ctx.query_service.execute_sql("CHECKPOINT");
        assert!(
            result.is_ok(),
            "CHECKPOINT after migration should succeed: {:?}",
            result.err()
        );

        // Verify: Check migration was recorded
        let result = ctx
            .query_service
            .execute(&format!(
                "SELECT version, name FROM {}.schema_migrations",
                schema_name
            ))
            .expect("Query should succeed");
        assert_eq!(result.rows.len(), 1, "Should have 1 migration recorded");

        // Verify: Check table exists by inserting and querying
        let result = ctx.query_service.execute_sql(&format!(
            "INSERT INTO {}.subscriptions (merchant_key) VALUES ('test_merchant')",
            schema_name
        ));
        assert!(result.is_ok(), "Insert into plugin table should succeed");

        let result = ctx
            .query_service
            .execute(&format!(
                "SELECT merchant_key FROM {}.subscriptions",
                schema_name
            ))
            .expect("Query should succeed");
        assert_eq!(result.rows.len(), 1, "Should have 1 subscription");
    }

    /// Test multiple plugins can have independent schemas and migrations
    #[test]
    fn test_multiple_plugin_schemas() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let ctx =
            TreelineContext::new(temp_dir.path(), None).expect("Failed to create TreelineContext");

        let plugins = vec!["plugin_budget", "plugin_goals", "plugin_subscriptions"];

        // Create schema and table for each plugin
        for schema_name in &plugins {
            ctx.query_service
                .execute_sql(&format!("CREATE SCHEMA IF NOT EXISTS {}", schema_name))
                .expect("Create schema should succeed");

            ctx.query_service
                .execute_sql(&format!(
                    "CREATE TABLE {}.data (id INTEGER PRIMARY KEY)",
                    schema_name
                ))
                .expect("Create table should succeed");

            ctx.query_service
                .execute_sql(&format!("INSERT INTO {}.data VALUES (1)", schema_name))
                .expect("Insert should succeed");
        }

        // CHECKPOINT once after all migrations
        ctx.query_service
            .execute_sql("CHECKPOINT")
            .expect("CHECKPOINT should succeed");

        // Verify each plugin's data is independent
        for schema_name in &plugins {
            let result = ctx
                .query_service
                .execute(&format!("SELECT COUNT(*) FROM {}.data", schema_name))
                .expect("Query should succeed");
            assert_eq!(
                result.rows[0][0],
                serde_json::json!(1),
                "Each plugin should have its own data"
            );
        }
    }

    /// Test that CHECKPOINT works with different cases
    #[test]
    fn test_checkpoint_case_insensitive() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let ctx =
            TreelineContext::new(temp_dir.path(), None).expect("Failed to create TreelineContext");

        // All case variations should work
        assert!(
            ctx.query_service.execute_sql("CHECKPOINT").is_ok(),
            "CHECKPOINT (uppercase) should work"
        );
        assert!(
            ctx.query_service.execute_sql("checkpoint").is_ok(),
            "checkpoint (lowercase) should work"
        );
        assert!(
            ctx.query_service.execute_sql("Checkpoint").is_ok(),
            "Checkpoint (mixed case) should work"
        );
    }

    /// Test that VACUUM command works through TreelineContext
    #[test]
    fn test_vacuum_command() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let ctx =
            TreelineContext::new(temp_dir.path(), None).expect("Failed to create TreelineContext");

        // VACUUM should succeed
        let result = ctx.query_service.execute_sql("VACUUM");
        assert!(result.is_ok(), "VACUUM should succeed: {:?}", result.err());
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // On Linux, set WEBKIT_DISABLE_COMPOSITING_MODE to avoid EGL initialization failures
    // This fixes "Could not create default EGL display: EGL_BAD_PARAMETER" errors
    // that can occur with certain GPU drivers, in VMs, or when running AppImages.
    // Only set if not already defined, allowing users to override via environment.
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").is_err() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
    }

    tauri::Builder::default()
        .manage(EncryptionState::default())
        .manage(DevtoolsState::default())
        .manage(AppUpdateState::default())
        .manage(TreelineContextState::default())
        .manage(LoggingState::default())
        .manage(PluginWatcherState::default())
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            let devtools_state = app.state::<DevtoolsState>();
            let logging_state = app.state::<LoggingState>();

            // Initialize logging service
            if let Ok(treeline_dir) = get_treeline_dir() {
                match LoggingService::new(
                    &treeline_dir,
                    EntryPoint::Desktop,
                    env!("CARGO_PKG_VERSION"),
                ) {
                    Ok(logger) => {
                        // Log app startup
                        let _ = logger.log_event("app_started");
                        if let Ok(mut guard) = logging_state.logger.lock() {
                            *guard = Some(logger);
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to initialize logging: {}", e);
                        // Continue without logging - it should never block app startup
                    }
                }
            }

            // If TREELINE_DIR is set (dev/testing), add its plugins dir to asset protocol scope
            if let Ok(custom_dir) = std::env::var("TREELINE_DIR") {
                let plugins_path = PathBuf::from(&custom_dir).join("plugins");
                if let Err(e) = app
                    .asset_protocol_scope()
                    .allow_directory(&plugins_path, true)
                {
                    eprintln!(
                        "Warning: Failed to add TREELINE_DIR plugins to asset scope: {}",
                        e
                    );
                } else {
                    println!("Added {} to asset protocol scope", plugins_path.display());
                }
            }

            // In debug builds, always open devtools
            #[cfg(debug_assertions)]
            {
                window.open_devtools();
                devtools_state.open.store(true, Ordering::SeqCst);
            }

            // In release builds, check if developerMode is enabled in settings
            #[cfg(not(debug_assertions))]
            {
                if let Ok(treeline_dir) = get_treeline_dir() {
                    let settings_path = treeline_dir.join("settings.json");
                    if settings_path.exists() {
                        if let Ok(content) = fs::read_to_string(&settings_path) {
                            if let Ok(settings) =
                                serde_json::from_str::<serde_json::Value>(&content)
                            {
                                if settings["app"]["developerMode"].as_bool() == Some(true) {
                                    window.open_devtools();
                                    devtools_state.open.store(true, Ordering::SeqCst);
                                }
                            }
                        }
                    }
                }
            }

            Ok(())
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_updater::Builder::new()
                .default_version_comparator(|current, remote| {
                    calver_comparator(&current.to_string(), &remote.version.to_string())
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            discover_plugins,
            get_plugins_dir,
            get_treeline_dir_display,
            execute_query,
            execute_query_with_params,
            read_plugin_config,
            write_plugin_config,
            read_settings,
            write_settings,
            read_plugin_state,
            write_plugin_state,
            run_sync,
            get_demo_mode,
            set_demo_mode,
            enable_demo,
            disable_demo,
            install_plugin,
            uninstall_plugin,
            upgrade_plugin,
            check_plugin_update,
            fetch_plugin_manifest,
            import_csv_preview,
            import_csv_execute,
            pick_csv_file,
            get_csv_headers,
            list_pending_imports,
            move_imported_file,
            setup_simplefin,
            setup_lunchflow,
            backfill_preview,
            backfill_execute,
            // Backup & Compact commands
            list_backups,
            create_backup,
            restore_backup,
            delete_backup,
            clear_backups,
            compact_database,
            // Encryption commands
            get_encryption_status,
            try_auto_unlock,
            unlock_database,
            enable_encryption,
            disable_encryption,
            // Theme commands
            list_themes,
            // Developer tools
            set_devtools,
            // Plugin hot-reload
            watch_plugins_dir,
            unwatch_plugins_dir,
            // Migrations
            run_migrations,
            // Account management
            delete_account,
            // App updates (with staging support)
            check_for_app_update,
            download_and_install_app_update,
            // Logging commands
            log_page,
            log_action,
            log_error,
            get_logs_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
