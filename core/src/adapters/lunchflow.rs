//! Lunchflow API client
//!
//! Handles communication with the Lunchflow API for account and transaction sync.
//! Lunchflow is a multi-provider bank aggregator supporting 20,000+ banks globally.
//!
//! API Documentation: https://docs.lunchflow.app/api-reference

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use reqwest::blocking::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::domain::result::{Error as DomainError, Result as DomainResult};
use crate::domain::{Account, BalanceSnapshot, Transaction};
use crate::ports::{
    DataAggregationProvider, FetchAccountsResult, FetchTransactionsResult, IntegrationProvider,
};

// =============================================================================
// API Response Models (matching Lunchflow API spec)
// =============================================================================

/// Wrapper for accounts list response
#[derive(Debug, Clone, Deserialize)]
struct AccountsResponse {
    accounts: Vec<LunchflowAccount>,
    #[allow(dead_code)]
    total: i64,
}

/// Lunchflow account from API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LunchflowAccount {
    /// Account ID (API returns number, we accept both)
    #[serde(deserialize_with = "deserialize_id")]
    pub id: String,
    pub name: String,
    pub institution_name: String,
    #[serde(default)]
    pub institution_logo: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Wrapper for balance response
#[derive(Debug, Clone, Deserialize)]
struct BalanceResponse {
    balance: BalanceData,
}

#[derive(Debug, Clone, Deserialize)]
struct BalanceData {
    amount: f64,
    currency: String,
}

/// Wrapper for transactions list response
#[derive(Debug, Clone, Deserialize)]
struct TransactionsResponse {
    transactions: Vec<LunchflowTransaction>,
    #[allow(dead_code)]
    total: i64,
}

/// Lunchflow transaction from API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LunchflowTransaction {
    pub id: String,
    /// Account ID this transaction belongs to
    #[serde(
        default,
        rename = "accountId",
        deserialize_with = "deserialize_optional_id"
    )]
    pub account_id: Option<String>,
    /// Amount as number from API
    #[serde(deserialize_with = "deserialize_amount")]
    pub amount: Decimal,
    pub currency: String,
    pub date: String, // ISO date string YYYY-MM-DD
    #[serde(default)]
    pub merchant: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// API uses isPending (camelCase)
    #[serde(default, rename = "isPending")]
    pub is_pending: bool,
}

/// Deserialize ID that can be number or string
fn deserialize_id<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value: JsonValue = Deserialize::deserialize(deserializer)?;
    match value {
        JsonValue::Number(n) => Ok(n.to_string()),
        JsonValue::String(s) => Ok(s),
        _ => Err(D::Error::custom("expected number or string for id")),
    }
}

/// Deserialize optional ID that can be number or string
fn deserialize_optional_id<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value: Option<JsonValue> = Option::deserialize(deserializer)?;
    match value {
        Some(JsonValue::Number(n)) => Ok(Some(n.to_string())),
        Some(JsonValue::String(s)) => Ok(Some(s)),
        Some(JsonValue::Null) | None => Ok(None),
        _ => Err(D::Error::custom("expected number or string for id")),
    }
}

/// Deserialize amount that can be number or string
fn deserialize_amount<'de, D>(deserializer: D) -> std::result::Result<Decimal, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value: JsonValue = Deserialize::deserialize(deserializer)?;
    match value {
        JsonValue::Number(n) => {
            let s = n.to_string();
            s.parse::<Decimal>()
                .map_err(|e| D::Error::custom(format!("invalid decimal: {}", e)))
        }
        JsonValue::String(s) => s
            .parse::<Decimal>()
            .map_err(|e| D::Error::custom(format!("invalid decimal: {}", e))),
        _ => Err(D::Error::custom("expected number or string for amount")),
    }
}

/// Result of syncing accounts from Lunchflow
#[derive(Debug)]
pub struct SyncedAccounts {
    pub accounts: Vec<Account>,
    pub balance_snapshots: Vec<BalanceSnapshot>,
    pub warnings: Vec<String>,
}

