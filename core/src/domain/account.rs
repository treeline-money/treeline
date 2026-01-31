//! Account domain model

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// A financial account owned by the user
/// Note: account_type is a freeform string using Plaid nomenclature.
/// Common values include "depository", "credit", "investment", "loan", "other"
/// but any string is accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    pub nickname: Option<String>,
    pub account_type: Option<String>,
    /// Balance classification: "asset" or "liability"
    /// Determines how balances affect net worth calculations
    pub classification: Option<String>,
    /// ISO 4217 currency code, normalized to uppercase
    pub currency: String,
    pub balance: Option<Decimal>,
    pub institution_name: Option<String>,
    pub institution_url: Option<String>,
    pub institution_domain: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // =========================================================================
    // Manual flag
    // =========================================================================
    /// True if this account was manually created by the user
    pub is_manual: bool,

    // =========================================================================
    // SimpleFIN: ALL fields from API (https://www.simplefin.org/protocol.html)
    // =========================================================================
    /// SimpleFIN account ID (required for dedup)
    pub sf_id: Option<String>,
    /// Account name
    pub sf_name: Option<String>,
    /// ISO 4217 currency code
    pub sf_currency: Option<String>,
    /// Raw balance string
    pub sf_balance: Option<String>,
    /// Available balance (optional)
    pub sf_available_balance: Option<String>,
    /// UNIX timestamp of balance
    pub sf_balance_date: Option<i64>,
    /// Institution name
    pub sf_org_name: Option<String>,
    /// Institution URL
    pub sf_org_url: Option<String>,
    /// Institution domain
    pub sf_org_domain: Option<String>,
    /// Extra blob pass-through (optional)
    pub sf_extra: Option<JsonValue>,

    // =========================================================================
    // Lunchflow: ALL fields from API
    // =========================================================================
    /// Lunchflow account ID (required for dedup)
    pub lf_id: Option<String>,
    /// Account name
    pub lf_name: Option<String>,
    /// Bank/institution name
    pub lf_institution_name: Option<String>,
    /// Logo URL
    pub lf_institution_logo: Option<String>,
    /// Provider: "gocardless", "quiltt", etc.
    pub lf_provider: Option<String>,
    /// Currency code
    pub lf_currency: Option<String>,
    /// Status: "ACTIVE", "DISCONNECTED", "ERROR"
    pub lf_status: Option<String>,
}

impl Account {
    /// Create a new account with required fields
    pub fn new(id: Uuid, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id,
            name: name.into(),
            nickname: None,
            account_type: None,
            classification: Some("asset".to_string()),
            currency: "USD".to_string(),
            balance: None,
            institution_name: None,
            institution_url: None,
            institution_domain: None,
            created_at: now,
            updated_at: now,
            // Manual flag
            is_manual: false,
            // SimpleFIN fields
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
            // Lunchflow fields
            lf_id: None,
            lf_name: None,
            lf_institution_name: None,
            lf_institution_logo: None,
            lf_provider: None,
            lf_currency: None,
            lf_status: None,
        }
    }

    /// Compute classification based on account_type (Plaid nomenclature)
    /// credit and loan are liabilities, everything else is an asset
    pub fn compute_classification(account_type: Option<&str>) -> String {
        match account_type.map(|t| t.to_lowercase()).as_deref() {
            Some("credit") | Some("loan") => "liability".to_string(),
            _ => "asset".to_string(),
        }
    }

    /// Normalize currency code to uppercase
    pub fn normalize_currency(currency: &str) -> String {
        currency.trim().to_uppercase()
    }

    /// Validate account data
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.trim().is_empty() {
            return Err("account name cannot be empty");
        }
        if self.currency.trim().is_empty() {
            return Err("currency cannot be empty");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_normalization() {
        assert_eq!(Account::normalize_currency("usd"), "USD");
        assert_eq!(Account::normalize_currency(" eur "), "EUR");
    }

    #[test]
    fn test_account_validation() {
        let mut account = Account::new(Uuid::new_v4(), "Test Account");
        assert!(account.validate().is_ok());

        account.name = "".to_string();
        assert!(account.validate().is_err());
    }
}
