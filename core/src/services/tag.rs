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
            });
        }

        // Get all enabled rules
        let rules = self.repository.get_enabled_auto_tag_rules()?;

        if rules.is_empty() {
            return Ok(AutoTagResult {
                rules_evaluated: 0,
                rules_matched: 0,
                transactions_tagged: 0,
            });
        }

        let mut rules_matched = 0;
        let mut transactions_tagged_set = std::collections::HashSet::new();

        // For each rule, find matching transactions and apply tags
        for rule in &rules {
            // Skip rules with no tags to apply
            if rule.tags.is_empty() {
                continue;
            }

            // Find which transactions match this rule's condition
            let matching_tx_ids = match self
                .repository
                .get_transactions_matching_rule(tx_ids, &rule.sql_condition)
            {
                Ok(ids) => ids,
                Err(_) => {
                    // Rule condition failed (invalid SQL?) - skip this rule and continue
                    continue;
                }
            };

            if matching_tx_ids.is_empty() {
                continue;
            }

            rules_matched += 1;

            // Apply tags to each matching transaction (additive)
            for tx_id in &matching_tx_ids {
                // Get existing tags
                if let Some(tx) = self.repository.get_transaction_by_id(&tx_id.to_string())? {
                    let mut tags = tx.tags;

                    // Merge new tags (additive, no duplicates)
                    let mut changed = false;
                    for tag in &rule.tags {
                        if !tags.contains(tag) {
                            tags.push(tag.clone());
                            changed = true;
                        }
                    }

                    // Update if we added new tags (and mark as auto-applied)
                    if changed {
                        self.repository
                            .update_transaction_tags_auto(&tx_id.to_string(), &tags)?;
                        transactions_tagged_set.insert(*tx_id);
                    }
                }
            }
        }

        Ok(AutoTagResult {
            rules_evaluated: rules.len() as i64,
            rules_matched,
            transactions_tagged: transactions_tagged_set.len() as i64,
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
}
