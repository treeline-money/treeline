//! Migration service - manages database schema migrations
//!
//! Migrations are SQL files embedded at compile time. Each migration is
//! tracked in the sys_migrations table to ensure idempotent execution.

use anyhow::Result;
use duckdb::Connection;

use crate::migrations::MIGRATIONS;

/// Result of running migrations
#[derive(Debug)]
pub struct MigrationResult {
    /// Names of newly applied migrations
    pub applied: Vec<String>,
    /// Count of migrations that were already applied
    pub already_applied: usize,
}

/// Service for managing database migrations
pub struct MigrationService<'a> {
    conn: &'a Connection,
}

impl<'a> MigrationService<'a> {
    /// Create a new migration service with a database connection
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Run all pending migrations
    ///
    /// This is the main entry point. It:
    /// 1. Ensures the sys_migrations table exists (bootstrap)
    /// 2. Gets list of already applied migrations
    /// 3. Applies any pending migrations in order
    /// 4. Records each applied migration
    pub fn run_pending(&self) -> Result<MigrationResult> {
        let mut newly_applied = Vec::new();

        // Bootstrap: run the first migration (000_migrations.sql) if sys_migrations doesn't exist
        let bootstrap_ran = if !self.migrations_table_exists()? {
            if let Some((name, sql)) = MIGRATIONS.iter().find(|(n, _)| *n == "000_migrations.sql") {
                self.conn.execute_batch(sql)?;
                self.record_migration(name)?;
                newly_applied.push(name.to_string());
                true
            } else {
                false
            }
        } else {
            false
        };

        // Get already applied migrations (will include bootstrap if we just ran it)
        let applied_set = self.get_applied()?;
        // Subtract bootstrap from already_applied count if we just ran it
        let already_applied = if bootstrap_ran {
            applied_set.len().saturating_sub(1)
        } else {
            applied_set.len()
        };

        // Apply pending migrations in order (skip bootstrap, we already handled it)
        for (name, sql) in MIGRATIONS.iter() {
            if *name == "000_migrations.sql" {
                continue; // Already handled above
            }
            if !applied_set.contains(&name.to_string()) {
                // Execute migration
                self.conn.execute_batch(sql)?;
                self.record_migration(name)?;
                newly_applied.push(name.to_string());
            }
        }

        // Force checkpoint after migrations to flush DDL to main database file.
        // This prevents WAL replay issues with ALTER TABLE that have function defaults
        // (DuckDB bug: WAL replay can't resolve functions like gen_random_uuid()).
        if !newly_applied.is_empty() {
            let _ = self.conn.execute("CHECKPOINT", []);
        }

        Ok(MigrationResult {
            applied: newly_applied,
            already_applied,
        })
    }

    /// Check if sys_migrations table exists
    fn migrations_table_exists(&self) -> Result<bool> {
        let result: Result<i64, _> = self.conn.query_row(
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'sys_migrations'",
            [],
            |row| row.get(0),
        );

        match result {
            Ok(count) => Ok(count > 0),
            Err(_) => Ok(false),
        }
    }

    /// Get list of already applied migration names
    pub fn get_applied(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT migration_name FROM sys_migrations ORDER BY migration_name")?;
        let names = stmt.query_map([], |row| row.get::<_, String>(0))?;

        let mut result = Vec::new();
        for name in names {
            result.push(name?);
        }
        Ok(result)
    }

    /// Get list of pending migration names
    pub fn get_pending(&self) -> Result<Vec<String>> {
        let applied = self.get_applied()?;
        let pending: Vec<String> = MIGRATIONS
            .iter()
            .filter(|(name, _)| !applied.contains(&name.to_string()))
            .map(|(name, _)| name.to_string())
            .collect();
        Ok(pending)
    }

    /// Record a migration as applied
    fn record_migration(&self, name: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sys_migrations (migration_name) VALUES (?)",
            [name],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::Connection;

    #[test]
    fn test_migrations_run_on_fresh_db() {
        let conn = Connection::open_in_memory().unwrap();
        let service = MigrationService::new(&conn);

        let result = service.run_pending().unwrap();

        // All migrations should be applied
        assert_eq!(result.applied.len(), MIGRATIONS.len());
        assert_eq!(result.already_applied, 0);

        // Running again should apply nothing
        let result2 = service.run_pending().unwrap();
        assert_eq!(result2.applied.len(), 0);
        assert_eq!(result2.already_applied, MIGRATIONS.len());
    }

    #[test]
    fn test_get_pending_on_fresh_db() {
        let conn = Connection::open_in_memory().unwrap();

        // Bootstrap the migrations table first and record it
        conn.execute_batch(MIGRATIONS[0].1).unwrap();
        conn.execute(
            "INSERT INTO sys_migrations (migration_name) VALUES (?)",
            [MIGRATIONS[0].0],
        )
        .unwrap();

        let service = MigrationService::new(&conn);
        let pending = service.get_pending().unwrap();

        // All migrations except 000 should be pending
        assert_eq!(pending.len(), MIGRATIONS.len() - 1);
    }
}
