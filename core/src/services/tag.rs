//! Tag service - transaction tagging

use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use uuid::Uuid;

use crate::adapters::duckdb::DuckDbRepository;

/// Tag service for transaction tagging
pub struct TagService {
    repository: Arc<DuckDbRepository>,
}

impl TagService {
    pub fn new(repository: Arc<DuckDbRepository>) -> Self {
        Self { repository }
    }

    /// Apply auto-tag rules to a set of transactions
    ///
    /// This fetches all enabled rules and applies matching tags to the given transactions.
    /// Rules are additive - they only add tags, never remove existing ones.
    /// All matching rules apply (not first-match-wins).
    pub fn apply_auto_tag_rules(&self, tx_ids: &[Uuid]) -> Result<AutoTagResult> {
        if tx_ids.is_empty() {
            return Ok(AutoTagResult {
                rules_evaluated: 0,
                rules_matched: 0,
                transactions_tagged: 0,
                failed_rules: Vec::new(),
            });
        }

        // Get all enabled rules
        let rules = self.repository.get_enabled_auto_tag_rules()?;

        if rules.is_empty() {
            return Ok(AutoTagResult {
                rules_evaluated: 0,
                rules_matched: 0,
                transactions_tagged: 0,
                failed_rules: Vec::new(),
            });
        }

        let mut rules_matched = 0;
        let mut transactions_tagged_set = std::collections::HashSet::new();
        let mut failed_rules = Vec::new();

        // For each rule, find matching transactions and apply tags in bulk
        // Each rule uses a single DB connection for both matching and updating
        for rule in &rules {
            // Skip rules with no tags to apply
            if rule.tags.is_empty() {
                continue;
            }

            // Find matching transactions and apply tags in a single DB connection
            match self.repository.bulk_apply_tags_to_matching(
                tx_ids,
                &rule.sql_condition,
                &rule.tags,
            ) {
                Ok(modified_ids) => {
                    if !modified_ids.is_empty() {
                        rules_matched += 1;
                    }
                    for id in modified_ids {
                        transactions_tagged_set.insert(id);
                    }
                }
                Err(e) => {
                    // Rule condition failed - record the failure and continue
                    // Sanitize error message to avoid leaking user data
                    let sanitized_error = sanitize_sql_error(&e.to_string());
                    failed_rules.push(RuleFailure {
                        rule_id: rule.rule_id.clone(),
                        rule_name: rule.name.clone(),
                        error: sanitized_error,
                    });
                }
            }
        }

        Ok(AutoTagResult {
            rules_evaluated: rules.len() as i64,
            rules_matched,
            transactions_tagged: transactions_tagged_set.len() as i64,
            failed_rules,
        })
    }

    /// Apply tags to transactions
    pub fn apply_tags(
        &self,
        tx_ids: &[String],
        tags: &[String],
        replace: bool,
    ) -> Result<TagResult> {
        let mut results = Vec::new();
        let mut succeeded = 0i64;
        let mut failed = 0i64;

        for tx_id in tx_ids {
            match self.apply_tags_to_transaction(tx_id, tags, replace) {
                Ok(applied_tags) => {
                    succeeded += 1;
                    results.push(TagResultEntry {
                        transaction_id: tx_id.clone(),
                        tags: Some(applied_tags),
                        success: true,
                        error: None,
                    });
                }
                Err(e) => {
                    failed += 1;
                    results.push(TagResultEntry {
                        transaction_id: tx_id.clone(),
                        tags: None,
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        Ok(TagResult {
            succeeded,
            failed,
            results,
        })
    }

    fn apply_tags_to_transaction(
        &self,
        tx_id: &str,
        new_tags: &[String],
        replace: bool,
    ) -> Result<Vec<String>> {
        // Validate UUID format upfront (matches Python behavior)
        if Uuid::parse_str(tx_id).is_err() {
            anyhow::bail!("Invalid UUID: {}", tx_id);
        }

        let final_tags = if replace {
            new_tags.to_vec()
        } else {
            // Get existing tags and merge
            if let Some(tx) = self.repository.get_transaction_by_id(tx_id)? {
                let mut tags = tx.tags;
                for tag in new_tags {
                    if !tags.contains(tag) {
                        tags.push(tag.clone());
                    }
                }
                tags
            } else {
                anyhow::bail!("Transaction not found");
            }
        };

        self.repository
            .update_transaction_tags(tx_id, &final_tags)?;
        Ok(final_tags)
    }
}

/// Result structure matching Python CLI output
#[derive(Debug, Serialize)]
pub struct TagResult {
    pub succeeded: i64,
    pub failed: i64,
    pub results: Vec<TagResultEntry>,
}

/// Individual transaction result entry
#[derive(Debug, Serialize)]
pub struct TagResultEntry {
    pub transaction_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result of applying auto-tag rules
#[derive(Debug, Serialize)]
pub struct AutoTagResult {
    /// Number of rules evaluated
    pub rules_evaluated: i64,
    /// Number of rules that matched at least one transaction
    pub rules_matched: i64,
    /// Number of transactions that had tags applied
    pub transactions_tagged: i64,
    /// Rules that failed to apply (with error messages)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed_rules: Vec<RuleFailure>,
}

/// Information about a failed rule application
#[derive(Debug, Serialize, Clone)]
pub struct RuleFailure {
    /// Rule ID
    pub rule_id: String,
    /// Rule name
    pub rule_name: String,
    /// Error message (sanitized - no user data)
    pub error: String,
}

/// Sanitize SQL error messages to avoid leaking user data
///
/// DuckDB error messages can contain the SQL query which may include
/// user-entered patterns. We extract just the error type/category.
fn sanitize_sql_error(error: &str) -> String {
    // Common DuckDB error patterns
    if error.contains("Parser Error") {
        return "SQL syntax error in rule condition".to_string();
    }
    if error.contains("Binder Error") {
        return "Invalid column or table reference in rule condition".to_string();
    }
    if error.contains("Invalid Input Error") {
        return "Invalid input in rule condition".to_string();
    }
    if error.contains("Catalog Error") {
        return "Unknown function or table in rule condition".to_string();
    }
    if error.contains("regexp") || error.contains("regex") {
        return "Invalid regex pattern in rule condition".to_string();
    }
    // Generic fallback - don't include the actual error text
    "Rule condition failed to execute".to_string()
}
