//! Balance service - balance snapshot management

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::adapters::duckdb::DuckDbRepository;
use crate::domain::BalanceSnapshot;

/// Balance service for balance snapshot management
pub struct BalanceService {
    repository: Arc<DuckDbRepository>,
}

impl BalanceService {
    pub fn new(repository: Arc<DuckDbRepository>) -> Self {
        Self { repository }
    }

    /// Add a manual balance snapshot
    pub fn add_balance(
        &self,
        account_id: &str,
        balance: Decimal,
        date: Option<NaiveDate>,
    ) -> Result<BalanceResult> {
        // Verify account exists
        if self.repository.get_account_by_id(account_id)?.is_none() {
            anyhow::bail!("Account not found: {}", account_id);
        }

        let account_uuid = Uuid::parse_str(account_id)?;

        // Use midnight for the snapshot time (matches Python behavior)
        let snapshot_time = date
            .map(|d| d.and_hms_opt(0, 0, 0).unwrap())
            .unwrap_or_else(|| Utc::now().naive_utc());

        // Check for duplicate balance snapshot (same account + date + balance)
        // Python allows multiple snapshots per day if the balance is different
        let snapshot_date = snapshot_time.date();
        let existing_snapshots = self.repository.get_balance_snapshots(Some(account_id))?;
        let has_same_balance = existing_snapshots.iter().any(|s| {
            s.snapshot_time.date() == snapshot_date
                && (s.balance - balance).abs() < Decimal::new(1, 2) // Within 0.01
        });
        if has_same_balance {
            anyhow::bail!(
                "Balance snapshot already exists for {} with same balance",
                snapshot_date
            );
        }

        let snapshot = BalanceSnapshot {
            id: Uuid::new_v4(),
            account_id: account_uuid,
            balance,
            snapshot_time,
            source: Some("manual".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        self.repository.add_balance_snapshot(&snapshot)?;

        Ok(BalanceResult {
            snapshot_id: snapshot.id.to_string(),
            account_id: account_id.to_string(),
            balance: balance.to_string(),
            snapshot_time: snapshot_time.to_string(),
        })
    }

    /// Preview what balance snapshots would be created/replaced
    ///
    /// Returns a list of BalanceSnapshotPreview showing calculated end-of-day balances
    /// working backwards from a known balance on a known date.
    ///
    /// Includes all dates that have either:
    /// - A transaction (we calculate a new balance)
    /// - An existing snapshot (we show what will be replaced)
    ///
    /// Parameters:
    /// - account_id: The account to preview
    /// - known_balance: A known balance amount (end-of-day on known_date)
    /// - known_date: The date of the known balance (used as calculation anchor)
    /// - start_date: Optional start of date range (inclusive)
    /// - end_date: Optional end of date range (inclusive), defaults to known_date
    pub fn backfill_preview(
        &self,
        account_id: &str,
        known_balance: Decimal,
        known_date: NaiveDate,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<Vec<BalanceSnapshotPreview>> {
        // Verify account exists
        if self.repository.get_account_by_id(account_id)?.is_none() {
            anyhow::bail!("Account not found: {}", account_id);
        }

        // Determine date range for output (defaults to known_date as end)
        let range_end = end_date.unwrap_or(known_date);

        // Get existing balance snapshots for this account
        let existing_snapshots = self.repository.get_balance_snapshots(Some(account_id))?;

        // Build map of existing snapshot dates to their details
        let existing_by_date: HashMap<NaiveDate, &BalanceSnapshot> = existing_snapshots
            .iter()
            .map(|s| (s.snapshot_time.date(), s))
            .collect();

        // Get transactions for this account
        let transactions = self.repository.get_transactions_by_account(account_id)?;

        // Group transactions by date and calculate daily totals
        let mut daily_totals: HashMap<NaiveDate, Decimal> = HashMap::new();
        let mut transactions_by_date: HashMap<NaiveDate, Vec<&crate::domain::Transaction>> =
            HashMap::new();
        for tx in &transactions {
            *daily_totals
                .entry(tx.transaction_date)
                .or_insert(Decimal::ZERO) += tx.amount;
            transactions_by_date
                .entry(tx.transaction_date)
                .or_default()
                .push(tx);
        }

        // Collect ALL dates we need to consider:
        // - Dates with transactions (we calculate new balances)
        // - Dates with existing snapshots (we show what will be replaced)
        let mut all_dates: HashSet<NaiveDate> = daily_totals.keys().copied().collect();
        for snapshot in &existing_snapshots {
            let snapshot_date = snapshot.snapshot_time.date();
            if snapshot_date <= known_date {
                all_dates.insert(snapshot_date);
            }
        }

        if all_dates.is_empty() {
            return Ok(vec![]);
        }

        // Sort descending (most recent first) for backwards calculation
        let mut dates_for_calculation: Vec<NaiveDate> =
            all_dates.into_iter().filter(|d| *d <= known_date).collect();
        dates_for_calculation.sort_by(|a, b| b.cmp(a));

        // Calculate balances working backwards from known_date
        // known_balance is the END-OF-DAY balance on known_date
        let mut previews: Vec<BalanceSnapshotPreview> = Vec::new();
        let mut current_balance = known_balance;

        for date in dates_for_calculation {
            // Get transaction total for this date (0 if no transactions)
            let daily_total = daily_totals.get(&date).copied().unwrap_or(Decimal::ZERO);

            // Check if this date is within the requested output range
            let in_range = date <= range_end && start_date.map_or(true, |s| date >= s);

            if in_range {
                // Check if existing snapshot exists
                let existing = existing_by_date.get(&date);
                let existing_balance = existing.map(|e| e.balance);
                // source is already Option<String>, so flatten it
                let existing_source = existing.and_then(|e| e.source.clone());

                // Get transactions for this date
                let day_transactions: Vec<TransactionSummary> = transactions_by_date
                    .get(&date)
                    .map(|txs| {
                        txs.iter()
                            .map(|tx| TransactionSummary {
                                description: tx.description.clone().unwrap_or_default(),
                                amount: tx.amount.to_f64().unwrap_or(0.0),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                previews.push(BalanceSnapshotPreview {
                    date: date.format("%Y-%m-%d").to_string(),
                    balance: current_balance.to_f64().unwrap_or(0.0),
                    daily_change: daily_total.to_f64().unwrap_or(0.0),
                    transactions: day_transactions,
                    is_new: existing.is_none(),
                    existing_balance: existing_balance.map(|b| b.to_f64().unwrap_or(0.0)),
                    existing_source,
                });
            }

            // Always update current_balance even if date is outside range
            // (we need to track the running balance for accuracy)
            current_balance -= daily_total;
        }

        // Already sorted descending (most recent first) from dates_for_calculation
        Ok(previews)
    }

    /// Execute balance backfill from a known balance
    ///
    /// Creates balance snapshots based on transaction history, replacing any
    /// existing snapshots in the date range (regardless of source).
    ///
    /// Parameters:
    /// - account_id: The account to backfill
    /// - known_balance: A known balance amount (end-of-day on known_date)
    /// - known_date: The date of the known balance (used as calculation anchor)
    /// - start_date: Optional start of date range (inclusive)
    /// - end_date: Optional end of date range (inclusive), defaults to known_date
    pub fn backfill_execute(
        &self,
        account_id: &str,
        known_balance: Decimal,
        known_date: NaiveDate,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<BackfillExecuteResult> {
        // Verify account exists
        let account = self
            .repository
            .get_account_by_id(account_id)?
            .ok_or_else(|| anyhow::anyhow!("Account not found: {}", account_id))?;
        let account_uuid = account.id;

        // Determine date range for output (defaults to known_date as end)
        let range_end = end_date.unwrap_or(known_date);

        // Get existing balance snapshots for this account (to know which dates had snapshots)
        let existing_snapshots = self.repository.get_balance_snapshots(Some(account_id))?;

        // Get transactions for this account
        let transactions = self.repository.get_transactions_by_account(account_id)?;

        // Aggregate transactions by date (sum amounts per day)
        let mut daily_totals: HashMap<NaiveDate, Decimal> = HashMap::new();
        for tx in &transactions {
            *daily_totals
                .entry(tx.transaction_date)
                .or_insert(Decimal::ZERO) += tx.amount;
        }

        // Collect ALL dates we need to consider:
        // - Dates with transactions (we calculate new balances)
        // - Dates with existing snapshots (we replace them)
        let mut all_dates: HashSet<NaiveDate> = daily_totals.keys().copied().collect();
        for snapshot in &existing_snapshots {
            let snapshot_date = snapshot.snapshot_time.date();
            if snapshot_date <= known_date {
                all_dates.insert(snapshot_date);
            }
        }

        if all_dates.is_empty() {
            return Ok(BackfillExecuteResult {
                snapshots_created: 0,
                snapshots_updated: 0,
                snapshots_skipped: 0,
            });
        }

        // Filter to dates within range and sort descending for backwards calculation
        let range_start = start_date.unwrap_or(NaiveDate::MIN);
        let mut dates_in_range: Vec<NaiveDate> = all_dates
            .iter()
            .filter(|d| **d <= range_end && **d >= range_start && **d <= known_date)
            .copied()
            .collect();
        dates_in_range.sort_by(|a, b| b.cmp(a));

        if dates_in_range.is_empty() {
            return Ok(BackfillExecuteResult {
                snapshots_created: 0,
                snapshots_updated: 0,
                snapshots_skipped: 0,
            });
        }

        // Delete ALL existing snapshots in the date range first
        // This handles duplicates and ensures clean replacement
        let actual_start = *dates_in_range.last().unwrap();
        let actual_end = *dates_in_range.first().unwrap();
        let deleted = self.repository.delete_balance_snapshots_in_range(
            account_id,
            actual_start,
            actual_end,
        )?;

        // Calculate balances working backwards from known_date
        // We need to iterate through ALL dates up to known_date to get correct running balance
        let mut all_dates_sorted: Vec<NaiveDate> =
            all_dates.into_iter().filter(|d| *d <= known_date).collect();
        all_dates_sorted.sort_by(|a, b| b.cmp(a));

        let mut current_balance = known_balance;
        let mut created = 0i64;

        for date in all_dates_sorted {
            let daily_total = daily_totals.get(&date).copied().unwrap_or(Decimal::ZERO);

            // Only create snapshot if date is within the output range
            let in_range = date <= range_end && date >= range_start;

            if in_range {
                // End of day timestamp
                let end_of_day = NaiveDateTime::new(
                    date,
                    NaiveTime::from_hms_micro_opt(23, 59, 59, 999999).unwrap(),
                );

                let snapshot = BalanceSnapshot {
                    id: Uuid::new_v4(),
                    account_id: account_uuid,
                    balance: current_balance,
                    snapshot_time: end_of_day,
                    source: Some("backfill".to_string()),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                };
                self.repository.add_balance_snapshot(&snapshot)?;
                created += 1;
            }

            // Always update current_balance even if date is outside range
            current_balance -= daily_total;
        }

        Ok(BackfillExecuteResult {
            snapshots_created: created,
            snapshots_updated: deleted as i64, // "updated" now means "replaced/deleted"
            snapshots_skipped: 0,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct BalanceResult {
    pub snapshot_id: String,
    pub account_id: String,
    pub balance: String,
    pub snapshot_time: String,
}

/// Summary of a transaction for preview display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionSummary {
    pub description: String,
    pub amount: f64,
}

/// Preview of a single balance snapshot for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceSnapshotPreview {
    pub date: String,
    /// The calculated balance for this date (end of day)
    pub balance: f64,
    /// Net transaction amount for this day (positive = inflow, negative = outflow), 0 if no transactions
    pub daily_change: f64,
    /// Transactions that occurred on this day
    pub transactions: Vec<TransactionSummary>,
    /// True if no existing snapshot exists (will be created)
    pub is_new: bool,
    /// The existing balance if one exists (will be replaced)
    pub existing_balance: Option<f64>,
    /// Source of existing snapshot: "sync", "manual", "backfill", "import", or null
    pub existing_source: Option<String>,
}

/// Result of executing balance backfill
#[derive(Debug, Serialize)]
pub struct BackfillExecuteResult {
    pub snapshots_created: i64,
    pub snapshots_updated: i64,
    pub snapshots_skipped: i64,
}
