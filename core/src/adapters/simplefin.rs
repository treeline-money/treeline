//! SimpleFIN API client
//!
//! Handles communication with the SimpleFIN Bridge API for account and transaction sync.

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use reqwest::blocking::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::domain::{Account, BalanceSnapshot, Transaction};

/// SimpleFIN API client
#[derive(Debug)]
pub struct SimpleFINClient {
    client: Client,
    base_url: String,
    username: String,
    password: String,
}

/// SimpleFIN API response for accounts
#[derive(Debug, Deserialize)]
pub struct AccountsResponse {
    pub accounts: Vec<SimpleFINAccount>,
    #[serde(default)]
    pub errors: Vec<String>,
}

/// SimpleFIN account from API
#[derive(Debug, Deserialize)]
pub struct SimpleFINAccount {
    pub id: String,
    pub name: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub balance: Option<String>,
    #[serde(rename = "available-balance", default)]
    pub available_balance: Option<String>,
    #[serde(rename = "balance-date", default)]
    pub balance_date: Option<i64>,
    #[serde(default)]
    pub org: Option<SimpleFINOrg>,
    #[serde(default)]
    pub transactions: Vec<SimpleFINTransaction>,
}

fn default_currency() -> String {
    "USD".to_string()
}

/// SimpleFIN organization/institution info
#[derive(Debug, Deserialize)]
pub struct SimpleFINOrg {
    pub name: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
}

/// SimpleFIN transaction from API
#[derive(Debug, Deserialize)]
pub struct SimpleFINTransaction {
    pub id: String,
    pub posted: i64,
    pub amount: String,
    #[serde(default)]
    pub description: Option<String>,
    /// UNIX timestamp of actual transaction (optional, may differ from posted)
    #[serde(rename = "transacted_at", default)]
    pub transacted_at: Option<i64>,
    #[serde(default)]
    pub pending: Option<bool>,
    #[serde(default)]
    pub extra: Option<SimpleFINTransactionExtra>,
}

/// Extra transaction metadata (pass-through JSON blob)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimpleFINTransactionExtra {
    pub category: Option<String>,
    // Allow any additional fields to be captured
    #[serde(flatten)]
    pub other: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Result of syncing accounts
pub struct SyncedAccounts {
    pub accounts: Vec<Account>,
    pub balance_snapshots: Vec<BalanceSnapshot>,
    pub warnings: Vec<String>,
}

/// Result of syncing transactions
pub struct SyncedTransactions {
    /// Tuples of (simplefin_account_id, transaction)
    pub transactions: Vec<(String, Transaction)>,
    pub warnings: Vec<String>,
}

