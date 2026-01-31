//! Auto-tag rule domain entity

use serde::Serialize;

/// An auto-tagging rule that applies tags to matching transactions
#[derive(Debug, Clone, Serialize)]
pub struct AutoTagRule {
    /// Unique rule ID
    pub rule_id: String,
    /// Human-readable rule name
    pub name: String,
    /// SQL WHERE clause condition (e.g., "description ILIKE '%walmart%'")
    pub sql_condition: String,
    /// Tags to apply when rule matches
    pub tags: Vec<String>,
    /// Whether the rule is active
    pub enabled: bool,
    /// Sort order for rule priority
    pub sort_order: i32,
}
