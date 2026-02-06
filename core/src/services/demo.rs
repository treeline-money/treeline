//! Demo service - manage demo mode
//!
//! Demo mode provides sample data for testing and onboarding without
//! connecting to real financial accounts.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

use crate::adapters::demo::{
    generate_demo_accounts, generate_demo_balance_snapshots, generate_demo_transactions,
};
use crate::adapters::duckdb::DuckDbRepository;
use crate::config::Config;

/// Demo service for managing demo mode
pub struct DemoService {
    treeline_dir: PathBuf,
}

impl DemoService {
    pub fn new(treeline_dir: &Path) -> Self {
        Self {
            treeline_dir: treeline_dir.to_path_buf(),
        }
    }

    /// Check if demo mode is currently enabled
    pub fn is_enabled(&self) -> Result<bool> {
        let config = Config::load(&self.treeline_dir)?;
        Ok(config.demo_mode)
    }

    /// Enable demo mode
    ///
    /// This will:
    /// 1. Delete any existing demo database (fresh start)
    /// 2. Enable demo mode in config
    /// 3. Create demo database with sample data
    pub fn enable(&self) -> Result<()> {
        // Delete existing demo database for a fresh start
        let demo_db = self.treeline_dir.join("demo.duckdb");
        let demo_wal = self.treeline_dir.join("demo.duckdb.wal");
        if demo_db.exists() {
            std::fs::remove_file(&demo_db)?;
        }
        if demo_wal.exists() {
            std::fs::remove_file(&demo_wal)?;
        }

        // Enable demo mode in config
        let mut config = Config::load(&self.treeline_dir).unwrap_or_default();
        config.enable_demo_mode();
        config.save(&self.treeline_dir)?;

        // Create demo database and populate with data
        let repository = Arc::new(DuckDbRepository::new(&demo_db, None)?);
        repository.ensure_schema()?;

        // Add demo integration
        repository.upsert_integration("demo", &serde_json::json!({}))?;

        // Add demo data using bulk operations (single connection each)
        repository.bulk_upsert_accounts(&generate_demo_accounts())?;
        repository.bulk_insert_transactions(&generate_demo_transactions())?;
        repository.bulk_insert_balance_snapshots(&generate_demo_balance_snapshots())?;

        Ok(())
    }

    /// Disable demo mode
    ///
    /// This will:
    /// 1. Disable demo mode in config
    /// 2. Optionally delete demo database (if clean = true)
    pub fn disable(&self, clean: bool) -> Result<()> {
        // Disable demo mode in config
        let mut config = Config::load(&self.treeline_dir).unwrap_or_default();
        config.disable_demo_mode();
        config.save(&self.treeline_dir)?;

        // Optionally clean up demo database
        if clean {
            let demo_db = self.treeline_dir.join("demo.duckdb");
            let demo_wal = self.treeline_dir.join("demo.duckdb.wal");
            if demo_db.exists() {
                std::fs::remove_file(&demo_db)?;
            }
            if demo_wal.exists() {
                std::fs::remove_file(&demo_wal)?;
            }
        }

        Ok(())
    }
}
