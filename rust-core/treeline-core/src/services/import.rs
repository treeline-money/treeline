//! Import service - CSV transaction import

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use regex::Regex;
use rust_decimal::Decimal;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::adapters::duckdb::DuckDbRepository;
use crate::config::{ColumnMappings, Config, ImportOptions as ConfigImportOptions, ImportProfile};
use crate::domain::{BalanceSnapshot, Transaction};
use crate::services::TagService;

/// Number format for parsing amounts
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum NumberFormat {
    /// US format: 1,234.56 (comma=thousands, dot=decimal)
    #[default]
    Us,
    /// European format: 1.234,56 (dot=thousands, comma=decimal)
    Eu,
    /// European format with space thousands: 1 234,56
    EuSpace,
}

impl NumberFormat {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "eu" => NumberFormat::Eu,
            "eu_space" => NumberFormat::EuSpace,
            _ => NumberFormat::Us,
        }
    }
}

/// Import options for CSV processing
#[derive(Debug, Default)]
pub struct ImportOptions {
    /// Negate debit values when debits are positive in CSV
    pub debit_negative: bool,
    /// Flip signs on all amounts (for credit card statements)
    pub flip_signs: bool,
    /// Number of rows to skip before the header row
    pub skip_rows: u32,
    /// Number format for parsing amounts
    pub number_format: NumberFormat,
    /// Anchor balance for calculating historical balances (preview only)
    pub anchor_balance: Option<Decimal>,
    /// Anchor date for the anchor balance (preview only)
    pub anchor_date: Option<NaiveDate>,
}

/// Import service for CSV imports
pub struct ImportService {
    repository: Arc<DuckDbRepository>,
    tag_service: TagService,
    treeline_dir: PathBuf,
}

impl ImportService {
    pub fn new(repository: Arc<DuckDbRepository>, treeline_dir: PathBuf) -> Self {
        let tag_service = TagService::new(repository.clone());
        Self {
            repository,
            tag_service,
            treeline_dir,
        }
    }

    /// List saved import profiles
    pub fn list_profiles(&self) -> Result<std::collections::HashMap<String, ImportProfile>> {
        let config = Config::load(&self.treeline_dir)?;
        Ok(config.import_profiles)
    }