/// Result of syncing transactions from Lunchflow
#[derive(Debug)]
pub struct SyncedTransactions {
    /// Tuples of (lunchflow_account_id, transaction)
    pub transactions: Vec<(String, Transaction)>,
    pub warnings: Vec<String>,
}

// =============================================================================
// Lunchflow HTTP Client
// =============================================================================

/// Default production API URL
const LUNCHFLOW_PRODUCTION_URL: &str = "https://www.lunchflow.app/api/v1";

/// Environment variable to override the Lunchflow API base URL.
/// Set this to use a staging/sandbox environment for testing.
pub const LUNCHFLOW_BASE_URL_ENV: &str = "LUNCHFLOW_BASE_URL";

/// Get the Lunchflow base URL, checking environment variable first
pub fn get_base_url() -> String {
    std::env::var(LUNCHFLOW_BASE_URL_ENV).unwrap_or_else(|_| LUNCHFLOW_PRODUCTION_URL.to_string())
}

/// Lunchflow API client
#[derive(Debug)]
pub struct LunchflowClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl LunchflowClient {
    /// Create a new Lunchflow client with the given API key.
    ///
    /// Uses the `LUNCHFLOW_BASE_URL` environment variable if set,
    /// otherwise defaults to the production API.
    pub fn new(api_key: &str) -> Result<Self> {
        Self::new_with_base_url(api_key, &get_base_url())
    }

