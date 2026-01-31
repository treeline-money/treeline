//! Data aggregation provider port
//!
//! Defines the interface for fetching account and transaction data from
//! external sources (SimpleFIN, demo data, CSV files, etc.)

use chrono::NaiveDate;
use serde_json::Value as JsonValue;

use crate::domain::result::Result;
use crate::domain::{Account, BalanceSnapshot, Transaction};

/// Result of fetching accounts from a provider
#[derive(Debug, Default)]
pub struct FetchAccountsResult {
    pub accounts: Vec<Account>,
    pub balance_snapshots: Vec<BalanceSnapshot>,
    pub warnings: Vec<String>,
}

/// Result of fetching transactions from a provider
#[derive(Debug, Default)]
pub struct FetchTransactionsResult {
    /// Transactions keyed by provider account ID
    pub transactions: Vec<(String, Transaction)>,
    pub warnings: Vec<String>,
}

/// Data aggregation provider trait
///
/// Implementations fetch account and transaction data from external sources.
/// The SyncService uses this trait to sync data without knowing the specifics
/// of each provider (SimpleFIN, demo, etc.)
pub trait DataAggregationProvider: Send + Sync {
    /// Provider name (e.g., "simplefin", "demo")
    fn name(&self) -> &str;

    /// Whether this provider can fetch accounts
    fn can_get_accounts(&self) -> bool;

    /// Whether this provider can fetch transactions
    fn can_get_transactions(&self) -> bool;

    /// Whether this provider can fetch balance snapshots
    fn can_get_balances(&self) -> bool;

    /// Fetch accounts from the provider
    ///
    /// # Arguments
    /// * `settings` - Provider-specific settings (e.g., access tokens)
    fn get_accounts(&self, settings: &JsonValue) -> Result<FetchAccountsResult>;

    /// Fetch transactions from the provider
    ///
    /// # Arguments
    /// * `start_date` - Start of date range
    /// * `end_date` - End of date range
    /// * `account_ids` - Provider-specific account IDs to fetch (empty = all)
    /// * `settings` - Provider-specific settings
    fn get_transactions(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        account_ids: &[String],
        settings: &JsonValue,
    ) -> Result<FetchTransactionsResult>;
}

/// Integration provider trait
///
/// Implementations handle setting up integrations (e.g., claiming SimpleFIN tokens,
/// enabling demo mode).
pub trait IntegrationProvider: Send + Sync {
    /// Set up a new integration
    ///
    /// # Arguments
    /// * `options` - Provider-specific setup options (e.g., setup token for SimpleFIN)
    ///
    /// # Returns
    /// Settings to store for this integration
    fn setup(&self, options: &JsonValue) -> Result<JsonValue>;
}
