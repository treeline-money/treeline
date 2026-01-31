//! Transaction domain model

use chrono::{DateTime, NaiveDate, Utc};
use regex::Regex;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// A single financial transaction belonging to an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: Uuid,
    pub account_id: Uuid,
    pub amount: Decimal,
    pub description: Option<String>,
    pub transaction_date: NaiveDate,
    pub posted_date: NaiveDate,
    /// Tags for categorization
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Soft delete timestamp
    pub deleted_at: Option<DateTime<Utc>>,
    /// Parent transaction ID for splits
    pub parent_transaction_id: Option<Uuid>,

    // =========================================================================
    // CSV Import tracking
    // =========================================================================
    /// Hash for CSV re-import protection
    pub csv_fingerprint: Option<String>,
    /// Which import batch this transaction belongs to
    pub csv_batch_id: Option<String>,

    // =========================================================================
    // Manual flag
    // =========================================================================
    /// True if this transaction was manually created by the user
    pub is_manual: bool,

    // =========================================================================
    // Auto-tag tracking
    // =========================================================================
    /// True if any tags on this transaction were applied by auto-tag rules
    pub tags_auto_applied: bool,

    // =========================================================================
    // SimpleFIN: ALL fields from API (https://www.simplefin.org/protocol.html)
    // =========================================================================
    /// SimpleFIN transaction ID (required for dedup)
    pub sf_id: Option<String>,
    /// UNIX timestamp when posted (required)
    pub sf_posted: Option<i64>,
    /// Raw amount string (required)
    pub sf_amount: Option<String>,
    /// Raw description (required)
    pub sf_description: Option<String>,
    /// UNIX timestamp of actual transaction (optional)
    pub sf_transacted_at: Option<i64>,
    /// Is transaction pending (optional)
    pub sf_pending: Option<bool>,
    /// Extra blob pass-through (optional)
    pub sf_extra: Option<JsonValue>,

    // =========================================================================
    // Lunchflow: ALL fields from API
    // =========================================================================
    /// Lunchflow transaction ID (required for dedup)
    pub lf_id: Option<String>,
    /// Lunchflow account ID
    pub lf_account_id: Option<String>,
    /// Raw amount
    pub lf_amount: Option<Decimal>,
    /// Currency code
    pub lf_currency: Option<String>,
    /// Transaction date
    pub lf_date: Option<NaiveDate>,
    /// Merchant/counterparty name
    pub lf_merchant: Option<String>,
    /// Description/memo
    pub lf_description: Option<String>,
    /// Is transaction pending
    pub lf_is_pending: Option<bool>,
}

