//! Logging service - structured event logging to DuckDB
//!
//! Provides a privacy-safe logging system that stores events in logs.duckdb.
//! No user data (transactions, accounts, balances, descriptions) is ever logged.
//!
//! This service is designed to be used by both CLI and desktop applications.
//! Uses per-operation locking to allow external tools to query logs while the app runs.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use duckdb::Connection;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::log_migrations::LOG_MIGRATIONS;

/// Counter for generating unique IDs within the same millisecond
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique ID based on timestamp + counter
fn generate_id() -> u64 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Use lower 48 bits for timestamp (good for ~8900 years)
    // Use upper 16 bits for counter (65536 unique IDs per millisecond)
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed) & 0xFFFF;
    (timestamp << 16) | counter
}

/// Get current unix timestamp in milliseconds
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Detect the current platform
fn detect_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

/// Entry point for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryPoint {
    Cli,
    Desktop,
}

impl EntryPoint {
    fn as_str(&self) -> &'static str {
        match self {
            EntryPoint::Cli => "cli",
            EntryPoint::Desktop => "desktop",
        }
    }
}

/// A log event to be recorded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_details: Option<String>,
}

impl LogEvent {
    /// Create a new log event with just an event name
    pub fn new(event: impl Into<String>) -> Self {
        Self {
            event: event.into(),
            integration: None,
            page: None,
            command: None,
            error_message: None,
            error_details: None,
        }
    }

    /// Set the integration context
    pub fn with_integration(mut self, integration: impl Into<String>) -> Self {
        self.integration = Some(integration.into());
        self
    }

    /// Set the page context (for frontend events)
    pub fn with_page(mut self, page: impl Into<String>) -> Self {
        self.page = Some(page.into());
        self
    }

    /// Set the command context (for CLI events)
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Set error information
    pub fn with_error(mut self, message: impl Into<String>) -> Self {
        self.error_message = Some(message.into());
        self
    }

    /// Set error details (stack trace, additional context)
    pub fn with_error_details(mut self, details: impl Into<String>) -> Self {
        self.error_details = Some(details.into());
        self
    }
}

/// A log entry as stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: i64,
    pub entry_point: String,
    pub app_version: String,
    pub platform: String,
    pub event: String,
    pub integration: Option<String>,
    pub page: Option<String>,
    pub command: Option<String>,
    pub error_message: Option<String>,
    pub error_details: Option<String>,
}

/// Service for structured event logging
///
/// This service manages the logs.duckdb database and provides methods
/// for logging events and querying the log history.
///
/// Uses per-operation filesystem locking to allow external tools (notebooks,
/// scripts) to query logs while the app is running.
pub struct LoggingService {
    db_path: PathBuf,
    entry_point: EntryPoint,
    app_version: String,
    platform: &'static str,
}

impl LoggingService {
    /// Create a new logging service
    ///
    /// Opens or creates logs.duckdb in the treeline directory and runs
    /// any pending migrations.
    pub fn new(
        treeline_dir: &Path,
        entry_point: EntryPoint,
        app_version: impl Into<String>,
    ) -> Result<Self> {
        let db_path = treeline_dir.join("logs.duckdb");

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let service = Self {
            db_path,
            entry_point,
            app_version: app_version.into(),
            platform: detect_platform(),
        };

        // Run migrations on startup
        service.run_migrations()?;

        Ok(service)
    }

    /// Acquire the filesystem lock for database access.
    fn acquire_lock(&self) -> Result<File> {
        let lock_path = self.db_path.with_extension("duckdb.lock");

        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| anyhow!("Failed to open lock file {}: {}", lock_path.display(), e))?;

        lock_file
            .lock_exclusive()
            .map_err(|e| anyhow!("Failed to acquire logs database lock: {}", e))?;