    /// Create a new Lunchflow client with a custom base URL.
    ///
    /// Prefer using `new()` with the `LUNCHFLOW_BASE_URL` env var for testing.
    pub fn new_with_base_url(api_key: &str, base_url: &str) -> Result<Self> {
        if api_key.is_empty() {
            anyhow::bail!("Lunchflow API key cannot be empty");
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            api_key: api_key.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    /// Fetch all accounts from Lunchflow
    pub fn get_accounts(&self) -> Result<SyncedAccounts> {
        let url = format!("{}/accounts", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .send()
            .map_err(|e| self.map_request_error(e))?;

        self.check_response_status(&response)?;

        // API returns { accounts: [...], total: N }
        let api_response: AccountsResponse = response
            .json()
            .context("Failed to parse Lunchflow accounts response")?;

        let mut domain_accounts = Vec::new();
        let mut balance_snapshots = Vec::new();
        let mut warnings = Vec::new();

        for lf_account in api_response.accounts {
            // Skip disconnected/error accounts
            if let Some(status) = &lf_account.status {
                if status != "ACTIVE" {
                    warnings.push(format!(
                        "Account '{}' has status '{}' - skipping",
                        lf_account.name, status
                    ));
                    continue;
                }
            }

            let account = self.map_account(&lf_account);

            // Fetch balance separately for each account
            match self.fetch_account_balance(&lf_account.id) {
                Ok((balance, currency)) => {
                    balance_snapshots.push(BalanceSnapshot {
                        id: Uuid::new_v4(),
                        account_id: account.id,
                        balance,
                        snapshot_time: Utc::now().naive_utc(),
                        source: Some("sync".to_string()),
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                    });
                    // Update account with fetched balance and currency
                    let mut account = account;
                    account.balance = Some(balance);
                    if !currency.is_empty() {
                        // Update both core currency and lf_currency from balance response
                        if account.currency == "USD" {
                            account.currency = currency.clone();
                        }
                        // Always set lf_currency if not already set
                        if account.lf_currency.is_none() {
                            account.lf_currency = Some(currency);
                        }
                    }
                    domain_accounts.push(account);
                }
                Err(e) => {
                    warnings.push(format!(
                        "Failed to fetch balance for account '{}': {}",
                        lf_account.name, e
                    ));
                    domain_accounts.push(account);
                }
            }
        }

        Ok(SyncedAccounts {
            accounts: domain_accounts,
            balance_snapshots,
            warnings,
        })
    }

    /// Fetch balance for a single account
    fn fetch_account_balance(&self, account_id: &str) -> Result<(Decimal, String)> {
        let url = format!("{}/accounts/{}/balance", self.base_url, account_id);

        let response = self
            .client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .send()
            .map_err(|e| self.map_request_error(e))?;

        self.check_response_status(&response)?;

        let balance_response: BalanceResponse = response
            .json()
            .context("Failed to parse balance response")?;

        let balance = Decimal::try_from(balance_response.balance.amount)
            .unwrap_or_else(|_| Decimal::new((balance_response.balance.amount * 100.0) as i64, 2));

        Ok((balance, balance_response.balance.currency))
    }

    /// Fetch transactions for specific accounts
    ///
    /// Note: The Lunchflow API does not support date filtering - it returns all transactions.
    /// The start_date and end_date parameters are kept for API compatibility but are ignored.
    pub fn get_transactions(
        &self,
        _start_date: NaiveDate,
        _end_date: NaiveDate,
        account_ids: Option<&[String]>,
    ) -> Result<SyncedTransactions> {
        let mut all_transactions = Vec::new();
        let mut warnings = Vec::new();

        // If no account IDs specified, we need to fetch accounts first
        let ids_to_fetch: Vec<String> = match account_ids {
            Some(ids) if !ids.is_empty() => ids.to_vec(),
            _ => {
                // Fetch all account IDs
                let accounts = self.get_accounts()?;
                accounts
                    .accounts
                    .iter()
                    .filter_map(|a| a.lf_id.clone())
                    .collect()
            }
        };

        for account_id in ids_to_fetch {
            match self.fetch_account_transactions(&account_id, true) {
                Ok(txs) => {
                    for lf_tx in txs {
                        let tx = self.map_transaction(&lf_tx);
                        all_transactions.push((account_id.clone(), tx));
                    }
                }
                Err(e) => {
                    warnings.push(format!(
                        "Failed to fetch transactions for account {}: {}",
                        account_id, e
                    ));
                }
            }
        }

        Ok(SyncedTransactions {
            transactions: all_transactions,
            warnings,
        })
    }

    /// Fetch transactions for a single account
    fn fetch_account_transactions(
        &self,
        account_id: &str,
        include_pending: bool,
    ) -> Result<Vec<LunchflowTransaction>> {
        let url = format!(
            "{}/accounts/{}/transactions?include_pending={}",
            self.base_url, account_id, include_pending
        );

        let response = self
            .client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .send()
            .map_err(|e| self.map_request_error(e))?;

        self.check_response_status(&response)?;

        // API returns { transactions: [...], total: N }
        let api_response: TransactionsResponse = response
            .json()
            .context("Failed to parse Lunchflow transactions response")?;

        Ok(api_response.transactions)
    }

    /// Map Lunchflow account to domain Account
    fn map_account(&self, lf_account: &LunchflowAccount) -> Account {
        // Compute classification based on account_type
        // Lunchflow doesn't provide account type, so default to asset
        // Users can override in the UI after sync
        let classification = Some(Account::compute_classification(None));

        let now = Utc::now();
        Account {
            id: Uuid::new_v4(),
            name: lf_account.name.clone(),
            nickname: None,
            currency: lf_account
                .currency
                .clone()
                .unwrap_or_else(|| "USD".to_string()),
            account_type: None, // Lunchflow doesn't provide account type
            classification,
            balance: None, // Will be set after fetching balance
            institution_name: Some(lf_account.institution_name.clone()),
            institution_url: None,
            institution_domain: None,
            created_at: now,
            updated_at: now,
            // Manual flag
            is_manual: false,
            // SimpleFIN fields (not applicable)
            sf_id: None,
            sf_name: None,
            sf_currency: None,
            sf_balance: None,
            sf_available_balance: None,
            sf_balance_date: None,
            sf_org_name: None,
            sf_org_url: None,
            sf_org_domain: None,
            sf_extra: None,
            // Lunchflow: Store ALL raw fields from API
            lf_id: Some(lf_account.id.clone()),
            lf_name: Some(lf_account.name.clone()),
            lf_institution_name: Some(lf_account.institution_name.clone()),
            lf_institution_logo: lf_account.institution_logo.clone(),
            lf_provider: lf_account.provider.clone(),
            lf_currency: lf_account.currency.clone(),
            lf_status: lf_account.status.clone(),
        }
    }

    /// Map Lunchflow transaction to domain Transaction
    fn map_transaction(&self, lf_tx: &LunchflowTransaction) -> Transaction {
        let posted_date = NaiveDate::parse_from_str(&lf_tx.date, "%Y-%m-%d")
            .unwrap_or_else(|_| Utc::now().naive_utc().date());

        // Map lf_description to core description (full transaction description)
        // Falls back to lf_merchant if description is missing or empty
        // Both raw fields are stored for power users
        let description = lf_tx
            .description
            .as_ref()
            .filter(|d| !d.trim().is_empty())
            .cloned()
            .or_else(|| {
                lf_tx
                    .merchant
                    .as_ref()
                    .filter(|m| !m.trim().is_empty())
                    .cloned()
            });

        let now = Utc::now();
        Transaction {
            id: Uuid::new_v4(),
            account_id: Uuid::nil(), // Will be set by sync service after mapping
            amount: lf_tx.amount,
            description, // Core field mapped from lf_description
            transaction_date: posted_date,
            posted_date,
            tags: vec![],
            created_at: now,
            updated_at: now,
            deleted_at: None,
            parent_transaction_id: None,
            // CSV Import tracking (not applicable)
            csv_fingerprint: None,
            csv_batch_id: None,
            // Manual flag
            is_manual: false,
            // Auto-tag tracking (starts false, set true when rules apply)
            tags_auto_applied: false,
            // SimpleFIN fields (not applicable)
            sf_id: None,
            sf_posted: None,
            sf_amount: None,
            sf_description: None,
            sf_transacted_at: None,
            sf_pending: None,
            sf_extra: None,
            // Lunchflow: Store ALL raw fields from API
            lf_id: Some(lf_tx.id.clone()),
            lf_account_id: lf_tx.account_id.clone(),
            lf_amount: Some(lf_tx.amount),
            lf_currency: Some(lf_tx.currency.clone()),
            lf_date: Some(posted_date),
            lf_merchant: lf_tx.merchant.clone(),
            lf_description: lf_tx.description.clone(),
            lf_is_pending: Some(lf_tx.is_pending),
        }
    }

    /// Map request errors to user-friendly messages
    fn map_request_error(&self, error: reqwest::Error) -> anyhow::Error {
        if error.is_timeout() {
            anyhow::anyhow!("Connection timed out after 120 seconds")
        } else if error.is_connect() {
            anyhow::anyhow!("Unable to connect to Lunchflow servers")
        } else {
            anyhow::anyhow!("Lunchflow request failed: {}", error)
        }
    }

    /// Check response status and return appropriate errors
    fn check_response_status(&self, response: &reqwest::blocking::Response) -> Result<()> {
        match response.status().as_u16() {
            200 => Ok(()),
            401 => anyhow::bail!(
                "Lunchflow authentication failed. Your API key may be invalid or revoked."
            ),
            402 => anyhow::bail!(
                "Lunchflow subscription required. Please check your account at https://lunchflow.app"
            ),
            429 => anyhow::bail!(
                "Lunchflow rate limit exceeded. Please wait a moment and try again."
            ),
            403 => anyhow::bail!(
                "Lunchflow access denied. Please check your API key permissions."
            ),
            404 => anyhow::bail!("Lunchflow resource not found."),
            status => anyhow::bail!("Lunchflow API error: HTTP {}", status),
        }
    }
}

// =============================================================================
// LunchflowProvider - implements DataAggregationProvider trait
// =============================================================================

/// Lunchflow data provider
///
/// Implements DataAggregationProvider and IntegrationProvider traits
/// for syncing financial data via Lunchflow.
pub struct LunchflowProvider;

impl LunchflowProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LunchflowProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DataAggregationProvider for LunchflowProvider {
    fn name(&self) -> &str {
        "lunchflow"
    }

    fn can_get_accounts(&self) -> bool {
        true
    }

    fn can_get_transactions(&self) -> bool {
        true
    }

    fn can_get_balances(&self) -> bool {
        true
    }

    fn get_accounts(&self, settings: &JsonValue) -> DomainResult<FetchAccountsResult> {
        let api_key = settings
            .get("apiKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                DomainError::Config("Lunchflow apiKey not found in settings".to_string())
            })?;

        // Check for custom base URL (for testing with mock server)
        let base_url = settings.get("baseUrl").and_then(|v| v.as_str());

        let client = if let Some(url) = base_url {
            LunchflowClient::new_with_base_url(api_key, url)
        } else {
            LunchflowClient::new(api_key)
        }
        .map_err(|e| DomainError::Sync(e.to_string()))?;

        let synced = client
            .get_accounts()
            .map_err(|e| DomainError::Sync(e.to_string()))?;

        Ok(FetchAccountsResult {
            accounts: synced.accounts,
            balance_snapshots: synced.balance_snapshots,
            warnings: synced.warnings,
        })
    }

    fn get_transactions(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        account_ids: &[String],
        settings: &JsonValue,
    ) -> DomainResult<FetchTransactionsResult> {
        let api_key = settings
            .get("apiKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                DomainError::Config("Lunchflow apiKey not found in settings".to_string())
            })?;

        // Check for custom base URL (for testing with mock server)
        let base_url = settings.get("baseUrl").and_then(|v| v.as_str());

        let client = if let Some(url) = base_url {
            LunchflowClient::new_with_base_url(api_key, url)
        } else {
            LunchflowClient::new(api_key)
        }
        .map_err(|e| DomainError::Sync(e.to_string()))?;

        let ids = if account_ids.is_empty() {
            None
        } else {
            Some(account_ids)
        };

        let synced = client
            .get_transactions(start_date, end_date, ids)
            .map_err(|e| DomainError::Sync(e.to_string()))?;

        Ok(FetchTransactionsResult {
            transactions: synced.transactions,
            warnings: synced.warnings,
        })
    }
}

impl IntegrationProvider for LunchflowProvider {
    fn setup(&self, options: &JsonValue) -> DomainResult<JsonValue> {
        let api_key = options
            .get("apiKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                DomainError::Config("Lunchflow apiKey required for setup".to_string())
            })?;