impl SimpleFINClient {
    /// Create a new SimpleFIN client from an access URL
    pub fn new(access_url: &str) -> Result<Self> {
        // Parse and validate access URL
        let parsed = Url::parse(access_url).context("Invalid URL format")?;

        // Validate HTTPS
        if parsed.scheme() != "https" {
            anyhow::bail!("SimpleFIN access URL must use HTTPS");
        }

        // Validate domain
        let host = parsed.host_str().unwrap_or("");
        if !host.ends_with("simplefin.org") {
            anyhow::bail!("SimpleFIN access URL must be from simplefin.org domain");
        }

        // Extract credentials
        let username = parsed.username().to_string();
        let password = parsed.password().unwrap_or("").to_string();

        if username.is_empty() || password.is_empty() {
            anyhow::bail!("SimpleFIN access URL must include credentials");
        }

        // Build base URL without credentials
        let base_url = format!(
            "{}://{}{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or(""),
            parsed.path()
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        Ok(Self {
            client,
            base_url,
            username,
            password,
        })
    }

    /// Get accounts from SimpleFIN
    pub fn get_accounts(&self) -> Result<SyncedAccounts> {
        let url = format!("{}/accounts", self.base_url);

        let response = self
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .map_err(|e| self.map_request_error(e))?;

        self.check_response_status(&response)?;

        let data: AccountsResponse = response
            .json()
            .context("Failed to parse SimpleFIN response")?;

        let mut accounts = Vec::new();
        let mut balance_snapshots = Vec::new();
        let warnings = data.errors.clone();

        for sf_account in data.accounts {
            let account = self.map_account(&sf_account);

            // Create balance snapshot if balance is available
            if let Some(balance_str) = &sf_account.balance {
                if let Ok(balance) = balance_str.parse::<Decimal>() {
                    let snapshot_time = sf_account
                        .balance_date
                        .map(|ts| Utc.timestamp_opt(ts, 0).single())
                        .flatten()
                        .unwrap_or_else(Utc::now)
                        .naive_utc();

                    balance_snapshots.push(BalanceSnapshot {
                        id: Uuid::new_v4(),
                        account_id: account.id,
                        balance,
                        snapshot_time,
                        source: Some("sync".to_string()),
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                    });
                }
            }

            accounts.push(account);
        }

        Ok(SyncedAccounts {
            accounts,
            balance_snapshots,
            warnings,
        })
    }

    /// Get transactions from SimpleFIN
    pub fn get_transactions(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        account_ids: Option<&[String]>,
    ) -> Result<SyncedTransactions> {
        let mut url = format!("{}/accounts", self.base_url);

        // Add query parameters
        let start_ts = start_date
            .and_hms_opt(0, 0, 0)
            .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).timestamp())
            .unwrap_or(0);
        let end_ts = end_date
            .and_hms_opt(23, 59, 59)
            .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).timestamp())
            .unwrap_or(0);

        url.push_str(&format!("?start-date={}&end-date={}", start_ts, end_ts));

        // Add account filters if specified
        if let Some(ids) = account_ids {
            for id in ids {
                url.push_str(&format!("&account={}", id));
            }
        }

        let response = self
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .map_err(|e| self.map_request_error(e))?;

        self.check_response_status(&response)?;

        let data: AccountsResponse = response
            .json()
            .context("Failed to parse SimpleFIN response")?;

        let mut transactions = Vec::new();
        let warnings = data.errors.clone();

        for sf_account in data.accounts {
            for sf_tx in sf_account.transactions {
                let tx = self.map_transaction(&sf_tx, &sf_account.id);
                transactions.push((sf_account.id.clone(), tx));
            }
        }

        Ok(SyncedTransactions {
            transactions,
            warnings,
        })
    }

    /// Map SimpleFIN account to domain Account
    fn map_account(&self, sf_account: &SimpleFINAccount) -> Account {
        // Parse balance if available
        let balance = sf_account
            .balance
            .as_ref()
            .and_then(|b| b.parse::<Decimal>().ok());

        // Compute classification based on account_type
        // SimpleFIN doesn't provide account_type, so default to asset
        // Users can override in the UI after sync
        let classification = Some(Account::compute_classification(None));

        let now = Utc::now();
        Account {
            id: Uuid::new_v4(),
            name: sf_account.name.clone(),
            nickname: None,
            currency: sf_account.currency.clone(),
            account_type: None,
            classification,
            balance,
            institution_name: sf_account.org.as_ref().and_then(|o| o.name.clone()),
            institution_url: sf_account.org.as_ref().and_then(|o| o.url.clone()),
            institution_domain: sf_account.org.as_ref().and_then(|o| o.domain.clone()),
            created_at: now,
            updated_at: now,
            // Manual flag
            is_manual: false,
            // SimpleFIN: Store ALL raw fields from API
            sf_id: Some(sf_account.id.clone()),
            sf_name: Some(sf_account.name.clone()),
            sf_currency: Some(sf_account.currency.clone()),
            sf_balance: sf_account.balance.clone(),
            sf_available_balance: sf_account.available_balance.clone(),
            sf_balance_date: sf_account.balance_date,
            sf_org_name: sf_account.org.as_ref().and_then(|o| o.name.clone()),
            sf_org_url: sf_account.org.as_ref().and_then(|o| o.url.clone()),
            sf_org_domain: sf_account.org.as_ref().and_then(|o| o.domain.clone()),
            sf_extra: None, // SimpleFIN accounts don't have extra field
            // Lunchflow fields (not applicable)
            lf_id: None,
            lf_name: None,
            lf_institution_name: None,
            lf_institution_logo: None,
            lf_provider: None,
            lf_currency: None,
            lf_status: None,
        }
    }

    /// Map SimpleFIN transaction to domain Transaction
    fn map_transaction(&self, sf_tx: &SimpleFINTransaction, _sf_account_id: &str) -> Transaction {
        // Parse amount
        let amount = sf_tx.amount.parse::<Decimal>().unwrap_or_default();

        // Convert timestamp to date
        let posted_date = Utc
            .timestamp_opt(sf_tx.posted, 0)
            .single()
            .map(|dt| dt.naive_utc().date())
            .unwrap_or_else(|| Utc::now().naive_utc().date());

        // Extract category as tag if present
        let tags = sf_tx
            .extra
            .as_ref()
            .and_then(|e| e.category.clone())
            .map(|c| vec![c])
            .unwrap_or_default();

        // Convert extra to JSON Value for storage
        let sf_extra = sf_tx
            .extra
            .as_ref()
            .and_then(|e| serde_json::to_value(e).ok());

        let now = Utc::now();
        Transaction {
            id: Uuid::new_v4(),
            account_id: Uuid::nil(), // Will be set by sync service after mapping
            amount,
            description: sf_tx.description.clone(), // Core field mapped from sf_description
            transaction_date: posted_date,
            posted_date,
            tags,
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
            // SimpleFIN: Store ALL raw fields from API
            sf_id: Some(sf_tx.id.clone()),
            sf_posted: Some(sf_tx.posted),
            sf_amount: Some(sf_tx.amount.clone()),
            sf_description: sf_tx.description.clone(),
            sf_transacted_at: sf_tx.transacted_at,
            sf_pending: sf_tx.pending,
            sf_extra,
            // Lunchflow fields (not applicable)
            lf_id: None,
            lf_account_id: None,
            lf_amount: None,
            lf_currency: None,
            lf_date: None,
            lf_merchant: None,
            lf_description: None,
            lf_is_pending: None,
        }
    }

    /// Map request errors to user-friendly messages
    fn map_request_error(&self, error: reqwest::Error) -> anyhow::Error {
        if error.is_timeout() {
            anyhow::anyhow!("Connection timed out after 120 seconds")
        } else if error.is_connect() {
            anyhow::anyhow!("Unable to connect to SimpleFIN servers")
        } else {
            anyhow::anyhow!("SimpleFIN request failed: {}", error)
        }
    }

    /// Check response status and return appropriate errors
    fn check_response_status(&self, response: &reqwest::blocking::Response) -> Result<()> {
        match response.status().as_u16() {
            200 => Ok(()),
            403 => anyhow::bail!(
                "SimpleFIN authentication failed. Your access token may be invalid or revoked. \
                Please reset your SimpleFIN credentials at https://beta-bridge.simplefin.org/"
            ),
            402 => anyhow::bail!(
                "SimpleFIN subscription payment required. \
                Please check your SimpleFIN account at https://beta-bridge.simplefin.org/"
            ),
            status => anyhow::bail!("SimpleFIN API error: HTTP {}", status),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_access_url() {
        let url = "https://user:pass@bridge.simplefin.org/simplefin/accounts";
        let client = SimpleFINClient::new(url);
        assert!(client.is_ok());
    }

    #[test]
    fn test_reject_http_url() {
        let url = "http://user:pass@bridge.simplefin.org/simplefin/accounts";
        let result = SimpleFINClient::new(url);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("HTTPS"));
    }

    #[test]
    fn test_reject_wrong_domain() {
        let url = "https://user:pass@evil.com/simplefin/accounts";
        let result = SimpleFINClient::new(url);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("simplefin.org"));
    }

    #[test]
    fn test_reject_missing_credentials() {
        let url = "https://bridge.simplefin.org/simplefin/accounts";
        let result = SimpleFINClient::new(url);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("credentials"));
    }
}