        Ok(lock_file)
    }

    /// Open a database connection
    fn open_connection(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)?;
        Ok(conn)
    }

    /// Execute a read-only operation with the database connection.
    fn with_connection<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let _lock = self.acquire_lock()?;
        let conn = self.open_connection()?;
        f(&conn)
    }

    /// Execute a write operation with the database connection.
    fn with_connection_write<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let _lock = self.acquire_lock()?;
        let conn = self.open_connection()?;
        let result = f(&conn)?;
        let _ = conn.execute("CHECKPOINT", []);
        Ok(result)
    }

    /// Run any pending migrations
    fn run_migrations(&self) -> Result<()> {
        self.with_connection_write(|conn| {
            // Check if migrations table exists
            let table_exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_name = 'sys_migrations'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(false);

            // Bootstrap migrations table if needed
            if !table_exists {
                if let Some((name, sql)) = LOG_MIGRATIONS
                    .iter()
                    .find(|(n, _)| *n == "000_migrations.sql")
                {
                    conn.execute_batch(sql)?;
                    conn.execute(
                        "INSERT INTO sys_migrations (migration_name) VALUES (?)",
                        [name],
                    )?;
                }
            }

            // Get applied migrations
            let mut stmt = conn.prepare("SELECT migration_name FROM sys_migrations")?;
            let applied: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            // Apply pending migrations
            for (name, sql) in LOG_MIGRATIONS.iter() {
                if *name == "000_migrations.sql" {
                    continue;
                }
                if !applied.contains(&name.to_string()) {
                    conn.execute_batch(sql)?;
                    conn.execute(
                        "INSERT INTO sys_migrations (migration_name) VALUES (?)",
                        [name],
                    )?;
                }
            }

            Ok(())
        })
    }

    /// Log an event
    ///
    /// This is the main method for recording events. The entry_point,
    /// app_version, and platform are automatically added from the service
    /// configuration.
    pub fn log(&self, event: LogEvent) -> Result<()> {
        self.with_connection_write(|conn| {
            conn.execute(
                r#"
                INSERT INTO sys_logs (
                    id, timestamp, entry_point, app_version, platform,
                    event, integration, page, command, error_message, error_details
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                duckdb::params![
                    generate_id(),
                    now_ms(),
                    self.entry_point.as_str(),
                    &self.app_version,
                    self.platform,
                    &event.event,
                    &event.integration,
                    &event.page,
                    &event.command,
                    &event.error_message,
                    &event.error_details,
                ],
            )?;
            Ok(())
        })
    }

    /// Log a simple event with just a name
    pub fn log_event(&self, event: &str) -> Result<()> {
        self.log(LogEvent::new(event))
    }

    /// Log a CLI command execution
    pub fn log_command(&self, command: &str) -> Result<()> {
        self.log(LogEvent::new("command_executed").with_command(command))
    }

    /// Log a frontend page navigation
    pub fn log_page(&self, page: &str) -> Result<()> {
        self.log(LogEvent::new("page_opened").with_page(page))
    }

    /// Log an error
    pub fn log_error(&self, event: &str, message: &str, details: Option<&str>) -> Result<()> {
        let mut log_event = LogEvent::new(event).with_error(message);
        if let Some(d) = details {
            log_event = log_event.with_error_details(d);
        }
        self.log(log_event)
    }

    /// Query recent log entries
    ///
    /// Returns the most recent entries, up to the specified limit.
    pub fn get_recent(&self, limit: usize) -> Result<Vec<LogEntry>> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, timestamp, entry_point, app_version, platform,
                       event, integration, page, command, error_message, error_details
                FROM sys_logs
                ORDER BY timestamp DESC
                LIMIT ?
                "#,
            )?;

            let entries = stmt
                .query_map([limit as i64], |row| {
                    Ok(LogEntry {
                        id: row.get(0)?,
                        timestamp: row.get(1)?,
                        entry_point: row.get(2)?,
                        app_version: row.get(3)?,
                        platform: row.get(4)?,
                        event: row.get(5)?,
                        integration: row.get(6)?,
                        page: row.get(7)?,
                        command: row.get(8)?,
                        error_message: row.get(9)?,
                        error_details: row.get(10)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(entries)
        })
    }

    /// Query log entries with errors
    pub fn get_errors(&self, limit: usize) -> Result<Vec<LogEntry>> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, timestamp, entry_point, app_version, platform,
                       event, integration, page, command, error_message, error_details
                FROM sys_logs
                WHERE error_message IS NOT NULL
                ORDER BY timestamp DESC
                LIMIT ?
                "#,
            )?;

            let entries = stmt
                .query_map([limit as i64], |row| {
                    Ok(LogEntry {
                        id: row.get(0)?,
                        timestamp: row.get(1)?,
                        entry_point: row.get(2)?,
                        app_version: row.get(3)?,
                        platform: row.get(4)?,
                        event: row.get(5)?,
                        integration: row.get(6)?,
                        page: row.get(7)?,
                        command: row.get(8)?,
                        error_message: row.get(9)?,
                        error_details: row.get(10)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(entries)
        })
    }

    /// Get the total number of log entries
    pub fn count(&self) -> Result<u64> {
        self.with_connection(|conn| {
            let count: u64 =
                conn.query_row("SELECT COUNT(*) FROM sys_logs", [], |row| row.get(0))?;
            Ok(count)
        })
    }

    /// Delete logs older than the specified timestamp (unix ms)
    pub fn delete_before(&self, timestamp_ms: i64) -> Result<u64> {
        self.with_connection_write(|conn| {
            let deleted =
                conn.execute("DELETE FROM sys_logs WHERE timestamp < ?", [timestamp_ms])?;
            Ok(deleted as u64)
        })
    }

    /// Export logs to a file for troubleshooting
    ///
    /// Creates a copy of the logs database that can be sent for analysis.
    pub fn export(&self, output_path: &Path) -> Result<PathBuf> {
        self.with_connection(|conn| {
            // Force checkpoint to ensure all data is written
            conn.execute("CHECKPOINT", [])?;

            // Copy the database file
            std::fs::copy(&self.db_path, output_path)?;

            Ok(output_path.to_path_buf())
        })
    }

    /// Get the path to the logs database
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_logging_service_creation() {
        let dir = tempdir().unwrap();
        let service = LoggingService::new(dir.path(), EntryPoint::Cli, "1.0.0").unwrap();

        assert!(service.db_path().exists());
    }

    #[test]
    fn test_log_event() {
        let dir = tempdir().unwrap();
        let service = LoggingService::new(dir.path(), EntryPoint::Cli, "1.0.0").unwrap();

        service.log_event("test_event").unwrap();

        let entries = service.get_recent(10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event, "test_event");
        assert_eq!(entries[0].entry_point, "cli");
        assert_eq!(entries[0].app_version, "1.0.0");
    }

    #[test]
    fn test_log_with_context() {
        let dir = tempdir().unwrap();
        let service = LoggingService::new(dir.path(), EntryPoint::Desktop, "2.0.0").unwrap();

        service
            .log(
                LogEvent::new("sync_completed")
                    .with_integration("simplefin")
                    .with_command("sync"),
            )
            .unwrap();

        let entries = service.get_recent(10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event, "sync_completed");
        assert_eq!(entries[0].integration, Some("simplefin".to_string()));
        assert_eq!(entries[0].command, Some("sync".to_string()));
        assert_eq!(entries[0].entry_point, "desktop");
    }

    #[test]
    fn test_log_error() {
        let dir = tempdir().unwrap();
        let service = LoggingService::new(dir.path(), EntryPoint::Cli, "1.0.0").unwrap();

        service
            .log_error("sync_failed", "Connection timeout", Some("at line 42"))
            .unwrap();

        let errors = service.get_errors(10).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].event, "sync_failed");
        assert_eq!(
            errors[0].error_message,
            Some("Connection timeout".to_string())
        );
        assert_eq!(errors[0].error_details, Some("at line 42".to_string()));
    }

    #[test]
    fn test_count_and_delete() {
        let dir = tempdir().unwrap();
        let service = LoggingService::new(dir.path(), EntryPoint::Cli, "1.0.0").unwrap();

        service.log_event("event1").unwrap();
        service.log_event("event2").unwrap();
        service.log_event("event3").unwrap();

        assert_eq!(service.count().unwrap(), 3);

        // Delete all logs (using future timestamp)
        let deleted = service.delete_before(now_ms() + 1000).unwrap();
        assert_eq!(deleted, 3);
        assert_eq!(service.count().unwrap(), 0);
    }

    #[test]
    fn test_export() {
        let dir = tempdir().unwrap();
        let service = LoggingService::new(dir.path(), EntryPoint::Cli, "1.0.0").unwrap();

        service.log_event("test_event").unwrap();

        let export_path = dir.path().join("export.duckdb");
        service.export(&export_path).unwrap();

        assert!(export_path.exists());
    }

    #[test]
    fn test_concurrent_log_access() {
        use std::sync::Arc;
        use std::thread;

        let dir = tempdir().unwrap();
        let db_path = Arc::new(dir.path().to_path_buf());

        // Create initial service and log some events
        {
            let service = LoggingService::new(&db_path, EntryPoint::Desktop, "1.0.0").unwrap();
            service.log_event("initial_event").unwrap();
        }

        // Spawn multiple threads that read and write concurrently
        let mut handles = vec![];

        for i in 0..4 {
            let db_path = Arc::clone(&db_path);
            let handle = thread::spawn(move || {
                let service = LoggingService::new(&db_path, EntryPoint::Cli, "1.0.0").unwrap();

                // Write
                service.log_event(&format!("thread_{}_event", i)).unwrap();

                // Read
                let count = service.count().unwrap();
                assert!(count >= 1, "Should have at least 1 log entry");
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all events were logged
        let service = LoggingService::new(&db_path, EntryPoint::Cli, "1.0.0").unwrap();
        let count = service.count().unwrap();
        assert_eq!(
            count, 5,
            "Should have 5 log entries (1 initial + 4 threads)"
        );
    }
}
