//! Treeline Core - Business logic for personal finance management
//!
//! This crate implements the core domain logic following hexagonal architecture:
//!
//! - **domain**: Core business entities (Account, Transaction, etc.)
//! - **ports**: Trait definitions for external dependencies (Repository, DataProvider)
//! - **services**: Business logic orchestration
//! - **adapters**: Concrete implementations (DuckDB, SimpleFIN, etc.)

pub mod adapters;
pub mod config;
pub mod domain;
pub mod migrations;
pub mod ports;
pub mod services;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use adapters::duckdb::DuckDbRepository;
use config::Config;
use services::*;

// Re-export commonly used types at crate root
pub use adapters::duckdb::QueryResult;
pub use domain::result::{Error, OperationResult};
pub use domain::{
    Account, BackupMetadata, BalanceSnapshot, EncryptionMetadata, EncryptionStatus, Transaction,
    User,
};

/// Main context for Treeline operations
///
/// This is the primary entry point for all business logic. It holds
/// the database connection, configuration, and all services.
pub struct TreelineContext {
    pub config: Config,
    pub repository: Arc<DuckDbRepository>,
    pub status_service: StatusService,
    pub sync_service: SyncService,
    pub query_service: QueryService,
    pub tag_service: TagService,
    pub backup_service: BackupService,
    pub compact_service: CompactService,
    pub doctor_service: DoctorService,
    pub encryption_service: EncryptionService,
    pub import_service: ImportService,
    pub balance_service: BalanceService,
    pub plugin_service: services::PluginService,
}

impl TreelineContext {
    /// Create a new Treeline context
    pub fn new(treeline_dir: &Path, password: Option<&str>) -> Result<Self> {
        let config = Config::load(treeline_dir)?;

        // Determine which database file to use
        let db_filename = if config.demo_mode {
            "demo.duckdb"
        } else {
            "treeline.duckdb"
        };

        let db_path = treeline_dir.join(db_filename);
        let repository = Arc::new(DuckDbRepository::new(&db_path, password)?);

        // Initialize schema
        repository.ensure_schema()?;

        // Create services
        let status_service = StatusService::new(Arc::clone(&repository));
        let sync_service = SyncService::new(Arc::clone(&repository), treeline_dir.to_path_buf());
        let query_service = QueryService::new(Arc::clone(&repository));
        let tag_service = TagService::new(Arc::clone(&repository));
        let backup_service = BackupService::new_with_repository(
            treeline_dir.to_path_buf(),
            db_filename.to_string(),
            Arc::clone(&repository),
        );
        let compact_service = CompactService::new(Arc::clone(&repository));
        let doctor_service =
            DoctorService::new(Arc::clone(&repository), treeline_dir.to_path_buf());
        let encryption_service =
            EncryptionService::new(treeline_dir.to_path_buf(), db_path.clone());
        let import_service =
            ImportService::new(Arc::clone(&repository), treeline_dir.to_path_buf());
        let balance_service = BalanceService::new(Arc::clone(&repository));
        let plugin_service = services::PluginService::new(treeline_dir);

        Ok(Self {
            config,
            repository,
            status_service,
            sync_service,
            query_service,
            tag_service,
            backup_service,
            compact_service,
            doctor_service,
            encryption_service,
            import_service,
            balance_service,
            plugin_service,
        })
    }
}