// =============================================================================
// SimpleFINProvider - implements DataAggregationProvider trait
// =============================================================================

use base64::Engine;
use serde_json::Value as JsonValue;

use crate::domain::result::Result as DomainResult;
use crate::ports::{
    DataAggregationProvider, FetchAccountsResult, FetchTransactionsResult, IntegrationProvider,
};

/// SimpleFIN data provider
///
/// Implements DataAggregationProvider and IntegrationProvider traits
/// for syncing real financial data via SimpleFIN Bridge.
pub struct SimpleFINProvider;

impl SimpleFINProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SimpleFINProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DataAggregationProvider for SimpleFINProvider {
    fn name(&self) -> &str {
        "simplefin"
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
        let access_url = settings
            .get("accessUrl")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::domain::result::Error::Config(
                    "SimpleFIN accessUrl not found in settings".to_string(),
                )
            })?;

        let client = SimpleFINClient::new(access_url)
            .map_err(|e| crate::domain::result::Error::Sync(e.to_string()))?;

        let synced = client
            .get_accounts()
            .map_err(|e| crate::domain::result::Error::Sync(e.to_string()))?;

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
        let access_url = settings
            .get("accessUrl")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::domain::result::Error::Config(
                    "SimpleFIN accessUrl not found in settings".to_string(),
                )
            })?;

        let client = SimpleFINClient::new(access_url)
            .map_err(|e| crate::domain::result::Error::Sync(e.to_string()))?;

        let ids = if account_ids.is_empty() {
            None
        } else {
            Some(account_ids)
        };
        let synced = client
            .get_transactions(start_date, end_date, ids)
            .map_err(|e| crate::domain::result::Error::Sync(e.to_string()))?;

        Ok(FetchTransactionsResult {
            transactions: synced.transactions,
            warnings: synced.warnings,
        })
    }
}

impl IntegrationProvider for SimpleFINProvider {
    fn setup(&self, options: &JsonValue) -> DomainResult<JsonValue> {
        let setup_token = options
            .get("setupToken")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::domain::result::Error::Config(
                    "SimpleFIN setupToken not found in options".to_string(),
                )
            })?;

        // Decode base64 setup token to get claim URL
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(setup_token)
            .map_err(|_| {
                crate::domain::result::Error::Config("Invalid setup token format".to_string())
            })?;

        let claim_url = String::from_utf8(decoded).map_err(|_| {
            crate::domain::result::Error::Config("Invalid setup token encoding".to_string())
        })?;

        // Claim the token to get access URL
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| {
                crate::domain::result::Error::Sync(format!("Failed to create HTTP client: {}", e))
            })?;

        let response = client.post(&claim_url).send().map_err(|e| {
            crate::domain::result::Error::Sync(format!("Failed to claim SimpleFIN token: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(crate::domain::result::Error::Sync(
                "Failed to verify SimpleFIN token".to_string(),
            ));
        }

        let access_url = response.text().map_err(|e| {
            crate::domain::result::Error::Sync(format!("Failed to read SimpleFIN response: {}", e))
        })?;

        if access_url.is_empty() {
            return Err(crate::domain::result::Error::Sync(
                "No access URL received from SimpleFIN".to_string(),
            ));
        }

        Ok(serde_json::json!({
            "accessUrl": access_url
        }))
    }
}
