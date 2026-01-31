//! Repository port - database abstraction

use async_trait::async_trait;
use chrono::NaiveDate;
use uuid::Uuid;

use crate::domain::result::Result;
use crate::domain::{Account, BalanceSnapshot, Transaction};

/// COMMENT: why is this the only "Port"?
/// I would have expected every abstraction to be a "Port"

/// Database repository abstraction
///
/// This trait defines all database operations. Implementations (adapters)
/// provide the actual database access logic.
#[async_trait]
pub trait Repository: Send + Sync {
    // === Schema ===

    /// Ensure the database file exists
    async fn ensure_db_exists(&self) -> Result<()>;

    /// Run any pending migrations
    async fn ensure_schema_upgraded(&self) -> Result<()>;

    // === Accounts ===

    /// Add a new account
    async fn add_account(&self, account: &Account) -> Result<()>;

    /// Upsert multiple accounts (insert or update)
    async fn bulk_upsert_accounts(&self, accounts: &[Account]) -> Result<()>;

    /// Get all accounts
    async fn get_accounts(&self) -> Result<Vec<Account>>;

    /// Get account by ID
    async fn get_account_by_id(&self, id: Uuid) -> Result<Option<Account>>;

    /// Get account by external ID (e.g., SimpleFIN ID)
    async fn get_account_by_external_id(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<Account>>;

    /// Update an existing account
    async fn update_account(&self, account: &Account) -> Result<()>;

    /// Delete an account and all associated data (transactions, balance snapshots)
    async fn delete_account(&self, id: Uuid) -> Result<()>;

    // === Transactions ===

    /// Add a new transaction
    async fn add_transaction(&self, tx: &Transaction) -> Result<()>;

    /// Upsert multiple transactions
    async fn bulk_upsert_transactions(&self, txs: &[Transaction]) -> Result<()>;

    /// Get transactions for an account
    async fn get_transactions_by_account(&self, account_id: Uuid) -> Result<Vec<Transaction>>;

    /// Get transactions by date range
    async fn get_transactions_by_date_range(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Vec<Transaction>>;

    /// Get transactions by fingerprints (for deduplication)
    async fn get_transactions_by_fingerprints(
        &self,
        fingerprints: &[String],
    ) -> Result<Vec<Transaction>>;

    /// Update transaction tags
    async fn update_transaction_tags(&self, id: Uuid, tags: &[String]) -> Result<()>;

    /// Soft delete a transaction
    async fn soft_delete_transaction(&self, id: Uuid) -> Result<()>;

    // === Balances ===

    /// Add a balance snapshot
    async fn add_balance(&self, balance: &BalanceSnapshot) -> Result<()>;

    /// Get balance snapshots for an account
    async fn get_balance_snapshots(&self, account_id: Uuid) -> Result<Vec<BalanceSnapshot>>;

    /// Get latest balance for an account
    async fn get_latest_balance(&self, account_id: Uuid) -> Result<Option<BalanceSnapshot>>;

    // === Queries ===

    /// Execute a read-only SQL query, returns JSON
    async fn execute_query(&self, sql: &str) -> Result<QueryResult>;

    // === Maintenance ===

    /// Compact the database (vacuum, checkpoint)
    async fn compact(&self) -> Result<CompactResult>;
}

/// Result of a SQL query
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
}

/// Result of a compact operation
#[derive(Debug, Clone)]
pub struct CompactResult {
    pub size_before: u64,
    pub size_after: u64,
}