        // Check for custom base URL (for testing with mock server)
        let base_url = options.get("baseUrl").and_then(|v| v.as_str());

        let client = if let Some(url) = base_url {
            LunchflowClient::new_with_base_url(api_key, url)
        } else {
            LunchflowClient::new(api_key)
        }
        .map_err(|e| DomainError::Sync(e.to_string()))?;

        // Validate API key by fetching accounts
        let _ = client.get_accounts().map_err(|e| {
            DomainError::Sync(format!("Failed to validate Lunchflow API key: {}", e))
        })?;

        // Build settings to store
        let mut settings = serde_json::json!({
            "apiKey": api_key
        });

        // Include base URL if custom (for testing)
        if let Some(url) = base_url {
            settings["baseUrl"] = serde_json::json!(url);
        }

        Ok(settings)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_name() {
        let provider = LunchflowProvider::new();
        assert_eq!(provider.name(), "lunchflow");
    }

    #[test]
    fn test_provider_capabilities() {
        let provider = LunchflowProvider::new();
        assert!(provider.can_get_accounts());
        assert!(provider.can_get_transactions());
        assert!(provider.can_get_balances());
    }

    #[test]
    fn test_reject_empty_api_key() {
        let result = LunchflowClient::new("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_account_mapping() {
        let lf_account = LunchflowAccount {
            id: "123".to_string(),
            name: "Test Account".to_string(),
            institution_name: "Test Bank".to_string(),
            institution_logo: None,
            provider: Some("gocardless".to_string()),
            currency: Some("EUR".to_string()),
            status: Some("ACTIVE".to_string()),
        };

        let client = LunchflowClient::new_with_base_url("test_key", "http://localhost").unwrap();
        let account = client.map_account(&lf_account);

        assert_eq!(account.name, "Test Account");
        assert_eq!(account.currency, "EUR");
        assert_eq!(account.institution_name, Some("Test Bank".to_string()));
        assert_eq!(account.lf_id, Some("123".to_string()));
        assert_eq!(account.lf_currency, Some("EUR".to_string()));
    }

    #[test]
    fn test_transaction_mapping() {
        let lf_tx = LunchflowTransaction {
            id: "tx_456".to_string(),
            account_id: Some("123".to_string()),
            date: "2025-01-15".to_string(),
            amount: Decimal::new(-4250, 2),
            currency: "EUR".to_string(),
            merchant: Some("Coffee Shop".to_string()),
            description: Some("Card payment".to_string()),
            is_pending: false,
        };

        let client = LunchflowClient::new_with_base_url("test_key", "http://localhost").unwrap();
        let tx = client.map_transaction(&lf_tx);

        // Core description = lf_description (full description), merchant stored separately
        assert_eq!(tx.description, Some("Card payment".to_string()));
        assert_eq!(tx.lf_merchant, Some("Coffee Shop".to_string()));
        assert_eq!(tx.lf_description, Some("Card payment".to_string()));
        assert_eq!(tx.amount, Decimal::new(-4250, 2));
        assert_eq!(tx.lf_id, Some("tx_456".to_string()));
        assert_eq!(tx.lf_currency, Some("EUR".to_string()));
    }

    #[test]
    fn test_transaction_mapping_no_description() {
        let lf_tx = LunchflowTransaction {
            id: "tx_789".to_string(),
            account_id: None,
            date: "2025-01-15".to_string(),
            amount: Decimal::new(10000, 2),
            currency: "USD".to_string(),
            merchant: Some("Employer Inc".to_string()),
            description: None,
            is_pending: false,
        };

        let client = LunchflowClient::new_with_base_url("test_key", "http://localhost").unwrap();
        let tx = client.map_transaction(&lf_tx);

        // Falls back to lf_merchant when no description
        assert_eq!(tx.description, Some("Employer Inc".to_string()));
        assert_eq!(tx.lf_merchant, Some("Employer Inc".to_string()));
        assert_eq!(tx.lf_description, None);
    }

    #[test]
    fn test_transaction_mapping_empty_description() {
        let lf_tx = LunchflowTransaction {
            id: "tx_empty".to_string(),
            account_id: None,
            date: "2025-01-15".to_string(),
            amount: Decimal::new(-1500, 2),
            currency: "USD".to_string(),
            merchant: Some("Grocery Store".to_string()),
            description: Some("".to_string()), // Empty string, not None
            is_pending: false,
        };

        let client = LunchflowClient::new_with_base_url("test_key", "http://localhost").unwrap();
        let tx = client.map_transaction(&lf_tx);

        // Falls back to lf_merchant when description is empty
        assert_eq!(tx.description, Some("Grocery Store".to_string()));
        assert_eq!(tx.lf_merchant, Some("Grocery Store".to_string()));
        assert_eq!(tx.lf_description, Some("".to_string()));
    }

    #[test]
    fn test_transaction_mapping_whitespace_description() {
        let lf_tx = LunchflowTransaction {
            id: "tx_whitespace".to_string(),
            account_id: None,
            date: "2025-01-15".to_string(),
            amount: Decimal::new(-2000, 2),
            currency: "USD".to_string(),
            merchant: Some("Gas Station".to_string()),
            description: Some("   ".to_string()), // Whitespace only
            is_pending: false,
        };

        let client = LunchflowClient::new_with_base_url("test_key", "http://localhost").unwrap();
        let tx = client.map_transaction(&lf_tx);

        // Falls back to lf_merchant when description is whitespace-only
        assert_eq!(tx.description, Some("Gas Station".to_string()));
        assert_eq!(tx.lf_merchant, Some("Gas Station".to_string()));
    }

    #[test]
    fn test_provider_setup_missing_api_key() {
        let provider = LunchflowProvider::new();
        let result = provider.setup(&serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_default_base_url() {
        // When LUNCHFLOW_BASE_URL env var is not set, should use production
        std::env::remove_var("LUNCHFLOW_BASE_URL");
        let url = get_base_url();
        assert_eq!(url, "https://www.lunchflow.app/api/v1");
    }

    #[test]
    fn test_base_url_trailing_slash_trimmed() {
        let client =
            LunchflowClient::new_with_base_url("test_key", "http://localhost/api/").unwrap();
        assert_eq!(client.base_url, "http://localhost/api");
    }
}
