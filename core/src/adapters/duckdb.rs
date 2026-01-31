//! DuckDB repository implementation

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fs2::FileExt;

use anyhow::{anyhow, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use duckdb::{params, Connection};
use rust_decimal::Decimal;
use sqlparser::dialect::DuckDbDialect;
use sqlparser::parser::Parser;
use uuid::Uuid;

use crate::domain::{Account, AutoTagRule, BalanceSnapshot, Transaction};
use crate::services::MigrationService;

/// Validate SQL syntax before execution to catch malformed queries early.
/// This prevents crashes from malformed SQL reaching the database engine.
fn validate_sql_syntax(sql: &str) -> Result<()> {
    // Skip validation for DuckDB-specific commands that sqlparser doesn't recognize.
    // These are valid DuckDB commands but the sqlparser DuckDbDialect doesn't parse them.
    let first_word = sql.trim().split_whitespace().next().unwrap_or("");
    if first_word.eq_ignore_ascii_case("CHECKPOINT") || first_word.eq_ignore_ascii_case("VACUUM") {
        return Ok(());
    }

    let dialect = DuckDbDialect {};
    Parser::parse_sql(&dialect, sql).map_err(|e| {
        // Clean up the error message - remove redundant prefix
        // Tauri will add "Failed to execute query:" wrapper
        let msg = e.to_string();
        let cleaned = msg.trim_start_matches("sql parser error: ");
        anyhow!("{}", cleaned)
    })?;
    Ok(())
}

/// DuckDB repository implementation
///
/// Uses a filesystem lock to prevent concurrent access from multiple processes
/// (app, CLI, etc.). The lock is held for the lifetime of the repository.
pub struct DuckDbRepository {
    conn: Mutex<Connection>,
    db_path: PathBuf,
    encryption_key: Option<String>,
    /// Filesystem lock file - held for the lifetime of the repository.
    /// The lock is released when this file is dropped.
    _lock_file: File,
}

impl DuckDbRepository {
    /// Create a new DuckDB repository
    ///
    /// For encrypted databases, uses DuckDB's ATTACH with ENCRYPTION_KEY.
    /// The key should be the hex-encoded derived key from Argon2.
    ///
    /// Uses a filesystem lock to prevent concurrent access from multiple processes
    /// or threads. The lock is held for the lifetime of the repository.
    pub fn new(db_path: &Path, encryption_key: Option<&str>) -> Result<Self> {
        // Acquire filesystem lock first
        let lock_file = Self::acquire_file_lock(db_path)?;

        // Open DuckDB connection
        let conn = Self::try_open_connection(db_path, encryption_key)?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path: db_path.to_path_buf(),
            encryption_key: encryption_key.map(|k| k.to_string()),
            _lock_file: lock_file,
        })
    }

    /// Acquire a filesystem lock for the database.
    ///
    /// This prevents concurrent access from multiple processes (app, CLI, etc.).
    /// Uses a blocking lock - if another process holds the lock, this will wait
    /// until it's released. The OS kernel manages the queue of waiters.
    fn acquire_file_lock(db_path: &Path) -> Result<File> {
        let lock_path = db_path.with_extension("duckdb.lock");

        // Ensure parent directory exists
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Open or create the lock file
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| anyhow!("Failed to open lock file {}: {}", lock_path.display(), e))?;

        // Acquire exclusive lock (blocks until available)
        // The OS kernel queues waiters and wakes them in order when lock is released
        lock_file
            .lock_exclusive()
            .map_err(|e| anyhow!("Failed to acquire database lock: {}", e))?;

        Ok(lock_file)
    }

    /// Attempt to open a database connection (called by new() with retry logic)
    fn try_open_connection(db_path: &Path, encryption_key: Option<&str>) -> Result<Connection> {
        // IMPORTANT: Disable extension autoloading to avoid macOS code signing issues
        // (cached extensions in ~/.duckdb/extensions may have different Team IDs)
        let conn = if let Some(key) = encryption_key {
            // Encrypted database: open in-memory first, then ATTACH encrypted file
            let config = duckdb::Config::default().enable_autoload_extension(false)?;
            let conn = Connection::open_in_memory_with_flags(config)?;
            conn.execute(
                &format!(
                    "ATTACH '{}' AS main_db (ENCRYPTION_KEY '{}')",
                    db_path.display(),
                    key
                ),
                [],
            )?;
            conn.execute("USE main_db", [])?;
            conn
        } else {
            let config = duckdb::Config::default().enable_autoload_extension(false)?;
            Connection::open_with_flags(db_path, config)?
        };

        // Note: JSON extension is statically linked via Cargo feature "json"
        // No LOAD required - it's compiled into DuckDB
        // ICU is NOT included - all date functions use Rust-computed dates

        Ok(conn)
    }

    /// Run database migrations using the MigrationService
    ///
    /// Returns the migration result showing what was applied.
    pub fn run_migrations(&self) -> Result<crate::services::MigrationResult> {
        let conn = self.conn.lock().unwrap();
        let migration_service = MigrationService::new(&conn);
        migration_service.run_pending()
    }

    /// Ensure database schema exists (runs pending migrations)
    pub fn ensure_schema(&self) -> Result<()> {
        self.run_migrations()?;
        Ok(())
    }

    /// Force a checkpoint to flush WAL to the main database file.
    ///
    /// This should be called before any operation that reads the raw database file
    /// (like backups) to ensure data consistency. Without this, data in the WAL
    /// file may not be included in the backup.
    pub fn checkpoint(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("CHECKPOINT")?;
        Ok(())
    }

    /// Get the path to the database file
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    // === Account operations ===

    pub fn get_accounts(&self) -> Result<Vec<Account>> {
        let conn = self.conn.lock().unwrap();
        // Join with balance_snapshots to get the latest balance for each account
        let mut stmt = conn.prepare(
            "SELECT a.account_id, a.name, a.nickname, a.account_type, a.currency,
                    a.external_ids, a.institution_name, a.institution_url, a.institution_domain,
                    a.created_at, a.updated_at,
                    (SELECT balance FROM sys_balance_snapshots bs
                     WHERE bs.account_id = a.account_id
                     ORDER BY bs.snapshot_time DESC LIMIT 1) as latest_balance,
                    a.classification, a.is_manual,
                    a.sf_id, a.sf_name, a.sf_currency, a.sf_balance, a.sf_available_balance,
                    a.sf_balance_date, a.sf_org_name, a.sf_org_url, a.sf_org_domain, a.sf_extra,
                    a.lf_id, a.lf_name, a.lf_institution_name, a.lf_institution_logo,
                    a.lf_provider, a.lf_currency, a.lf_status
             FROM sys_accounts a",
        )?;

        let accounts = stmt
            .query_map([], |row| self.row_to_account(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(accounts)
    }

    pub fn get_account_by_id(&self, id: &str) -> Result<Option<Account>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT a.account_id, a.name, a.nickname, a.account_type, a.currency,
                    a.external_ids, a.institution_name, a.institution_url, a.institution_domain,
                    a.created_at, a.updated_at,
                    (SELECT balance FROM sys_balance_snapshots bs
                     WHERE bs.account_id = a.account_id
                     ORDER BY bs.snapshot_time DESC LIMIT 1) as latest_balance,
                    a.classification, a.is_manual,
                    a.sf_id, a.sf_name, a.sf_currency, a.sf_balance, a.sf_available_balance,
                    a.sf_balance_date, a.sf_org_name, a.sf_org_url, a.sf_org_domain, a.sf_extra,
                    a.lf_id, a.lf_name, a.lf_institution_name, a.lf_institution_logo,
                    a.lf_provider, a.lf_currency, a.lf_status
             FROM sys_accounts a WHERE a.account_id = ?",
        )?;

        let account = stmt.query_row([id], |row| self.row_to_account(row)).ok();

        Ok(account)
    }

    fn row_to_account(&self, row: &duckdb::Row) -> std::result::Result<Account, duckdb::Error> {
        // Column indices from SELECT:
        // 0: account_id, 1: name, 2: nickname, 3: account_type, 4: currency,
        // 5: external_ids, 6: institution_name, 7: institution_url, 8: institution_domain,
        // 9: created_at, 10: updated_at, 11: latest_balance, 12: classification, 13: is_manual,
        // 14: sf_id, 15: sf_name, 16: sf_currency, 17: sf_balance, 18: sf_available_balance,
        // 19: sf_balance_date, 20: sf_org_name, 21: sf_org_url, 22: sf_org_domain, 23: sf_extra,
        // 24: lf_id, 25: lf_name, 26: lf_institution_name, 27: lf_institution_logo,
        // 28: lf_provider, 29: lf_currency, 30: lf_status
        let id_str: String = row.get(0)?;
        // Note: column 5 (external_ids) is read but not used - kept for backwards compat
        let created_str: String = row.get(9).unwrap_or_default();
        let updated_str: String = row.get(10).unwrap_or_default();
        let sf_extra_json: Option<String> = row.get(23).ok();

        // Parse UUID - if this fails, skip the row rather than creating a new UUID
        let id = Uuid::parse_str(&id_str).map_err(|e| {
            duckdb::Error::FromSqlConversionFailure(0, duckdb::types::Type::Text, Box::new(e))
        })?;

        Ok(Account {
            id,
            name: row.get(1).unwrap_or_default(),
            nickname: row.get(2).ok(),
            account_type: row.get::<_, Option<String>>(3).ok().flatten(),
            classification: row.get::<_, Option<String>>(12).ok().flatten(),
            currency: row.get(4).unwrap_or_else(|_| "USD".to_string()),
            institution_name: row.get(6).ok(),
            institution_url: row.get(7).ok(),
            institution_domain: row.get(8).ok(),
            created_at: parse_timestamp(&created_str),
            updated_at: parse_timestamp(&updated_str),
            // Balance from latest balance snapshot (column 11)
            balance: row
                .get::<_, Option<f64>>(11)
                .ok()
                .flatten()
                .map(|f| Decimal::try_from(f).unwrap_or_default()),
            // Manual flag (column 13)
            is_manual: row
                .get::<_, Option<bool>>(13)
                .ok()
                .flatten()
                .unwrap_or(false),
            // SimpleFIN fields (columns 14-23)
            sf_id: row.get(14).ok(),
            sf_name: row.get(15).ok(),
            sf_currency: row.get(16).ok(),
            sf_balance: row.get(17).ok(),
            sf_available_balance: row.get(18).ok(),
            sf_balance_date: row.get(19).ok(),
            sf_org_name: row.get(20).ok(),
            sf_org_url: row.get(21).ok(),
            sf_org_domain: row.get(22).ok(),
            sf_extra: sf_extra_json.and_then(|s| serde_json::from_str(&s).ok()),
            // Lunchflow fields (columns 24-30)
            lf_id: row.get(24).ok(),
            lf_name: row.get(25).ok(),
            lf_institution_name: row.get(26).ok(),
            lf_institution_logo: row.get(27).ok(),
            lf_provider: row.get(28).ok(),
            lf_currency: row.get(29).ok(),
            lf_status: row.get(30).ok(),
        })
    }

    pub fn upsert_account(&self, account: &Account) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Write empty JSON for external_ids - kept for backwards compat with DB schema
        let external_ids = "{}";
        let sf_extra = account.sf_extra.as_ref().map(|v| v.to_string());

        // Use COALESCE to preserve user-edited values like Python CLI does
        // Note: balance is stored in balance_snapshots, not in accounts table (matching Python schema)
        // Classification is preserved on sync - we only set it if the user hasn't already set one
        conn.execute(
            "INSERT INTO sys_accounts (account_id, name, nickname, account_type, classification, currency,
                                       external_ids, institution_name, institution_url, institution_domain,
                                       created_at, updated_at, is_manual,
                                       sf_id, sf_name, sf_currency, sf_balance, sf_available_balance,
                                       sf_balance_date, sf_org_name, sf_org_url, sf_org_domain, sf_extra,
                                       lf_id, lf_name, lf_institution_name, lf_institution_logo,
                                       lf_provider, lf_currency, lf_status)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (account_id) DO UPDATE SET
                name = EXCLUDED.name,
                nickname = COALESCE(sys_accounts.nickname, EXCLUDED.nickname),
                account_type = COALESCE(sys_accounts.account_type, EXCLUDED.account_type),
                classification = COALESCE(sys_accounts.classification, EXCLUDED.classification),
                currency = EXCLUDED.currency,
                external_ids = EXCLUDED.external_ids,
                institution_name = COALESCE(EXCLUDED.institution_name, sys_accounts.institution_name),
                institution_url = COALESCE(EXCLUDED.institution_url, sys_accounts.institution_url),
                institution_domain = COALESCE(EXCLUDED.institution_domain, sys_accounts.institution_domain),
                updated_at = EXCLUDED.updated_at,
                is_manual = COALESCE(sys_accounts.is_manual, EXCLUDED.is_manual),
                sf_id = COALESCE(EXCLUDED.sf_id, sys_accounts.sf_id),
                sf_name = COALESCE(EXCLUDED.sf_name, sys_accounts.sf_name),
                sf_currency = COALESCE(EXCLUDED.sf_currency, sys_accounts.sf_currency),
                sf_balance = COALESCE(EXCLUDED.sf_balance, sys_accounts.sf_balance),
                sf_available_balance = COALESCE(EXCLUDED.sf_available_balance, sys_accounts.sf_available_balance),
                sf_balance_date = COALESCE(EXCLUDED.sf_balance_date, sys_accounts.sf_balance_date),
                sf_org_name = COALESCE(EXCLUDED.sf_org_name, sys_accounts.sf_org_name),
                sf_org_url = COALESCE(EXCLUDED.sf_org_url, sys_accounts.sf_org_url),
                sf_org_domain = COALESCE(EXCLUDED.sf_org_domain, sys_accounts.sf_org_domain),
                sf_extra = COALESCE(EXCLUDED.sf_extra, sys_accounts.sf_extra),
                lf_id = COALESCE(EXCLUDED.lf_id, sys_accounts.lf_id),
                lf_name = COALESCE(EXCLUDED.lf_name, sys_accounts.lf_name),
                lf_institution_name = COALESCE(EXCLUDED.lf_institution_name, sys_accounts.lf_institution_name),
                lf_institution_logo = COALESCE(EXCLUDED.lf_institution_logo, sys_accounts.lf_institution_logo),
                lf_provider = COALESCE(EXCLUDED.lf_provider, sys_accounts.lf_provider),
                lf_currency = COALESCE(EXCLUDED.lf_currency, sys_accounts.lf_currency),
                lf_status = COALESCE(EXCLUDED.lf_status, sys_accounts.lf_status)",
            params![
                account.id.to_string(),
                account.name,
                account.nickname,
                account.account_type.as_ref().map(|t| t.to_string()),
                account.classification.as_ref().map(|c| c.to_string()),
                account.currency,
                external_ids,
                account.institution_name,
                account.institution_url,
                account.institution_domain,
                account.created_at.to_rfc3339(),
                account.updated_at.to_rfc3339(),
                account.is_manual,
                account.sf_id,
                account.sf_name,
                account.sf_currency,
                account.sf_balance,
                account.sf_available_balance,
                account.sf_balance_date,
                account.sf_org_name,
                account.sf_org_url,
                account.sf_org_domain,
                sf_extra,
                account.lf_id,
                account.lf_name,
                account.lf_institution_name,
                account.lf_institution_logo,
                account.lf_provider,
                account.lf_currency,
                account.lf_status,
            ],
        )?;

        Ok(())
    }

    /// Delete an account and all associated data (transactions, balance snapshots)
    ///
    /// This performs a cascade delete:
    /// 1. Delete all transactions for the account
    /// 2. Delete all balance snapshots for the account
    /// 3. Delete the account itself
    ///
    /// Note: We intentionally don't wrap this in an explicit transaction because
    /// DuckDB validates foreign key constraints at statement-level, not at commit
    /// time. Within a transaction, the DELETE from sys_accounts fails FK validation
    /// because it doesn't "see" the earlier deletes of child records.
    ///
    /// Data consistency is maintained by:
    /// 1. Filesystem lock preventing concurrent access
    /// 2. Delete order respecting FK constraints (children before parent)
    /// 3. Each statement auto-committing on success
    pub fn delete_account(&self, account_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Delete in order to respect foreign key constraints:
        // transactions and snapshots reference accounts, so delete them first

        // 1. Delete all transactions (including soft-deleted ones)
        conn.execute(
            "DELETE FROM sys_transactions WHERE account_id = ?",
            params![account_id],
        )?;

        // 2. Delete all balance snapshots
        conn.execute(
            "DELETE FROM sys_balance_snapshots WHERE account_id = ?",
            params![account_id],
        )?;

        // 3. Delete the account
        conn.execute(
            "DELETE FROM sys_accounts WHERE account_id = ?",
            params![account_id],
        )?;

        Ok(())
    }

    // === Transaction operations ===

    pub fn get_transactions(&self) -> Result<Vec<Transaction>> {
        let conn = self.conn.lock().unwrap();
        // Note: CAST(tags AS VARCHAR) is required because duckdb-rs cannot read VARCHAR[]
        // directly as String. Without the CAST, row.get() silently fails and returns "[]".
        // See parse_duckdb_array() for the parsing logic.
        let mut stmt = conn.prepare(
            "SELECT transaction_id, account_id, amount, description, transaction_date::VARCHAR,
                    posted_date::VARCHAR, CAST(tags AS VARCHAR) as tags, external_ids, deleted_at, parent_transaction_id,
                    created_at, updated_at, csv_fingerprint, csv_batch_id, is_manual, tags_auto_applied,
                    sf_id, sf_posted, sf_amount, sf_description, sf_transacted_at, sf_pending, sf_extra,
                    lf_id, lf_account_id, lf_amount, lf_currency, lf_date::VARCHAR, lf_merchant, lf_description, lf_is_pending
             FROM sys_transactions
             WHERE deleted_at IS NULL"
        )?;

        let transactions = stmt
            .query_map([], |row| self.row_to_transaction(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(transactions)
    }

    /// Get transactions for a specific account, ordered by transaction_date DESC
    pub fn get_transactions_by_account(&self, account_id: &str) -> Result<Vec<Transaction>> {
        let conn = self.conn.lock().unwrap();
        // CAST(tags AS VARCHAR) required - see get_transactions() for explanation
        let mut stmt = conn.prepare(
            "SELECT transaction_id, account_id, amount, description, transaction_date::VARCHAR,
                    posted_date::VARCHAR, CAST(tags AS VARCHAR) as tags, external_ids, deleted_at, parent_transaction_id,
                    created_at, updated_at, csv_fingerprint, csv_batch_id, is_manual, tags_auto_applied,
                    sf_id, sf_posted, sf_amount, sf_description, sf_transacted_at, sf_pending, sf_extra,
                    lf_id, lf_account_id, lf_amount, lf_currency, lf_date::VARCHAR, lf_merchant, lf_description, lf_is_pending
             FROM sys_transactions
             WHERE account_id = ? AND deleted_at IS NULL
             ORDER BY transaction_date DESC"
        )?;

        let transactions = stmt
            .query_map([account_id], |row| self.row_to_transaction(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(transactions)
    }

    pub fn get_transaction_count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sys_transactions WHERE deleted_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get the maximum transaction date from the database
    pub fn get_max_transaction_date(&self) -> Result<Option<NaiveDate>> {
        let conn = self.conn.lock().unwrap();
        let result: Option<String> = conn.query_row(
            "SELECT MAX(transaction_date)::VARCHAR FROM sys_transactions WHERE deleted_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(result.map(|s| parse_date(&s)))
    }

    pub fn get_balance_snapshot_count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM sys_balance_snapshots", [], |row| {
                row.get(0)
            })?;
        Ok(count)
    }

    pub fn get_transaction_date_range(&self) -> Result<crate::services::DateRange> {
        let conn = self.conn.lock().unwrap();
        let result: (Option<String>, Option<String>) = conn.query_row(
            "SELECT
                MIN(transaction_date)::VARCHAR,
                MAX(transaction_date)::VARCHAR
             FROM sys_transactions
             WHERE deleted_at IS NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(crate::services::DateRange {
            earliest: result.0,
            latest: result.1,
        })
    }

    fn row_to_transaction(
        &self,
        row: &duckdb::Row,
    ) -> std::result::Result<Transaction, duckdb::Error> {
        // Column indices from SELECT:
        // 0: transaction_id, 1: account_id, 2: amount, 3: description, 4: transaction_date,
        // 5: posted_date, 6: tags, 7: external_ids, 8: deleted_at, 9: parent_transaction_id,
        // 10: created_at, 11: updated_at, 12: csv_fingerprint, 13: csv_batch_id, 14: is_manual, 15: tags_auto_applied,
        // 16: sf_id, 17: sf_posted, 18: sf_amount, 19: sf_description, 20: sf_transacted_at, 21: sf_pending, 22: sf_extra,
        // 23: lf_id, 24: lf_account_id, 25: lf_amount, 26: lf_currency, 27: lf_date, 28: lf_merchant, 29: lf_description, 30: lf_is_pending
        let id_str: String = row.get(0)?;
        let account_id_str: String = row.get(1)?;
        let amount: f64 = row.get(2).unwrap_or(0.0);
        let tx_date_str: String = row.get(4).unwrap_or_default();
        let posted_date_str: String = row.get(5).unwrap_or_default();

        // Tags are stored as VARCHAR[] - DuckDB Rust binding returns them as a string
        // Parse the DuckDB array format: [tag1, tag2] or ['tag1', 'tag2']
        let tags_str: String = row.get(6).unwrap_or_else(|_| "[]".to_string());
        let tags = parse_duckdb_array(&tags_str);

        // Note: column 7 (external_ids) is in the query but not used - kept for backwards compat
        let parent_id_str: Option<String> = row.get(9).ok();
        let created_str: String = row.get(10).unwrap_or_default();
        let updated_str: String = row.get(11).unwrap_or_default();
        let sf_extra_json: Option<String> = row.get(22).ok();
        let lf_date_str: Option<String> = row.get(27).ok();

        // Parse UUIDs - if these fail, skip the row rather than creating new UUIDs
        let id = Uuid::parse_str(&id_str).map_err(|e| {
            duckdb::Error::FromSqlConversionFailure(0, duckdb::types::Type::Text, Box::new(e))
        })?;
        let account_id = Uuid::parse_str(&account_id_str).map_err(|e| {
            duckdb::Error::FromSqlConversionFailure(1, duckdb::types::Type::Text, Box::new(e))
        })?;

        // Parse amount - if conversion fails, this is a data integrity issue
        let amount = Decimal::try_from(amount).map_err(|e| {
            duckdb::Error::FromSqlConversionFailure(2, duckdb::types::Type::Text, Box::new(e))
        })?;

        Ok(Transaction {
            id,
            account_id,
            amount,
            description: row.get(3).ok(),
            transaction_date: parse_date(&tx_date_str),
            posted_date: parse_date(&posted_date_str),
            tags,
            deleted_at: None,
            parent_transaction_id: parent_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
            created_at: parse_timestamp(&created_str),
            updated_at: parse_timestamp(&updated_str),
            // CSV Import tracking (columns 12-13)
            csv_fingerprint: row.get(12).ok(),
            csv_batch_id: row.get(13).ok(),
            // Manual flag (column 14)
            is_manual: row
                .get::<_, Option<bool>>(14)
                .ok()
                .flatten()
                .unwrap_or(false),
            // Auto-tag tracking (column 15)
            tags_auto_applied: row
                .get::<_, Option<bool>>(15)
                .ok()
                .flatten()
                .unwrap_or(false),
            // SimpleFIN fields (columns 16-22)
            sf_id: row.get(16).ok(),
            sf_posted: row.get(17).ok(),
            sf_amount: row.get(18).ok(),
            sf_description: row.get(19).ok(),
            sf_transacted_at: row.get(20).ok(),
            sf_pending: row.get(21).ok(),
            sf_extra: sf_extra_json.and_then(|s| serde_json::from_str(&s).ok()),
            // Lunchflow fields (columns 23-30)
            lf_id: row.get(23).ok(),
            lf_account_id: row.get(24).ok(),
            lf_amount: row
                .get::<_, Option<f64>>(25)
                .ok()
                .flatten()
                .map(|f| Decimal::try_from(f).unwrap_or_default()),
            lf_currency: row.get(26).ok(),
            lf_date: lf_date_str.map(|s| parse_date(&s)),
            lf_merchant: row.get(28).ok(),
            lf_description: row.get(29).ok(),
            lf_is_pending: row.get(30).ok(),
        })
    }

    pub fn upsert_transaction(&self, tx: &Transaction) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Write empty JSON for external_ids - kept for backwards compat with DB schema
        let external_ids = "{}";
        let sf_extra = tx.sf_extra.as_ref().map(|v| v.to_string());

        // Build tags array literal for DuckDB: ['tag1', 'tag2']
        let tags_literal = format_tags_array(&tx.tags);

        // Use raw SQL with array literal since DuckDB Rust binding doesn't support array params well
        let sql = format!(
            "INSERT INTO sys_transactions (transaction_id, account_id, amount, description,
                                           transaction_date, posted_date, tags, external_ids,
                                           parent_transaction_id, created_at, updated_at,
                                           csv_fingerprint, csv_batch_id, is_manual, tags_auto_applied,
                                           sf_id, sf_posted, sf_amount, sf_description, sf_transacted_at, sf_pending, sf_extra,
                                           lf_id, lf_account_id, lf_amount, lf_currency, lf_date, lf_merchant, lf_description, lf_is_pending)
             VALUES (?, ?, ?, ?, ?, ?, {}, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (transaction_id) DO UPDATE SET
                account_id = EXCLUDED.account_id,
                amount = EXCLUDED.amount,
                description = EXCLUDED.description,
                transaction_date = EXCLUDED.transaction_date,
                posted_date = EXCLUDED.posted_date,
                tags = EXCLUDED.tags,
                external_ids = EXCLUDED.external_ids,
                parent_transaction_id = EXCLUDED.parent_transaction_id,
                updated_at = EXCLUDED.updated_at,
                csv_fingerprint = COALESCE(EXCLUDED.csv_fingerprint, sys_transactions.csv_fingerprint),
                csv_batch_id = COALESCE(EXCLUDED.csv_batch_id, sys_transactions.csv_batch_id),
                is_manual = COALESCE(sys_transactions.is_manual, EXCLUDED.is_manual),
                tags_auto_applied = COALESCE(sys_transactions.tags_auto_applied, EXCLUDED.tags_auto_applied),
                sf_id = COALESCE(EXCLUDED.sf_id, sys_transactions.sf_id),
                sf_posted = COALESCE(EXCLUDED.sf_posted, sys_transactions.sf_posted),
                sf_amount = COALESCE(EXCLUDED.sf_amount, sys_transactions.sf_amount),
                sf_description = COALESCE(EXCLUDED.sf_description, sys_transactions.sf_description),
                sf_transacted_at = COALESCE(EXCLUDED.sf_transacted_at, sys_transactions.sf_transacted_at),
                sf_pending = COALESCE(EXCLUDED.sf_pending, sys_transactions.sf_pending),
                sf_extra = COALESCE(EXCLUDED.sf_extra, sys_transactions.sf_extra),
                lf_id = COALESCE(EXCLUDED.lf_id, sys_transactions.lf_id),
                lf_account_id = COALESCE(EXCLUDED.lf_account_id, sys_transactions.lf_account_id),
                lf_amount = COALESCE(EXCLUDED.lf_amount, sys_transactions.lf_amount),
                lf_currency = COALESCE(EXCLUDED.lf_currency, sys_transactions.lf_currency),
                lf_date = COALESCE(EXCLUDED.lf_date, sys_transactions.lf_date),
                lf_merchant = COALESCE(EXCLUDED.lf_merchant, sys_transactions.lf_merchant),
                lf_description = COALESCE(EXCLUDED.lf_description, sys_transactions.lf_description),
                lf_is_pending = COALESCE(EXCLUDED.lf_is_pending, sys_transactions.lf_is_pending)",
            tags_literal
        );

        conn.execute(
            &sql,
            params![
                tx.id.to_string(),
                tx.account_id.to_string(),
                tx.amount.to_string().parse::<f64>().unwrap_or(0.0),
                tx.description,
                tx.transaction_date.to_string(),
                tx.posted_date.to_string(),
                external_ids,
                tx.parent_transaction_id.map(|id| id.to_string()),
                tx.created_at.to_rfc3339(),
                tx.updated_at.to_rfc3339(),
                tx.csv_fingerprint,
                tx.csv_batch_id,
                tx.is_manual,
                tx.tags_auto_applied,
                tx.sf_id,
                tx.sf_posted,
                tx.sf_amount,
                tx.sf_description,
                tx.sf_transacted_at,
                tx.sf_pending,
                sf_extra,
                tx.lf_id,
                tx.lf_account_id,
                tx.lf_amount
                    .map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
                tx.lf_currency,
                tx.lf_date.map(|d| d.to_string()),
                tx.lf_merchant,
                tx.lf_description,
                tx.lf_is_pending,
            ],
        )?;

        Ok(())
    }

    pub fn update_transaction_tags(&self, tx_id: &str, tags: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tags_literal = format_tags_array(tags);
        let sql = format!(
            "UPDATE sys_transactions SET tags = {}, updated_at = CURRENT_TIMESTAMP WHERE transaction_id = ?",
            tags_literal
        );
        conn.execute(&sql, params![tx_id])?;
        Ok(())
    }

    /// Update transaction tags and mark them as auto-applied (by rules)
    pub fn update_transaction_tags_auto(&self, tx_id: &str, tags: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tags_literal = format_tags_array(tags);
        let sql = format!(
            "UPDATE sys_transactions SET tags = {}, tags_auto_applied = TRUE, updated_at = CURRENT_TIMESTAMP WHERE transaction_id = ?",
            tags_literal
        );
        conn.execute(&sql, params![tx_id])?;
        Ok(())
    }

    /// Insert a transaction only if it doesn't already exist (skip existing to preserve user edits)
    /// Returns true if inserted, false if skipped
    pub fn insert_transaction_if_not_exists(&self, tx: &Transaction) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        // Write empty JSON for external_ids - kept for backwards compat with DB schema
        let external_ids = "{}";
        let sf_extra = tx.sf_extra.as_ref().map(|v| v.to_string());
        let tags_literal = format_tags_array(&tx.tags);

        // Use INSERT ... ON CONFLICT DO NOTHING to skip existing transactions
        let sql = format!(
            "INSERT INTO sys_transactions (transaction_id, account_id, amount, description,
                                           transaction_date, posted_date, tags, external_ids,
                                           parent_transaction_id, created_at, updated_at,
                                           csv_fingerprint, csv_batch_id, is_manual, tags_auto_applied,
                                           sf_id, sf_posted, sf_amount, sf_description, sf_transacted_at, sf_pending, sf_extra,
                                           lf_id, lf_account_id, lf_amount, lf_currency, lf_date, lf_merchant, lf_description, lf_is_pending)
             VALUES (?, ?, ?, ?, ?, ?, {}, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (transaction_id) DO NOTHING",
            tags_literal
        );

        let rows_changed = conn.execute(
            &sql,
            params![
                tx.id.to_string(),
                tx.account_id.to_string(),
                tx.amount.to_string().parse::<f64>().unwrap_or(0.0),
                tx.description,
                tx.transaction_date.to_string(),
                tx.posted_date.to_string(),
                external_ids,
                tx.parent_transaction_id.map(|id| id.to_string()),
                tx.created_at.to_rfc3339(),
                tx.updated_at.to_rfc3339(),
                tx.csv_fingerprint,
                tx.csv_batch_id,
                tx.is_manual,
                tx.tags_auto_applied,
                tx.sf_id,
                tx.sf_posted,
                tx.sf_amount,
                tx.sf_description,
                tx.sf_transacted_at,
                tx.sf_pending,
                sf_extra,
                tx.lf_id,
                tx.lf_account_id,
                tx.lf_amount
                    .map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
                tx.lf_currency,
                tx.lf_date.map(|d| d.to_string()),
                tx.lf_merchant,
                tx.lf_description,
                tx.lf_is_pending,
            ],
        )?;

        Ok(rows_changed > 0)
    }

    /// Check if a transaction exists by ID
    pub fn transaction_exists(&self, tx_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sys_transactions WHERE transaction_id = ?",
            params![tx_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Check if a transaction exists by SimpleFIN ID (indexed, fast)
    pub fn transaction_exists_by_sf_id(&self, sf_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sys_transactions WHERE sf_id = ?",
            params![sf_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Check if a transaction exists by Lunchflow ID (indexed, fast)
    pub fn transaction_exists_by_lf_id(&self, lf_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sys_transactions WHERE lf_id = ?",
            params![lf_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Check if a CSV fingerprint exists in batches other than the current one
    /// This allows duplicate transactions within a single import batch but prevents re-import
    pub fn csv_fingerprint_exists_in_other_batches(
        &self,
        fingerprint: &str,
        current_batch_id: &str,
    ) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sys_transactions WHERE csv_fingerprint = ? AND (csv_batch_id IS NULL OR csv_batch_id != ?)",
            params![fingerprint, current_batch_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn get_transaction_by_id(&self, id: &str) -> Result<Option<Transaction>> {
        let conn = self.conn.lock().unwrap();
        // CAST(tags AS VARCHAR) required - see get_transactions() for explanation
        let mut stmt = conn.prepare(
            "SELECT transaction_id, account_id, amount, description, transaction_date::VARCHAR,
                    posted_date::VARCHAR, CAST(tags AS VARCHAR) as tags, external_ids, deleted_at, parent_transaction_id,
                    created_at, updated_at, csv_fingerprint, csv_batch_id, is_manual, tags_auto_applied,
                    sf_id, sf_posted, sf_amount, sf_description, sf_transacted_at, sf_pending, sf_extra,
                    lf_id, lf_account_id, lf_amount, lf_currency, lf_date::VARCHAR, lf_merchant, lf_description, lf_is_pending
             FROM sys_transactions WHERE transaction_id = ?"
        )?;

        let tx = stmt
            .query_row([id], |row| self.row_to_transaction(row))
            .ok();

        Ok(tx)
    }

    // === Balance snapshot operations ===

    pub fn add_balance_snapshot(&self, snapshot: &BalanceSnapshot) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sys_balance_snapshots (snapshot_id, account_id, balance, snapshot_time, source, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                snapshot.id.to_string(),
                snapshot.account_id.to_string(),
                snapshot.balance.to_string().parse::<f64>().unwrap_or(0.0),
                snapshot.snapshot_time.to_string(),
                snapshot.source.as_ref().map(|s| s.to_string()),
                snapshot.created_at.to_rfc3339(),
                snapshot.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_balance_snapshots(&self, account_id: Option<&str>) -> Result<Vec<BalanceSnapshot>> {
        let conn = self.conn.lock().unwrap();
        // Cast TIMESTAMP and balance columns to VARCHAR so they can be read as strings with full precision
        let sql = if account_id.is_some() {
            "SELECT snapshot_id, account_id, balance::VARCHAR, snapshot_time::VARCHAR, source, created_at::VARCHAR, updated_at::VARCHAR
             FROM sys_balance_snapshots WHERE account_id = ? ORDER BY snapshot_time DESC"
        } else {
            "SELECT snapshot_id, account_id, balance::VARCHAR, snapshot_time::VARCHAR, source, created_at::VARCHAR, updated_at::VARCHAR
             FROM sys_balance_snapshots ORDER BY snapshot_time DESC"
        };

        let mut stmt = conn.prepare(sql)?;

        let snapshots = if let Some(aid) = account_id {
            stmt.query_map([aid], |row| Ok(self.row_to_balance_snapshot(row)))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map([], |row| Ok(self.row_to_balance_snapshot(row)))?
                .filter_map(|r| r.ok())
                .collect()
        };

        Ok(snapshots)
    }

    pub fn update_balance_snapshot(
        &self,
        snapshot_id: &str,
        new_balance: Decimal,
        new_source: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sys_balance_snapshots SET balance = ?, source = ?, updated_at = ? WHERE snapshot_id = ?",
            params![
                new_balance.to_string().parse::<f64>().unwrap_or(0.0),
                new_source,
                Utc::now().to_rfc3339(),
                snapshot_id,
            ],
        )?;
        Ok(())
    }

    /// Delete all balance snapshots for an account within a date range
    pub fn delete_balance_snapshots_in_range(
        &self,
        account_id: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        // Delete snapshots where the date part of snapshot_time falls within the range
        let deleted = conn.execute(
            "DELETE FROM sys_balance_snapshots
             WHERE account_id = ?
             AND CAST(snapshot_time AS DATE) >= ?
             AND CAST(snapshot_time AS DATE) <= ?",
            params![account_id, start_date.to_string(), end_date.to_string(),],
        )?;
        Ok(deleted)
    }

    fn row_to_balance_snapshot(&self, row: &duckdb::Row) -> BalanceSnapshot {
        let id_str: String = row.get(0).unwrap_or_default();
        let account_id_str: String = row.get(1).unwrap_or_default();
        let balance_str: String = row.get(2).unwrap_or_default();
        let snapshot_time_str: String = row.get(3).unwrap_or_default();
        let source: Option<String> = row.get(4).ok();
        let created_str: String = row.get(5).unwrap_or_default();
        let updated_str: String = row.get(6).unwrap_or_default();

        BalanceSnapshot {
            id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
            account_id: Uuid::parse_str(&account_id_str).unwrap_or_else(|_| Uuid::new_v4()),
            balance: Decimal::from_str_exact(&balance_str).unwrap_or_default(),
            snapshot_time: parse_naive_datetime(&snapshot_time_str),
            source,
            created_at: parse_timestamp(&created_str),
            updated_at: parse_timestamp(&updated_str),
        }
    }

    // === Query operations ===

    pub fn execute_query(&self, sql: &str) -> Result<QueryResult> {
        // Validate it's a read-only query by checking SQL statement type
        // Only look at the first word after stripping whitespace/comments
        let sql_trimmed = sql.trim();
        let first_word = sql_trimmed
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_uppercase();
        if first_word != "SELECT" && first_word != "WITH" {
            anyhow::bail!("Only SELECT queries are allowed");
        }

        // Also block dangerous operations even in subqueries
        let sql_upper = sql.to_uppercase();
        // Use word boundaries to avoid false positives (deleted_at vs DELETE)
        let dangerous_patterns = [
            " INSERT ",
            " UPDATE ",
            " DROP ",
            " CREATE ",
            " ALTER ",
            " TRUNCATE ",
            "\nINSERT ",
            "\nUPDATE ",
            "\nDROP ",
            "\nCREATE ",
            "\nALTER ",
            "\nTRUNCATE ",
            "(INSERT ",
            "(UPDATE ",
            "(DROP ",
            "(CREATE ",
            "(ALTER ",
            "(TRUNCATE ",
        ];
        for pattern in dangerous_patterns {
            if sql_upper.contains(pattern) {
                anyhow::bail!("Only SELECT queries are allowed");
            }
        }

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(sql)?;

        // Execute query and iterate
        let mut result_rows = stmt.query([])?;

        // Collect all rows first
        let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
        let mut column_count = 0;

        while let Some(row) = result_rows.next()? {
            // Get column count from the first row
            if rows.is_empty() {
                column_count = row.as_ref().column_count();
            }

            let mut row_values: Vec<serde_json::Value> = Vec::new();
            for i in 0..column_count {
                let value = self.get_column_value(row, i);
                row_values.push(value);
            }
            rows.push(row_values);
        }

        // Drop result_rows to release borrow on stmt
        drop(result_rows);

        // Now get column names
        let columns: Vec<String> = if column_count > 0 {
            (0..column_count)
                .map(|i| {
                    stmt.column_name(i)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|_| format!("col{}", i))
                })
                .collect()
        } else {
            // No rows, try to get column count from statement
            let count = stmt.column_count();
            (0..count)
                .map(|i| {
                    stmt.column_name(i)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|_| format!("col{}", i))
                })
                .collect()
        };

        let row_count = rows.len();

        Ok(QueryResult {
            columns,
            rows,
            row_count,
        })
    }

    /// Execute arbitrary SQL (read or write)
    ///
    /// Unlike `execute_query`, this method allows both SELECT and write operations.
    /// For SELECT queries, returns columns and rows.
    /// For write queries (INSERT/UPDATE/DELETE), returns affected_rows count.
    pub fn execute_sql(&self, sql: &str) -> Result<QueryResult> {
        // Validate SQL syntax before execution to prevent crashes on malformed queries
        validate_sql_syntax(sql)?;

        let sql_trimmed = sql.trim();
        let first_word = sql_trimmed
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_uppercase();

        let is_select = first_word == "SELECT"
            || first_word == "WITH"
            || first_word == "DESCRIBE"
            || first_word == "SHOW";

        let conn = self.conn.lock().unwrap();

        if is_select {
            // Read query - return columns and rows
            let mut stmt = conn.prepare(sql)?;
            let mut result_rows = stmt.query([])?;

            let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
            let mut column_count = 0;

            while let Some(row) = result_rows.next()? {
                if rows.is_empty() {
                    column_count = row.as_ref().column_count();
                }

                let mut row_values: Vec<serde_json::Value> = Vec::new();
                for i in 0..column_count {
                    let value = self.get_column_value(row, i);
                    row_values.push(value);
                }
                rows.push(row_values);
            }

            drop(result_rows);

            let columns: Vec<String> = if column_count > 0 {
                (0..column_count)
                    .map(|i| {
                        stmt.column_name(i)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|_| format!("col{}", i))
                    })
                    .collect()
            } else {
                let count = stmt.column_count();
                (0..count)
                    .map(|i| {
                        stmt.column_name(i)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|_| format!("col{}", i))
                    })
                    .collect()
            };

            let row_count = rows.len();

            Ok(QueryResult {
                columns,
                rows,
                row_count,
            })
        } else {
            // Write query - return affected rows
            let affected = conn.execute(sql, [])?;

            Ok(QueryResult {
                columns: vec!["affected_rows".to_string()],
                rows: vec![vec![serde_json::json!(affected)]],
                row_count: 1,
            })
        }
    }

    /// Execute parameterized SQL (read or write)
    ///
    /// Parameters are passed as JSON values and bound to ? placeholders.
    pub fn execute_sql_with_params(
        &self,
        sql: &str,
        params: &[serde_json::Value],
    ) -> Result<QueryResult> {
        // Validate SQL syntax before execution to prevent crashes on malformed queries
        validate_sql_syntax(sql)?;

        let sql_trimmed = sql.trim();
        let first_word = sql_trimmed
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_uppercase();

        let is_select = first_word == "SELECT"
            || first_word == "WITH"
            || first_word == "DESCRIBE"
            || first_word == "SHOW";

        let conn = self.conn.lock().unwrap();

        // Convert JSON params to DuckDB params
        let duckdb_params: Vec<Box<dyn duckdb::ToSql>> = params
            .iter()
            .map(|v| Self::json_to_duckdb_param(v))
            .collect();
        let param_refs: Vec<&dyn duckdb::ToSql> =
            duckdb_params.iter().map(|b| b.as_ref()).collect();

        if is_select {
            // Read query - return columns and rows
            let mut stmt = conn.prepare(sql)?;
            let mut result_rows = stmt.query(param_refs.as_slice())?;

            let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
            let mut column_count = 0;

            while let Some(row) = result_rows.next()? {
                if rows.is_empty() {
                    column_count = row.as_ref().column_count();
                }

                let mut row_values: Vec<serde_json::Value> = Vec::new();
                for i in 0..column_count {
                    let value = self.get_column_value(row, i);
                    row_values.push(value);
                }
                rows.push(row_values);
            }

            drop(result_rows);

            let columns: Vec<String> = if column_count > 0 {
                (0..column_count)
                    .map(|i| {
                        stmt.column_name(i)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|_| format!("col{}", i))
                    })
                    .collect()
            } else {
                let count = stmt.column_count();
                (0..count)
                    .map(|i| {
                        stmt.column_name(i)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|_| format!("col{}", i))
                    })
                    .collect()
            };

            let row_count = rows.len();

            Ok(QueryResult {
                columns,
                rows,
                row_count,
            })
        } else {
            // Write query - return affected rows
            let mut stmt = conn.prepare(sql)?;
            let affected = stmt.execute(param_refs.as_slice())?;

            Ok(QueryResult {
                columns: vec!["affected_rows".to_string()],
                rows: vec![vec![serde_json::json!(affected)]],
                row_count: 1,
            })
        }
    }

    /// Convert JSON value to DuckDB parameter
    fn json_to_duckdb_param(value: &serde_json::Value) -> Box<dyn duckdb::ToSql> {
        match value {
            serde_json::Value::Null => Box::new(None::<String>),
            serde_json::Value::Bool(b) => Box::new(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Box::new(i)
                } else if let Some(f) = n.as_f64() {
                    Box::new(f)
                } else {
                    Box::new(n.to_string())
                }
            }
            serde_json::Value::String(s) => Box::new(s.clone()),
            serde_json::Value::Array(arr) => {
                // Convert array to comma-separated string
                let strings: Vec<String> = arr
                    .iter()
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string(),
                    })
                    .collect();
                Box::new(strings.join(","))
            }
            serde_json::Value::Object(_) => {
                // Convert object to JSON string
                Box::new(value.to_string())
            }
        }
    }

    fn get_column_value(&self, row: &duckdb::Row, idx: usize) -> serde_json::Value {
        use duckdb::types::ValueRef;

        // Use get_ref to get the raw ValueRef, which handles all types including arrays
        match row.get_ref(idx) {
            Ok(ValueRef::Null) => serde_json::Value::Null,
            Ok(ValueRef::Boolean(b)) => serde_json::Value::Bool(b),
            Ok(ValueRef::TinyInt(i)) => serde_json::json!(i),
            Ok(ValueRef::SmallInt(i)) => serde_json::json!(i),
            Ok(ValueRef::Int(i)) => serde_json::json!(i),
            Ok(ValueRef::BigInt(i)) => serde_json::json!(i),
            Ok(ValueRef::HugeInt(i)) => serde_json::json!(i.to_string()),
            Ok(ValueRef::UTinyInt(i)) => serde_json::json!(i),
            Ok(ValueRef::USmallInt(i)) => serde_json::json!(i),
            Ok(ValueRef::UInt(i)) => serde_json::json!(i),
            Ok(ValueRef::UBigInt(i)) => serde_json::json!(i),
            Ok(ValueRef::Float(f)) => serde_json::json!(f),
            Ok(ValueRef::Double(f)) => serde_json::json!(f),
            Ok(ValueRef::Decimal(d)) => {
                // Convert Decimal to f64 for JSON compatibility
                // This matches the old Arrow-based behavior
                use std::str::FromStr;
                let s = d.to_string();
                match f64::from_str(&s) {
                    Ok(f) => serde_json::json!(f),
                    Err(_) => serde_json::Value::String(s), // Fallback for very large decimals
                }
            }
            Ok(ValueRef::Text(bytes)) => {
                let s = String::from_utf8_lossy(bytes).to_string();
                serde_json::Value::String(s)
            }
            Ok(ValueRef::Blob(bytes)) => {
                serde_json::Value::String(format!("<blob {} bytes>", bytes.len()))
            }
            Ok(ValueRef::Date32(d)) => {
                // Days since epoch
                let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
                let date = epoch + chrono::Duration::days(d as i64);
                serde_json::Value::String(date.to_string())
            }
            Ok(ValueRef::Timestamp(_, ts)) => {
                // Microseconds since epoch
                let dt = chrono::DateTime::from_timestamp_micros(ts)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| ts.to_string());
                serde_json::Value::String(dt)
            }
            Ok(ValueRef::Time64(_, t)) => serde_json::json!(t),
            Ok(ValueRef::Interval {
                months,
                days,
                nanos,
            }) => {
                serde_json::json!({
                    "months": months,
                    "days": days,
                    "nanos": nanos
                })
            }
            Ok(ValueRef::List(list_type, list_idx)) => {
                // Handle VARCHAR[] arrays
                self.list_to_json(&list_type, list_idx)
            }
            Ok(ValueRef::Enum(_, idx)) => serde_json::json!(idx),
            Ok(ValueRef::Struct(arr, idx)) => {
                // Convert struct to object
                self.struct_to_json(arr, idx)
            }
            _ => serde_json::Value::Null,
        }
    }

    fn list_to_json(&self, list_type: &duckdb::types::ListType, idx: usize) -> serde_json::Value {
        use duckdb::arrow::array::{Array, StringArray};

        // Get the list array and extract values at the given index
        match list_type {
            duckdb::types::ListType::Regular(arr) => {
                if arr.is_null(idx) {
                    return serde_json::Value::Null;
                }
                let values = arr.value(idx);
                // Try to convert to string array
                if let Some(str_arr) = values.as_any().downcast_ref::<StringArray>() {
                    let items: Vec<serde_json::Value> = (0..str_arr.len())
                        .map(|i| {
                            if str_arr.is_null(i) {
                                serde_json::Value::Null
                            } else {
                                serde_json::Value::String(str_arr.value(i).to_string())
                            }
                        })
                        .collect();
                    serde_json::Value::Array(items)
                } else {
                    // Fallback: convert to string representation
                    serde_json::Value::String(format!("{:?}", values))
                }
            }
            duckdb::types::ListType::Large(arr) => {
                if arr.is_null(idx) {
                    return serde_json::Value::Null;
                }
                let values = arr.value(idx);
                if let Some(str_arr) = values.as_any().downcast_ref::<StringArray>() {
                    let items: Vec<serde_json::Value> = (0..str_arr.len())
                        .map(|i| {
                            if str_arr.is_null(i) {
                                serde_json::Value::Null
                            } else {
                                serde_json::Value::String(str_arr.value(i).to_string())
                            }
                        })
                        .collect();
                    serde_json::Value::Array(items)
                } else {
                    serde_json::Value::String(format!("{:?}", values))
                }
            }
        }
    }

    fn struct_to_json(
        &self,
        arr: &duckdb::arrow::array::StructArray,
        idx: usize,
    ) -> serde_json::Value {
        use duckdb::arrow::array::Array;

        if arr.is_null(idx) {
            return serde_json::Value::Null;
        }

        let mut obj = serde_json::Map::new();
        for (field_idx, field) in arr.fields().iter().enumerate() {
            let col = arr.column(field_idx);
            // Simplified: just get string representation
            if col.is_null(idx) {
                obj.insert(field.name().clone(), serde_json::Value::Null);
            } else {
                obj.insert(
                    field.name().clone(),
                    serde_json::Value::String(format!("{:?}", col)),
                );
            }
        }
        serde_json::Value::Object(obj)
    }

    // === Integration operations ===

    pub fn get_integrations(&self) -> Result<Vec<Integration>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT integration_name, integration_settings FROM sys_integrations")?;

        let integrations = stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let settings_json: String = row.get(1)?;
                let settings: serde_json::Value =
                    serde_json::from_str(&settings_json).unwrap_or(serde_json::json!({}));
                Ok(Integration { name, settings })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(integrations)
    }

    pub fn upsert_integration(&self, name: &str, settings: &serde_json::Value) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let settings_json = serde_json::to_string(settings)?;
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO sys_integrations (integration_name, integration_settings, created_at, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (integration_name) DO UPDATE SET
                integration_settings = EXCLUDED.integration_settings,
                updated_at = EXCLUDED.updated_at",
            params![name, settings_json, now, now],
        )?;

        Ok(())
    }

    pub fn delete_integration(&self, name: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM sys_integrations WHERE integration_name = ?",
            params![name],
        )?;
        Ok(rows > 0)
    }

    // === Maintenance operations ===

    pub fn compact(&self) -> Result<()> {
        use std::fs;

        // Note: We already hold the filesystem lock for the repository lifetime,
        // so no additional locking is needed here.

        // Proper DuckDB compaction: COPY FROM DATABASE to a new file
        // Note: VACUUM does not reclaim space in DuckDB - only COPY FROM DATABASE does
        // Reference: https://duckdb.org/docs/stable/operations_manual/footprint_of_duckdb/reclaiming_space

        // Create temp path for the compacted database
        let temp_db = self.db_path.with_extension("duckdb.tmp");

        // Remove temp file if it exists from a previous failed run
        let _ = fs::remove_file(&temp_db);

        // Create a new in-memory connection for the compact operation
        // This allows us to attach both source and target databases
        let config = duckdb::Config::default().enable_autoload_extension(false)?;
        let compact_conn = Connection::open_in_memory_with_flags(config)?;

        // Attach the source database (current db_path)
        if let Some(key) = &self.encryption_key {
            compact_conn.execute(
                &format!(
                    "ATTACH '{}' AS source_db (ENCRYPTION_KEY '{}')",
                    self.db_path.display(),
                    key
                ),
                [],
            )?;
        } else {
            compact_conn.execute(
                &format!("ATTACH '{}' AS source_db", self.db_path.display()),
                [],
            )?;
        }

        // Attach the target database (temp file)
        if let Some(key) = &self.encryption_key {
            compact_conn.execute(
                &format!(
                    "ATTACH '{}' AS target_db (ENCRYPTION_KEY '{}')",
                    temp_db.display(),
                    key
                ),
                [],
            )?;
        } else {
            compact_conn.execute(&format!("ATTACH '{}' AS target_db", temp_db.display()), [])?;
        }

        // Workaround for DuckDB issue #16785: COPY FROM DATABASE with foreign keys
        // Setting threads to 1 may help with foreign key constraint ordering
        compact_conn.execute("SET threads TO 1", [])?;

        // Copy all data from source to target
        // This copies schema (tables, constraints, indexes, sequences, macros) and data
        compact_conn.execute("COPY FROM DATABASE source_db TO target_db", [])?;

        // Detach both databases to ensure they're flushed
        compact_conn.execute("DETACH source_db", [])?;
        compact_conn.execute("DETACH target_db", [])?;

        // Close the compact connection
        drop(compact_conn);

        // Close the main database connection temporarily
        // (we hold the internal mutex to prevent other threads from using the connection)
        let mut conn_guard = self.conn.lock().unwrap();

        // Replace the old database with the compacted one
        // Backup the original first, then move temp in place
        let backup_db = self.db_path.with_extension("duckdb.old");
        let _ = fs::remove_file(&backup_db); // Remove old backup if exists
        fs::rename(&self.db_path, &backup_db)?;
        fs::rename(&temp_db, &self.db_path)?;

        // Reopen the connection to the new compacted database
        let new_conn = if let Some(key) = &self.encryption_key {
            let config = duckdb::Config::default().enable_autoload_extension(false)?;
            let conn = Connection::open_in_memory_with_flags(config)?;
            conn.execute(
                &format!(
                    "ATTACH '{}' AS main_db (ENCRYPTION_KEY '{}')",
                    self.db_path.display(),
                    key
                ),
                [],
            )?;
            conn.execute("USE main_db", [])?;
            conn
        } else {
            let config = duckdb::Config::default().enable_autoload_extension(false)?;
            Connection::open_with_flags(&self.db_path, config)?
        };

        // Replace the connection in the mutex
        *conn_guard = new_conn;

        // Clean up the backup file
        let _ = fs::remove_file(&backup_db);

        Ok(())
    }

    pub fn get_db_size(&self) -> Result<u64> {
        // Get actual file size from filesystem
        let metadata = std::fs::metadata(&self.db_path)?;
        Ok(metadata.len())
    }

    // === Doctor checks ===

    pub fn check_orphaned_transactions(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT t.transaction_id FROM sys_transactions t
             LEFT JOIN sys_accounts a ON t.account_id = a.account_id
             WHERE a.account_id IS NULL AND t.deleted_at IS NULL",
        )?;

        let orphans: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(orphans)
    }

    pub fn check_orphaned_snapshots(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.snapshot_id FROM sys_balance_snapshots s
             LEFT JOIN sys_accounts a ON s.account_id = a.account_id
             WHERE a.account_id IS NULL",
        )?;

        let orphans: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(orphans)
    }

    pub fn check_future_transactions(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        // Use Rust-computed date to avoid ICU extension dependency
        let tomorrow = (chrono::Utc::now() + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sys_transactions
             WHERE transaction_date > ? AND deleted_at IS NULL",
            params![tomorrow],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn count_untagged_transactions(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sys_transactions
             WHERE (tags IS NULL OR len(tags) = 0)
             AND deleted_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn count_uncategorized_expenses(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sys_transactions
             WHERE amount < 0
             AND (tags IS NULL OR len(tags) = 0)
             AND deleted_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Check for transactions with unreasonable dates (before 1970 or more than 1 year in future)
    pub fn check_date_sanity(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        // Use Rust-computed date to avoid ICU extension dependency
        let one_year_future = (chrono::Utc::now() + chrono::Duration::days(365))
            .format("%Y-%m-%d")
            .to_string();
        let mut stmt = conn.prepare(
            "SELECT transaction_id, transaction_date::VARCHAR, description, amount
             FROM sys_transactions
             WHERE deleted_at IS NULL
               AND (transaction_date > ?
                    OR transaction_date < '1970-01-01')
             LIMIT 100",
        )?;

        let results: Vec<String> = stmt
            .query_map(params![one_year_future], |row| {
                let tx_id: String = row.get(0)?;
                let date: String = row.get(1)?;
                let desc: Option<String> = row.get(2)?;
                let amount: f64 = row.get(3)?;
                Ok(format!(
                    "{}|{}|{}|{}",
                    tx_id,
                    date,
                    desc.unwrap_or_default(),
                    amount
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Check if a table exists
    pub fn table_exists(&self, table_name: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        // Split schema.table if present
        let (schema, table) = if table_name.contains('.') {
            let parts: Vec<&str> = table_name.split('.').collect();
            (parts[0], parts[1])
        } else {
            ("main", table_name)
        };

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM information_schema.tables
             WHERE table_schema = ? AND table_name = ?",
            [schema, table],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // ========================================================================
    // Auto-Tag Rules
    // ========================================================================

    /// Get all enabled auto-tag rules, ordered by sort_order
    pub fn get_enabled_auto_tag_rules(&self) -> Result<Vec<AutoTagRule>> {
        let conn = self.conn.lock().unwrap();
        // CAST(tags AS VARCHAR) is critical here - without it, duckdb-rs silently fails
        // to read VARCHAR[] as String, returning "[]" and causing rules to have no tags.
        // This was the root cause of auto-tag rules not applying. See parse_duckdb_array().
        let mut stmt = conn.prepare(
            "SELECT rule_id, name, sql_condition, CAST(tags AS VARCHAR) as tags_str, enabled, sort_order
             FROM sys_transactions_rules
             WHERE enabled = true
             ORDER BY sort_order, created_at"
        )?;

        let rules = stmt.query_map([], |row| {
            let tags_str: String = row.get(3).unwrap_or_else(|_| "[]".to_string());
            Ok(AutoTagRule {
                rule_id: row.get(0)?,
                name: row.get(1)?,
                sql_condition: row.get(2)?,
                tags: parse_duckdb_array(&tags_str),
                enabled: row.get(4)?,
                sort_order: row.get(5)?,
            })
        })?;

        let mut result = Vec::new();
        for rule in rules {
            result.push(rule?);
        }
        Ok(result)
    }

    /// Get transaction IDs that match a SQL condition from a given set of IDs
    ///
    /// The sql_condition should be a valid SQL WHERE clause fragment
    /// (e.g., "description ILIKE '%walmart%'" or "amount < 0")
    pub fn get_transactions_matching_rule(
        &self,
        tx_ids: &[Uuid],
        sql_condition: &str,
    ) -> Result<Vec<Uuid>> {
        if tx_ids.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().unwrap();

        // Build IN clause with UUIDs
        let id_list: Vec<String> = tx_ids.iter().map(|id| format!("'{}'", id)).collect();
        let in_clause = id_list.join(", ");

        // Build query with user's SQL condition
        // Use the transactions view to ensure computed columns are available
        let sql = format!(
            "SELECT transaction_id FROM transactions
             WHERE transaction_id IN ({})
             AND ({})",
            in_clause, sql_condition
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            Ok(id_str)
        })?;

        let mut result = Vec::new();
        for row in rows {
            if let Ok(id_str) = row {
                if let Ok(uuid) = Uuid::parse_str(&id_str) {
                    result.push(uuid);
                }
            }
        }
        Ok(result)
    }
}

/// Query result structure
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
}

/// Integration info
#[derive(Debug, Clone)]
pub struct Integration {
    pub name: String,
    pub settings: serde_json::Value,
}

// Helper functions

fn parse_timestamp(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or_else(|_| Utc::now().date_naive())
}

fn parse_naive_datetime(s: &str) -> NaiveDateTime {
    // Try parsing with timezone first (e.g., "2026-01-14T23:59:59+00:00")
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return dt.naive_utc();
    }
    // Strip timezone suffix if present (e.g., "+00:00") and parse as naive
    let s_stripped = if s.len() > 6 && (s.contains('+') || s.ends_with('Z')) {
        s.trim_end_matches('Z')
            .rsplit_once('+')
            .map(|(base, _)| base)
            .or_else(|| {
                s.rsplit_once('-')
                    .filter(|(base, tz)| base.len() > 10 && tz.contains(':'))
                    .map(|(base, _)| base)
            })
            .unwrap_or(s)
    } else {
        s
    };
    // Try various timestamp formats that DuckDB might produce
    NaiveDateTime::parse_from_str(s_stripped, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s_stripped, "%Y-%m-%d %H:%M:%S%.f"))
        .or_else(|_| NaiveDateTime::parse_from_str(s_stripped, "%Y-%m-%d %H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(s_stripped, "%Y-%m-%dT%H:%M:%S%.f"))
        .unwrap_or_else(|_| Utc::now().naive_utc())
}

/// Format tags as a DuckDB array literal: ['tag1', 'tag2']
fn format_tags_array(tags: &[String]) -> String {
    if tags.is_empty() {
        return "[]".to_string();
    }

    let escaped: Vec<String> = tags
        .iter()
        .map(|t| {
            // Escape single quotes by doubling them
            let escaped = t.replace('\'', "''");
            format!("'{}'", escaped)
        })
        .collect();

    format!("[{}]", escaped.join(", "))
}

/// Parse DuckDB array string format: [tag1, tag2] or ['tag1', 'tag2']
fn parse_duckdb_array(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() || s == "[]" || s == "NULL" {
        return Vec::new();
    }

    // Remove brackets
    let inner = s.trim_start_matches('[').trim_end_matches(']');
    if inner.is_empty() {
        return Vec::new();
    }

    // Split by comma and clean up each element
    inner
        .split(',')
        .map(|item| item.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Ensure WAL is checkpointed before the connection is dropped.
/// This prevents WAL corruption on restart.
impl Drop for DuckDbRepository {
    fn drop(&mut self) {
        // Force a checkpoint to flush WAL to the main database file
        // This ensures clean shutdown and prevents WAL corruption on restart
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute("CHECKPOINT", []);
        }
        // _lock_file is dropped after this, releasing the filesystem lock
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Valid SQL Tests ====================

    #[test]
    fn test_valid_select() {
        assert!(validate_sql_syntax("SELECT * FROM transactions").is_ok());
    }

    #[test]
    fn test_valid_select_with_limit() {
        assert!(validate_sql_syntax("SELECT * FROM transactions LIMIT 10").is_ok());
    }

    #[test]
    fn test_valid_select_with_where() {
        assert!(validate_sql_syntax("SELECT * FROM transactions WHERE amount > 100").is_ok());
    }

    #[test]
    fn test_valid_select_with_join() {
        assert!(validate_sql_syntax(
            "SELECT t.*, a.name FROM transactions t JOIN accounts a ON t.account_id = a.id"
        )
        .is_ok());
    }

    #[test]
    fn test_valid_insert() {
        assert!(
            validate_sql_syntax("INSERT INTO transactions (id, amount) VALUES ('abc', 100)")
                .is_ok()
        );
    }

    #[test]
    fn test_valid_update() {
        assert!(
            validate_sql_syntax("UPDATE transactions SET amount = 200 WHERE id = 'abc'").is_ok()
        );
    }

    #[test]
    fn test_valid_delete() {
        assert!(validate_sql_syntax("DELETE FROM transactions WHERE id = 'abc'").is_ok());
    }

    #[test]
    fn test_valid_cte() {
        assert!(validate_sql_syntax(
            "WITH monthly AS (SELECT * FROM transactions) SELECT * FROM monthly"
        )
        .is_ok());
    }

    #[test]
    fn test_valid_subquery() {
        assert!(validate_sql_syntax(
            "SELECT * FROM transactions WHERE account_id IN (SELECT id FROM accounts)"
        )
        .is_ok());
    }

    #[test]
    fn test_valid_aggregate() {
        assert!(validate_sql_syntax(
            "SELECT account_id, SUM(amount) FROM transactions GROUP BY account_id HAVING SUM(amount) > 1000"
        ).is_ok());
    }

    // ==================== Missing Space Errors ====================
    // These are the original crash cases on Windows

    #[test]
    fn test_missing_space_before_limit() {
        let result = validate_sql_syntax("SELECT * FROM transactionsLIMIT 10");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(!err.contains("sql parser error:")); // Verify cleanup worked
        assert!(err.contains("Expected")); // Should have meaningful error
    }

    #[test]
    fn test_missing_space_before_where() {
        let result = validate_sql_syntax("SELECT * FROM transactionsWHERE amount > 100");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_space_before_order() {
        let result = validate_sql_syntax("SELECT * FROM transactionsORDER BY date");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_space_before_group() {
        let result =
            validate_sql_syntax("SELECT account_id, COUNT(*) FROM transactionsGROUP BY account_id");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_space_after_select() {
        // SELECT* is actually valid - * is parsed as "all columns"
        let result = validate_sql_syntax("SELECT* FROM transactions");
        assert!(result.is_ok());
    }

    // ==================== Syntax Errors ====================

    #[test]
    fn test_unclosed_parenthesis() {
        let result = validate_sql_syntax("SELECT * FROM transactions WHERE (amount > 100");
        assert!(result.is_err());
    }

    #[test]
    fn test_unclosed_string() {
        let result = validate_sql_syntax("SELECT * FROM transactions WHERE name = 'test");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_from() {
        let result = validate_sql_syntax("SELECT * transactions");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_table_name() {
        let result = validate_sql_syntax("SELECT * FROM");
        assert!(result.is_err());
    }

    #[test]
    fn test_double_where() {
        let result = validate_sql_syntax("SELECT * FROM transactions WHERE WHERE amount > 100");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_operator() {
        // >> is actually valid (bitshift), use something truly invalid
        let result = validate_sql_syntax("SELECT * FROM transactions WHERE amount <> > 100");
        assert!(result.is_err());
    }

    #[test]
    fn test_typo_in_keyword() {
        let result = validate_sql_syntax("SELEC * FROM transactions");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_values_keyword() {
        let result = validate_sql_syntax("INSERT INTO transactions (id) ('abc')");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_set_keyword() {
        let result = validate_sql_syntax("UPDATE transactions amount = 100");
        assert!(result.is_err());
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_empty_query() {
        // Empty string parses as valid (zero statements) in sqlparser
        // DuckDB will reject this at execution time, which is fine
        let result = validate_sql_syntax("");
        assert!(result.is_ok());
    }

    #[test]
    fn test_whitespace_only() {
        // Whitespace-only parses as valid (zero statements)
        let result = validate_sql_syntax("   ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_just_semicolon() {
        // Semicolon alone is valid (empty statement)
        let result = validate_sql_syntax(";");
        assert!(result.is_ok());
    }

    #[test]
    fn test_random_text() {
        let result = validate_sql_syntax("hello world this is not sql");
        assert!(result.is_err());
    }

    #[test]
    fn test_partial_statement() {
        let result = validate_sql_syntax("SELECT");
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_statements_valid() {
        // Multiple statements should be valid
        assert!(validate_sql_syntax("SELECT 1; SELECT 2;").is_ok());
    }

    #[test]
    fn test_comment_only() {
        // Comments alone are valid (zero statements after parsing)
        let result = validate_sql_syntax("-- this is a comment");
        assert!(result.is_ok());
    }

    #[test]
    fn test_unbalanced_quotes() {
        let result = validate_sql_syntax("SELECT * FROM t WHERE x = 'abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_closing_paren() {
        let result = validate_sql_syntax("SELECT * FROM t WHERE (x = 1 AND y = 2");
        assert!(result.is_err());
    }

    // ==================== Error Message Format ====================

    #[test]
    fn test_error_message_format() {
        let result = validate_sql_syntax("SELECT * FROM transactionsLIMIT 10");
        let err = result.unwrap_err().to_string();

        // Should NOT contain the redundant "sql parser error:" prefix
        assert!(!err.contains("sql parser error:"), "Error was: {}", err);

        // Should have meaningful content
        assert!(err.contains("Expected"), "Error was: {}", err);
    }

    #[test]
    fn test_error_contains_location_info() {
        let result = validate_sql_syntax("SELECT * FROM transactionsLIMIT 10");
        let err = result.unwrap_err().to_string();

        // Should contain line/column info
        assert!(
            err.contains("Line:") || err.contains("line"),
            "Error was: {}",
            err
        );
    }

    // ==================== DuckDB-specific Commands ====================
    // These commands are valid in DuckDB but not recognized by sqlparser's DuckDbDialect.
    // We skip validation for these to allow them through.

    #[test]
    fn test_checkpoint_command() {
        // CHECKPOINT is a valid DuckDB command to force WAL flush
        // sqlparser doesn't recognize it, so we skip validation for this command
        assert!(validate_sql_syntax("CHECKPOINT").is_ok());
    }

    #[test]
    fn test_checkpoint_command_lowercase() {
        assert!(validate_sql_syntax("checkpoint").is_ok());
    }

    #[test]
    fn test_vacuum_command() {
        // VACUUM is a valid DuckDB command to reclaim space
        // sqlparser doesn't recognize it, so we skip validation for this command
        assert!(validate_sql_syntax("VACUUM").is_ok());
    }

    #[test]
    fn test_vacuum_command_lowercase() {
        assert!(validate_sql_syntax("vacuum").is_ok());
    }

    // ==================== parse_duckdb_array Tests ====================
    // These tests ensure VARCHAR[] arrays are correctly parsed from DuckDB string format.
    // This is critical for auto-tag rules where tags are stored as VARCHAR[].

    #[test]
    fn test_parse_duckdb_array_empty() {
        assert_eq!(parse_duckdb_array("[]"), Vec::<String>::new());
        assert_eq!(parse_duckdb_array(""), Vec::<String>::new());
        assert_eq!(parse_duckdb_array("NULL"), Vec::<String>::new());
    }

    #[test]
    fn test_parse_duckdb_array_single_element() {
        assert_eq!(parse_duckdb_array("[groceries]"), vec!["groceries"]);
        assert_eq!(parse_duckdb_array("[test-tag]"), vec!["test-tag"]);
    }

    #[test]
    fn test_parse_duckdb_array_multiple_elements() {
        assert_eq!(
            parse_duckdb_array("[groceries, food, essentials]"),
            vec!["groceries", "food", "essentials"]
        );
    }

    #[test]
    fn test_parse_duckdb_array_with_single_quotes() {
        // DuckDB sometimes returns arrays with single-quoted strings
        assert_eq!(parse_duckdb_array("['groceries']"), vec!["groceries"]);
        assert_eq!(
            parse_duckdb_array("['groceries', 'food']"),
            vec!["groceries", "food"]
        );
    }

    #[test]
    fn test_parse_duckdb_array_with_double_quotes() {
        assert_eq!(parse_duckdb_array("[\"groceries\"]"), vec!["groceries"]);
        assert_eq!(
            parse_duckdb_array("[\"groceries\", \"food\"]"),
            vec!["groceries", "food"]
        );
    }

    #[test]
    fn test_parse_duckdb_array_with_whitespace() {
        assert_eq!(parse_duckdb_array("  [groceries]  "), vec!["groceries"]);
        assert_eq!(
            parse_duckdb_array("[  groceries  ,  food  ]"),
            vec!["groceries", "food"]
        );
    }

    #[test]
    fn test_parse_duckdb_array_preserves_hyphens_and_special_chars() {
        // Tags can contain hyphens and other characters
        assert_eq!(
            parse_duckdb_array("[test-auto-tag, my_tag, tag123]"),
            vec!["test-auto-tag", "my_tag", "tag123"]
        );
    }

    #[test]
    fn test_parse_duckdb_array_filters_empty_elements() {
        // Empty elements should be filtered out
        assert_eq!(
            parse_duckdb_array("[groceries, , food]"),
            vec!["groceries", "food"]
        );
    }
}