    /// Import transactions from CSV
    pub fn import(
        &self,
        file_path: &Path,
        account_id: &str,
        mappings: &ColumnMappings,
        options: &ImportOptions,
        preview_only: bool,
    ) -> Result<ImportResult> {
        // Verify account exists
        if self.repository.get_account_by_id(account_id)?.is_none() {
            anyhow::bail!("Account not found: {}", account_id);
        }

        let account_uuid = Uuid::parse_str(account_id).context("Invalid account ID")?;

        // Read CSV with optional row skipping
        let (headers, records) = if options.skip_rows > 0 {
            // Use raw reader to skip rows before header
            use std::fs::File;
            use std::io::{BufRead, BufReader};

            let file = File::open(file_path).context("Failed to open CSV file")?;
            let buf_reader = BufReader::new(file);
            let mut lines = buf_reader.lines();

            // Skip leading rows
            for _ in 0..options.skip_rows {
                lines.next();
            }

            // Read header line
            let header_line = lines
                .next()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No header row found after skipping {} rows",
                        options.skip_rows
                    )
                })?
                .context("Failed to read header line")?;

            // Detect delimiter (semicolon common in EU, comma in US)
            let semicolons = header_line.matches(';').count();
            let commas = header_line.matches(',').count();
            let tabs = header_line.matches('\t').count();
            let delimiter = if semicolons > commas && semicolons > tabs {
                b';'
            } else if tabs > commas && tabs > semicolons {
                b'\t'
            } else {
                b','
            };

            // Parse headers using csv crate with detected delimiter
            let mut header_reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .delimiter(delimiter)
                .from_reader(header_line.as_bytes());

            let header_record = header_reader
                .records()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Empty header line"))?
                .context("Failed to parse header line")?;

            // Clean headers: trim and strip # prefix
            let headers: Vec<String> = header_record
                .iter()
                .map(|h| h.trim().trim_start_matches('#').to_string())
                .collect();

            // Collect remaining lines as data
            let remaining_content: String =
                lines.filter_map(|l| l.ok()).collect::<Vec<_>>().join("\n");

            // Parse remaining content as CSV records with same delimiter
            let mut data_reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .delimiter(delimiter)
                .from_reader(remaining_content.as_bytes());

            let records: Vec<csv::StringRecord> =
                data_reader.records().filter_map(|r| r.ok()).collect();

            (headers, records)
        } else {
            // Standard path: first row is header
            let mut reader =
                csv::Reader::from_path(file_path).context("Failed to read CSV file")?;

            // Clean headers: trim and strip # prefix
            let headers: Vec<String> = reader
                .headers()?
                .iter()
                .map(|h| h.trim().trim_start_matches('#').to_string())
                .collect();

            let records: Vec<csv::StringRecord> = reader.records().filter_map(|r| r.ok()).collect();

            (headers, records)
        };

        // Find column indices
        let date_idx = headers
            .iter()
            .position(|h| h == mappings.date.as_str())
            .context(format!("Date column '{}' not found", mappings.date))?;

        // Check for debit/credit columns first, fall back to amount
        let debit_idx = mappings
            .debit
            .as_ref()
            .and_then(|d| headers.iter().position(|h| h == d.as_str()));
        let credit_idx = mappings
            .credit
            .as_ref()
            .and_then(|c| headers.iter().position(|h| h == c.as_str()));

        let amount_idx = if debit_idx.is_some() || credit_idx.is_some() {
            None
        } else {
            Some(
                headers
                    .iter()
                    .position(|h| h == mappings.amount.as_str())
                    .context(format!("Amount column '{}' not found", mappings.amount))?,
            )
        };

        let desc_idx = mappings
            .description
            .as_ref()
            .and_then(|d| headers.iter().position(|h| h == d.as_str()));

        // Optional balance column for running balance snapshots
        let balance_idx = mappings
            .balance
            .as_ref()
            .and_then(|b| headers.iter().position(|h| h == b.as_str()));

        let mut transactions = Vec::new();
        let mut skipped = 0;
        // Track end-of-day balances: for each date, store the last balance seen
        let mut end_of_day_balances: HashMap<NaiveDate, Decimal> = HashMap::new();
        // Track per-row balance for preview display
        let mut preview_balances: Vec<Option<String>> = Vec::new();

        for record in &records {
            // Parse date
            let date_str = record.get(date_idx).unwrap_or("");
            let date = parse_date(date_str);
            if date.is_none() {
                skipped += 1;
                continue;
            }
            let date = date.unwrap();

            // Parse amount from either amount column or debit/credit columns
            let amount = if let Some(amt_idx) = amount_idx {
                let amount_str = record.get(amt_idx).unwrap_or("");
                parse_amount_with_format(amount_str, options.number_format)
            } else {
                // Handle debit/credit columns
                // Preserve sign from CSV, only negate if debit_negative option is set
                let debit = debit_idx.and_then(|i| record.get(i)).and_then(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        parse_amount_with_format(s, options.number_format)
                    }
                });
                let credit = credit_idx.and_then(|i| record.get(i)).and_then(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        parse_amount_with_format(s, options.number_format)
                    }
                });

                match (debit, credit) {
                    (Some(d), None) => {
                        // Debit: preserve sign from CSV by default
                        // If debit_negative is true, negate positive values (for unsigned CSVs)
                        let d = if options.debit_negative && d > Decimal::ZERO {
                            -d
                        } else {
                            d
                        };
                        Some(d)
                    }
                    (None, Some(c)) => {
                        // Credit: incoming money (preserve sign)
                        Some(c)
                    }
                    (Some(d), Some(c)) => {
                        // Both present: use the one with larger absolute value
                        if d.abs() >= c.abs() {
                            let d = if options.debit_negative && d > Decimal::ZERO {
                                -d
                            } else {
                                d
                            };
                            Some(d)
                        } else {
                            Some(c)
                        }
                    }
                    (None, None) => None,
                }
            };

            if amount.is_none() {
                skipped += 1;
                continue;
            }

            let mut amount = amount.unwrap();

            // Apply flip_signs if requested (for credit card statements)
            if options.flip_signs {
                amount = -amount;
            }

            // Get description
            let description = desc_idx.and_then(|i| record.get(i)).map(|s| s.to_string());

            // Generate fingerprint for deduplication
            let fingerprint =
                generate_fingerprint(account_id, &date, &amount, description.as_deref());

            let mut tx = Transaction::new(Uuid::new_v4(), account_uuid, amount, date);
            tx.description = description;
            // Use dedicated csv_fingerprint column for deduplication
            tx.csv_fingerprint = Some(fingerprint.clone());

            transactions.push(tx);

            // Collect balance for end-of-day snapshot (if balance column is mapped)
            // We store the last balance seen for each date as we iterate through rows
            // Also capture raw balance for preview display
            let row_balance = if let Some(bal_idx) = balance_idx {
                if let Some(balance_str) = record.get(bal_idx) {
                    if let Some(balance) =
                        parse_amount_with_format(balance_str, options.number_format)
                    {
                        // Overwrite - we want the last balance for each date in CSV order
                        end_of_day_balances.insert(date, balance);
                        Some(balance.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            preview_balances.push(row_balance);
        }

        // Track discovered count (valid transactions before deduplication)
        let discovered = transactions.len() as i64;
        let fingerprints_checked = discovered;

        // Generate batch ID for this import
        let batch_id = format!("import_{}", chrono::Utc::now().format("%Y%m%d_%H%M%S"));

        // For preview mode, return all parsed transactions without deduplication
        // User wants to see what's in the CSV, not what will be imported
        if preview_only {
            // If anchor balance is provided and no balance column exists, calculate balances
            let final_preview_balances = if options.anchor_balance.is_some()
                && options.anchor_date.is_some()
                && preview_balances.iter().all(|b| b.is_none())
            {
                let anchor_balance = options.anchor_balance.unwrap();
                let anchor_date = options.anchor_date.unwrap();

                // Calculate per-transaction running balance (like a bank statement)
                // This shows the balance AFTER each transaction

                // Get unique dates, sorted
                let mut unique_dates: Vec<NaiveDate> =
                    transactions.iter().map(|t| t.transaction_date).collect();
                unique_dates.sort();
                unique_dates.dedup();

                // Calculate the opening balance for each day by working backwards from anchor
                // Closing balance on anchor_date = anchor_balance
                // Opening balance = closing - sum(transactions on that day)
                let mut day_opening_balance: HashMap<NaiveDate, Decimal> = HashMap::new();
                let mut closing_balance = anchor_balance;

                for date in unique_dates.iter().rev() {
                    if *date > anchor_date {
                        continue; // Skip dates after anchor
                    }

                    // Sum of transactions on this date
                    let day_sum: Decimal = transactions
                        .iter()
                        .filter(|t| t.transaction_date == *date)
                        .map(|t| t.amount)
                        .sum();

                    let opening = closing_balance - day_sum;
                    day_opening_balance.insert(*date, opening);

                    // Previous day's closing = this day's opening
                    closing_balance = opening;
                }

                // Calculate per-transaction running balance
                // For each date, start with opening balance and add each transaction
                let mut tx_balances: Vec<Option<String>> = vec![None; transactions.len()];

                for date in &unique_dates {
                    if *date > anchor_date {
                        continue;
                    }

                    let mut balance = *day_opening_balance.get(date).unwrap_or(&Decimal::ZERO);

                    // Process transactions for this date in CSV order (original order)
                    for (idx, tx) in transactions.iter().enumerate() {
                        if tx.transaction_date == *date {
                            balance += tx.amount;
                            tx_balances[idx] = Some(balance.to_string());
                        }
                    }
                }

                tx_balances
            } else {
                preview_balances
            };

            // Sort transactions by date for preview display so running balance flows logically
            // Then reverse so newest is first (standard bank statement order)
            let mut sorted_indices: Vec<usize> = (0..transactions.len()).collect();
            sorted_indices.sort_by_key(|&i| transactions[i].transaction_date);
            sorted_indices.reverse(); // Newest first

            return Ok(ImportResult {
                batch_id,
                discovered,
                imported: 0, // Not importing in preview
                skipped: skipped as i64,
                fingerprints_checked: 0,      // Not checking in preview
                balance_snapshots_created: 0, // Not creating in preview
                preview: true,
                transactions: Some(
                    sorted_indices
                        .iter()
                        .map(|&i| {
                            let t = &transactions[i];
                            TransactionPreview {
                                date: t.transaction_date.to_string(),
                                amount: t.amount.to_string(),
                                description: t.description.clone(),
                                balance: final_preview_balances.get(i).cloned().flatten(),
                            }
                        })
                        .collect(),
                ),
            });
        }

        // Deduplicate: check which fingerprints already exist in csv_fingerprint column
        let mut new_transactions = Vec::new();
        let mut duplicate_count = 0i64;

        for tx in transactions {
            if let Some(fp) = tx.csv_fingerprint.as_ref() {
                // Check csv_fingerprint column for existing transactions
                if self
                    .repository
                    .csv_fingerprint_exists_in_other_batches(fp, "")?
                {
                    duplicate_count += 1;
                    continue;
                }
            }
            new_transactions.push(tx);
        }

        let imported = new_transactions.len() as i64;

        // Add batch_id to each transaction before inserting
        for tx in &mut new_transactions {
            // Use dedicated csv_batch_id column
            tx.csv_batch_id = Some(batch_id.clone());
        }

        // Collect IDs for auto-tagging
        let new_tx_ids: Vec<Uuid> = new_transactions.iter().map(|tx| tx.id).collect();

        for tx in &new_transactions {
            self.repository.upsert_transaction(tx)?;
        }

        // Apply auto-tag rules to newly imported transactions
        if !new_tx_ids.is_empty() {
            // Best-effort tagging - don't fail import if rules fail
            let _ = self.tag_service.apply_auto_tag_rules(&new_tx_ids);
        }

        // Create balance snapshots from collected end-of-day balances
        let mut balance_snapshots_created = 0i64;
        if !end_of_day_balances.is_empty() {
            // Get existing snapshots for deduplication
            let existing_snapshots = self.repository.get_balance_snapshots(Some(account_id))?;

            for (date, balance) in &end_of_day_balances {
                // Create end-of-day timestamp (23:59:59.999999)
                let snapshot_time = NaiveDateTime::new(
                    *date,
                    NaiveTime::from_hms_micro_opt(23, 59, 59, 999999).unwrap(),
                );

                // Check for duplicate: same account + date + balance (within 0.01)
                let is_duplicate = existing_snapshots.iter().any(|s| {
                    s.snapshot_time.date() == *date
                        && (s.balance - *balance).abs() < Decimal::new(1, 2)
                });

                if is_duplicate {
                    continue;
                }

                let snapshot = BalanceSnapshot {
                    id: Uuid::new_v4(),
                    account_id: account_uuid,
                    balance: *balance,
                    snapshot_time,
                    source: Some("csv_import".to_string()),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                };

                // Best-effort - don't fail import if snapshot insert fails
                if self.repository.add_balance_snapshot(&snapshot).is_ok() {
                    balance_snapshots_created += 1;
                }
            }
        }

        Ok(ImportResult {
            batch_id,
            discovered,
            imported,
            skipped: skipped + duplicate_count,
            fingerprints_checked,
            balance_snapshots_created,
            preview: false,
            transactions: None,
        })
    }

    /// Save an import profile
    pub fn save_profile(
        &self,
        name: &str,
        mappings: &ColumnMappings,
        options: &ImportOptions,
    ) -> Result<()> {
        let mut config = Config::load(&self.treeline_dir)?;
        config.import_profiles.insert(
            name.to_string(),
            ImportProfile {
                column_mappings: mappings.clone(),
                date_format: None,
                skip_rows: 0,
                options: ConfigImportOptions {
                    flip_signs: options.flip_signs,
                    debit_negative: options.debit_negative,
                },
            },
        );
        config.save(&self.treeline_dir)?;
        Ok(())
    }

    /// Get a saved profile
    pub fn get_profile(&self, name: &str) -> Result<Option<ImportProfile>> {
        let config = Config::load(&self.treeline_dir)?;
        Ok(config.import_profiles.get(name).cloned())
    }

    /// Auto-detect column mapping from CSV headers
    ///
    /// Returns best-guess mapping for date, amount, description, and optionally debit/credit columns.
    /// Matches Python CLI behavior with same pattern matching.
    pub fn detect_columns(&self, file_path: &Path) -> Result<DetectedColumns> {
        let mut reader = csv::Reader::from_path(file_path).context("Failed to read CSV file")?;

        let headers: Vec<String> = reader.headers()?.iter().map(|h| h.to_string()).collect();

        let date_patterns = [
            "date",
            "transaction date",
            "trans date",
            "txn date",
            "txndate",
            "posted",
            "post date",
            "dt",
        ];
        let desc_patterns = [
            "description",
            "desc",
            "memo",
            "payee",
            "merchant",
            "details",
            "narration",
        ];
        let amount_patterns = ["amount", "amt", "total", "transaction amount"];
        let debit_patterns = ["debit", "dr", "withdrawal", "debit amount"];
        let credit_patterns = ["credit", "cr", "deposit", "credit amount"];

        let mut detected = DetectedColumns::default();

        // Find date column
        for header in &headers {
            let header_lower = header.to_lowercase();
            if date_patterns.iter().any(|p| header_lower.contains(p)) {
                detected.date = Some(header.clone());
                break;
            }
        }

        // Find amount column (prefer single amount column)
        for header in &headers {
            let header_lower = header.to_lowercase();
            if amount_patterns.iter().any(|p| header_lower.contains(p)) {
                detected.amount = Some(header.clone());
                break;
            }
        }

        // If no 'amount' found, check for debit/credit
        if detected.amount.is_none() {
            for header in &headers {
                let header_lower = header.to_lowercase();
                if debit_patterns.iter().any(|p| header_lower.contains(p)) {
                    detected.debit = Some(header.clone());
                }
                if credit_patterns.iter().any(|p| header_lower.contains(p)) {
                    detected.credit = Some(header.clone());
                }
            }
        }

        // Find description column
        for header in &headers {
            let header_lower = header.to_lowercase();
            // Skip if this is the date column
            if detected.date.as_ref() == Some(header) {
                continue;
            }
            if desc_patterns.iter().any(|p| header_lower.contains(p)) {
                detected.description = Some(header.clone());
                break;
            }
        }

        // Fallback for description
        if detected.description.is_none() {
            let fallback_patterns = ["name", "type", "ref", "reference", "category"];
            for header in &headers {
                let header_lower = header.to_lowercase();
                if detected.date.as_ref() == Some(header) {
                    continue;
                }
                if fallback_patterns.iter().any(|p| header_lower.contains(p)) {
                    detected.description = Some(header.clone());
                    break;
                }
            }
        }

        Ok(detected)
    }
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    // Try common formats
    let formats = [
        "%Y-%m-%d", "%m/%d/%Y", "%d/%m/%Y", "%m-%d-%Y", "%d-%m-%Y", "%Y/%m/%d",
    ];

    for fmt in &formats {
        if let Ok(date) = NaiveDate::parse_from_str(s, fmt) {
            return Some(date);
        }
    }
    None
}

/// Strip currency suffix from amount string (e.g., "100.50 PLN" -> "100.50")
fn strip_currency_suffix(s: &str) -> &str {
    const CURRENCIES: &[&str] = &[
        "PLN", "EUR", "USD", "GBP", "CHF", "CZK", "SEK", "NOK", "DKK", "CAD", "AUD", "JPY", "CNY",
        "INR", "BRL", "MXN", "KRW", "RUB",
    ];
    let s = s.trim();
    for currency in CURRENCIES {
        if s.ends_with(currency) {
            return s[..s.len() - currency.len()].trim();
        }
    }
    s
}

#[allow(dead_code)]
fn parse_amount(s: &str) -> Option<Decimal> {
    parse_amount_with_format(s, NumberFormat::Us)
}

fn parse_amount_with_format(s: &str, format: NumberFormat) -> Option<Decimal> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Strip currency suffix first
    let s = strip_currency_suffix(s);

    // Handle parentheses notation for negative numbers: (100.00) -> -100.00
    let (is_negative, s) = if s.starts_with('(') && s.ends_with(')') {
        (true, &s[1..s.len() - 1])
    } else {
        (false, s)
    };

    // Normalize based on format
    let normalized = match format {
        NumberFormat::Us => {
            // US: 1,234.56 - remove commas, keep dots
            s.replace(',', "")
        }
        NumberFormat::Eu => {
            // EU: 1.234,56 - remove dots (thousands), convert comma to dot (decimal)
            s.replace('.', "").replace(',', ".")
        }
        NumberFormat::EuSpace => {
            // EU with space: 1 234,56 - remove spaces, convert comma to dot
            s.replace(' ', "").replace(',', ".")
        }
    };

    // Keep only digits, dot, minus
    let cleaned: String = normalized
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();

    if cleaned.is_empty() {
        return None;
    }

    let mut amount: Decimal = cleaned.parse().ok()?;

    // Apply parentheses negation
    if is_negative && amount > Decimal::ZERO {
        amount = -amount;
    }

    Some(amount)
}

