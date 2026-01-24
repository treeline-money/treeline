//! Service layer - business logic orchestration
//!
//! Services coordinate domain logic and port interactions. Each service
//! focuses on a specific use case or feature area.

mod backup;
mod balance;
mod compact;
mod demo;
mod doctor;
pub mod encryption;
pub mod import;
pub mod logging;
pub mod migration;
pub mod plugin;
mod query;
mod status;
mod sync;
mod tag;

pub use backup::BackupService;
pub use balance::{BackfillExecuteResult, BalanceService, BalanceSnapshotPreview};
pub use compact::CompactService;
pub use demo::DemoService;
pub use doctor::DoctorService;
pub use encryption::EncryptionService;
pub use import::{ImportOptions, ImportResult, ImportService, NumberFormat};
pub use logging::{EntryPoint, LogEntry, LogEvent, LoggingService};
pub use migration::{MigrationResult, MigrationService};
pub use plugin::{PluginInfo, PluginManifest, PluginResult, PluginService, UpdateInfo};
pub use query::QueryService;
pub use status::{AccountSummary, DateRange, StatusService, StatusSummary};
pub use sync::SyncService;
pub use tag::{AutoTagResult, TagResult, TagResultEntry, TagService};
