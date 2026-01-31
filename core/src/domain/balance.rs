//! Balance snapshot domain model

use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents an account balance captured at a point in time
/// Note: source is a freeform string to match Python CLI behavior.
/// Common values include "sync", "manual", "backfill" but any string is accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceSnapshot {
    pub id: Uuid,
    pub account_id: Uuid,
    pub balance: Decimal,
    /// When the balance was captured (naive datetime, local time)
    pub snapshot_time: NaiveDateTime,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// How this snapshot was created (e.g., "sync", "manual", "backfill")
    pub source: Option<String>,
}

impl BalanceSnapshot {
    /// Create a new balance snapshot
    pub fn new(account_id: Uuid, balance: Decimal, snapshot_time: NaiveDateTime) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            account_id,
            balance,
            snapshot_time,
            created_at: now,
            updated_at: now,
            source: None,
        }
    }

    /// Create a snapshot from a sync operation
    pub fn from_sync(account_id: Uuid, balance: Decimal, snapshot_time: NaiveDateTime) -> Self {
        let mut snapshot = Self::new(account_id, balance, snapshot_time);
        snapshot.source = Some("sync".to_string());
        snapshot
    }

    /// Create a snapshot from manual entry
    pub fn from_manual(account_id: Uuid, balance: Decimal, snapshot_time: NaiveDateTime) -> Self {
        let mut snapshot = Self::new(account_id, balance, snapshot_time);
        snapshot.source = Some("manual".to_string());
        snapshot
    }

    /// Create a snapshot from backfill
    pub fn from_backfill(account_id: Uuid, balance: Decimal, snapshot_time: NaiveDateTime) -> Self {
        let mut snapshot = Self::new(account_id, balance, snapshot_time);
        snapshot.source = Some("backfill".to_string());
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_snapshot_creation() {
        let account_id = Uuid::new_v4();
        let balance = Decimal::new(10000, 2); // 100.00
        let time =
            NaiveDateTime::parse_from_str("2025-01-15 10:30:00", "%Y-%m-%d %H:%M:%S").unwrap();

        let snapshot = BalanceSnapshot::from_sync(account_id, balance, time);

        assert_eq!(snapshot.account_id, account_id);
        assert_eq!(snapshot.balance, balance);
        assert_eq!(snapshot.source, Some("sync".to_string()));
    }
}