/// Generate a fingerprint for transaction deduplication
/// Based on account_id, date, amount, and normalized description
fn generate_fingerprint(
    account_id: &str,
    date: &NaiveDate,
    amount: &Decimal,
    description: Option<&str>,
) -> String {
    let normalized_desc = description
        .map(|d| normalize_description(d))
        .unwrap_or_default();

    let fingerprint_input = format!("{}|{}|{:.2}|{}", account_id, date, amount, normalized_desc);

    let mut hasher = Sha256::new();
    hasher.update(fingerprint_input.as_bytes());
    let result = hasher.finalize();

    // Return first 16 characters of hex hash
    result[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

/// Normalize description for fingerprinting
/// Matches Python implementation exactly:
/// - Lowercase
/// - Remove literal "null" strings (CSV exports)
/// - Remove card number masks (10+ X's followed by 4 digits)
/// - Normalize account/phone numbers to last 4 digits
/// - Remove whitespace and special characters
fn normalize_description(desc: &str) -> String {
    let desc = desc.to_lowercase();

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

/// Result of column auto-detection
#[derive(Debug, Default, Serialize)]
pub struct DetectedColumns {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credit: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    /// Unique batch ID for this import
    pub batch_id: String,
    /// Total transactions discovered in CSV
    pub discovered: i64,
    /// Successfully imported transactions
    pub imported: i64,
    /// Skipped transactions (invalid or duplicate)
    pub skipped: i64,
    /// Number of fingerprints checked for deduplication
    pub fingerprints_checked: i64,
    /// Number of balance snapshots created from running balance column
    pub balance_snapshots_created: i64,
    /// Whether this was a preview (no changes applied)
    pub preview: bool,
    /// Transaction previews (only in preview mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transactions: Option<Vec<TransactionPreview>>,
}

#[derive(Debug, Serialize)]
pub struct TransactionPreview {
    pub date: String,
    pub amount: String,
    pub description: Option<String>,
    /// Running balance (from CSV, if mapped)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==========================================================================
    // parse_amount tests - US format (current behavior)
    // ==========================================================================

    #[test]
    fn test_parse_amount_simple_positive() {
        assert_eq!(parse_amount("100.50"), Some(Decimal::new(10050, 2)));
    }

    #[test]
    fn test_parse_amount_simple_negative() {
        assert_eq!(parse_amount("-50.25"), Some(Decimal::new(-5025, 2)));
    }

    #[test]
    fn test_parse_amount_with_comma_thousands() {
        // US format: 1,234.56
        assert_eq!(parse_amount("1,234.56"), Some(Decimal::new(123456, 2)));
    }

    #[test]
    fn test_parse_amount_with_currency_symbol() {
        assert_eq!(parse_amount("$100.00"), Some(Decimal::new(10000, 2)));
        assert_eq!(parse_amount("-$50.00"), Some(Decimal::new(-5000, 2)));
    }

    #[test]
    fn test_parse_amount_parentheses_negative() {
        // Accounting notation: (100.00) means -100.00
        assert_eq!(parse_amount("(100.00)"), Some(Decimal::new(-10000, 2)));
        assert_eq!(parse_amount("(1,234.56)"), Some(Decimal::new(-123456, 2)));
    }

    #[test]
    fn test_parse_amount_with_whitespace() {
        assert_eq!(parse_amount("  100.50  "), Some(Decimal::new(10050, 2)));
    }

    #[test]
    fn test_parse_amount_empty_string() {
        assert_eq!(parse_amount(""), None);
    }

    #[test]
    fn test_parse_amount_invalid() {
        assert_eq!(parse_amount("abc"), None);
    }

    // ==========================================================================
    // parse_date tests
    // ==========================================================================

    #[test]
    fn test_parse_date_iso_format() {
        // YYYY-MM-DD (ISO)
        assert_eq!(
            parse_date("2024-01-15"),
            Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap())
        );
    }

    #[test]
    fn test_parse_date_us_format() {
        // MM/DD/YYYY (US)
        assert_eq!(
            parse_date("12/03/2025"),
            Some(NaiveDate::from_ymd_opt(2025, 12, 3).unwrap())
        );
    }

    #[test]
    fn test_parse_date_invalid() {
        assert_eq!(parse_date("not-a-date"), None);
        assert_eq!(parse_date(""), None);
    }

    // ==========================================================================
    // European format tests - with proper format parameter
    // ==========================================================================

    #[test]
    fn test_parse_amount_european_format_comma_decimal() {
        // EU format: 1.234,56 (dot=thousands, comma=decimal)
        let result = parse_amount_with_format("1.234,56", NumberFormat::Eu);
        assert_eq!(
            result,
            Some(Decimal::new(123456, 2)),
            "EU format 1.234,56 should parse as 1234.56"
        );
    }

    #[test]
    fn test_parse_amount_european_space_thousands() {
        // EU format with space thousands: 8 019,40 PLN
        let result = parse_amount_with_format("8 019,40 PLN", NumberFormat::EuSpace);
        assert_eq!(
            result,
            Some(Decimal::new(801940, 2)),
            "EU format '8 019,40 PLN' should parse as 8019.40"
        );
    }

    #[test]
    fn test_parse_amount_currency_suffix() {
        // Amount with currency suffix (common in EU exports)
        // Now works with any format because currency stripping is format-independent
        let result = parse_amount_with_format("100,50 EUR", NumberFormat::Eu);
        assert_eq!(
            result,
            Some(Decimal::new(10050, 2)),
            "Amount with EUR suffix should parse correctly"
        );
    }

    #[test]
    fn test_parse_amount_us_with_currency_suffix() {
        // US format with currency suffix
        let result = parse_amount("100.50 USD");
        assert_eq!(
            result,
            Some(Decimal::new(10050, 2)),
            "US format with USD suffix should parse correctly"
        );
    }

    #[test]
    fn test_number_format_from_str() {
        assert_eq!(NumberFormat::from_str("us"), NumberFormat::Us);
        assert_eq!(NumberFormat::from_str("eu"), NumberFormat::Eu);
        assert_eq!(NumberFormat::from_str("eu_space"), NumberFormat::EuSpace);
        assert_eq!(NumberFormat::from_str("unknown"), NumberFormat::Us); // default
    }
}
