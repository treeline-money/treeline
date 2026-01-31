//! Core domain entities
//!
//! All business entities are defined here. These are pure data structures
//! with validation logic - no I/O or external dependencies.

mod account;
mod backup;
pub mod balance;
mod encryption;
pub mod result;
mod rule;
mod transaction;
mod user;

pub use account::Account;
pub use backup::BackupMetadata;
pub use balance::BalanceSnapshot;
pub use encryption::{Argon2Params, EncryptionMetadata, EncryptionStatus};
pub use rule::AutoTagRule;
pub use transaction::Transaction;
pub use user::User;