impl Transaction {
    /// Create a new transaction with required fields
    pub fn new(id: Uuid, account_id: Uuid, amount: Decimal, transaction_date: NaiveDate) -> Self {
        let now = Utc::now();
        Self {
            id,
            account_id,
            amount,
            description: None,
            transaction_date,
            posted_date: transaction_date,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            parent_transaction_id: None,
            // CSV Import tracking
            csv_fingerprint: None,
            csv_batch_id: None,
            // Manual flag
            is_manual: false,
            // Auto-tag tracking
            tags_auto_applied: false,
            // SimpleFIN fields
            sf_id: None,
            sf_posted: None,
            sf_amount: None,
            sf_description: None,
            sf_transacted_at: None,
            sf_pending: None,
            sf_extra: None,
            // Lunchflow fields
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

    /// Ensure csv_fingerprint is set
    pub fn ensure_fingerprint(&mut self) {
        if self.csv_fingerprint.is_none() {
            self.csv_fingerprint = Some(self.calculate_fingerprint());
        }
    }

    /// Get the fingerprint if present
    pub fn fingerprint(&self) -> Option<&str> {
        self.csv_fingerprint.as_deref()
    }

    /// Calculate fingerprint hash for deduplication
    ///
    /// Uses: account_id, transaction_date, amount (with sign), and normalized description.
    ///
    /// Description normalization handles CSV vs SimpleFIN format differences:
    /// - Removes literal "null" strings (CSV exports)
    /// - Removes card number masks (XXXXXXXXXXXX1234 - CSV only)
    /// - Normalizes account/phone numbers to last 4 digits
    /// - Removes whitespace and special characters
    pub fn calculate_fingerprint(&self) -> String {
        let tx_date = self.transaction_date.format("%Y-%m-%d").to_string();

        // Normalize amount: treat -0 as 0
        let amount = if self.amount == Decimal::ZERO {
            Decimal::ZERO.abs()
        } else {
            self.amount
        };
        let amount_normalized = format!("{:.2}", amount);

        // Normalize description
        let desc_normalized = Self::normalize_description(self.description.as_deref());

        let fingerprint_str = format!(
            "{}|{}|{}|{}",
            self.account_id, tx_date, amount_normalized, desc_normalized
        );

        // SHA256 hash, truncated to 16 chars
        let mut hasher = Sha256::new();
        hasher.update(fingerprint_str.as_bytes());
        let result = hasher.finalize();
        hex::encode(&result[..8]) // 16 hex chars
    }

    /// Normalize description for fingerprint comparison
    fn normalize_description(desc: Option<&str>) -> String {
        let desc = desc.unwrap_or("").to_lowercase();

        // Remove literal "null" strings (common in CSV exports)
        let null_re = Regex::new(r"\bnull\b").unwrap();
        let mut normalized = null_re.replace_all(&desc, "").to_string();

        // Remove card number masks (10+ X's followed by 4 digits)
        let card_mask_re = Regex::new(r"x{10,}\d{4}").unwrap();
        normalized = card_mask_re.replace_all(&normalized, "").to_string();

        // Normalize phone/account numbers (7-12 chars of X's and digits)
        // Keep only last 4 digits
        let account_re = Regex::new(r"[x0-9]{7,12}").unwrap();
        normalized = account_re
            .replace_all(&normalized, |caps: &regex::Captures| {
                let text = caps.get(0).unwrap().as_str();
                let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() >= 4 {
                    digits[digits.len() - 4..].to_string()
                } else {
                    text.to_string()
                }
            })
            .to_string();

        // Remove whitespace
        let whitespace_re = Regex::new(r"\s+").unwrap();
        normalized = whitespace_re.replace_all(&normalized, "").to_string();

        // Remove all special characters, keep only alphanumeric
        let special_re = Regex::new(r"[^a-z0-9]").unwrap();
        special_re.replace_all(&normalized, "").to_string()
    }

    /// Normalize tags: deduplicate, trim whitespace, remove empty
    pub fn normalize_tags(tags: &[String]) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for tag in tags {
            let trimmed = tag.trim().to_string();
            if !trimmed.is_empty() && seen.insert(trimmed.clone()) {
                result.push(trimmed);
            }
        }

        result
    }
}

// Need hex encoding for fingerprint
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_generation() {
        let account_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new(-5000, 2), // -50.00
            NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
        );
        tx.description = Some("ACME STORE".to_string());

        let fp = tx.calculate_fingerprint();
        assert_eq!(fp.len(), 16);
    }

    #[test]
    fn test_description_normalization() {
        // Card mask removal
        assert!(
            !Transaction::normalize_description(Some("PURCHASE XXXXXXXXXXXX1234 STORE"))
                .contains("xxxx")
        );

        // Null removal
        assert!(!Transaction::normalize_description(Some("null PAYMENT null")).contains("null"));

        // Account number normalization
        let normalized = Transaction::normalize_description(Some("PAYMENT 7208987070"));
        assert!(normalized.contains("7070"));
    }

    #[test]
    fn test_tag_normalization() {
        let tags = vec![
            "food".to_string(),
            "  groceries ".to_string(),
            "food".to_string(), // duplicate
            "".to_string(),     // empty
        ];
        let normalized = Transaction::normalize_tags(&tags);
        assert_eq!(normalized, vec!["food", "groceries"]);
    }
}
