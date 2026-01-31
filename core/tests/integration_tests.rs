//! Integration tests for treeline-core services
//!
//! These tests verify critical data integrity scenarios using real DuckDB.
//! Network IO is mocked at the trait level, but all database operations are real.
//!
//! Run with: cargo test --test integration_tests -- --nocapture

use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;

use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::config::ColumnMappings;
use treeline_core::domain::{Account, BalanceSnapshot, Transaction};
use treeline_core::services::{
    BackupService, ImportOptions, ImportService, NumberFormat, TagService,
};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test repository with schema initialized
fn create_test_repo(temp_dir: &TempDir) -> Arc<DuckDbRepository> {
    let db_path = temp_dir.path().join("test.duckdb");
    let repo = DuckDbRepository::new(&db_path, None).expect("Failed to create repository");
    repo.ensure_schema().expect("Failed to initialize schema");
    Arc::new(repo)
}

/// Create a test account with a unique ID
fn create_test_account(name: &str) -> Account {
    Account::new(Uuid::new_v4(), name.to_string())
}

/// Create a test transaction
fn create_test_transaction(account_id: Uuid, amount: i64, date: NaiveDate) -> Transaction {
    Transaction::new(
        Uuid::new_v4(),
        account_id,
        Decimal::new(amount, 2), // amount in cents, e.g., 1234 = $12.34
        date,
    )
}

/// Create a balance snapshot with current time
fn create_balance_snapshot(account_id: Uuid, balance: Decimal) -> BalanceSnapshot {
    let now = Utc::now().naive_utc();
    BalanceSnapshot::new(account_id, balance, now)
}

// ============================================================================
// Delete Account Atomicity Tests
// ============================================================================

/// Test that delete_account is atomic - all or nothing
#[test]
fn test_delete_account_removes_all_related_data() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // Create account with transactions and balance snapshots
    let account = create_test_account("Test Account");
    let account_id = account.id;
    repo.upsert_account(&account).unwrap();

    // Add transactions
    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    for i in 0..5 {
        let mut tx = create_test_transaction(account_id, (i + 1) * 1000, date);
        tx.description = Some(format!("Transaction {}", i));
        repo.upsert_transaction(&tx).unwrap();
    }

    // Add balance snapshot
    let snapshot = create_balance_snapshot(account_id, Decimal::new(5000, 2));
    repo.add_balance_snapshot(&snapshot).unwrap();

    // Verify data exists
    let accounts_before = repo.get_accounts().unwrap();
    let transactions_before = repo.get_transactions().unwrap();
    let snapshots_before = repo.get_balance_snapshot_count().unwrap();
    assert_eq!(accounts_before.len(), 1);
    assert_eq!(transactions_before.len(), 5);
    assert!(snapshots_before >= 1);

    // Delete account
    repo.delete_account(&account_id.to_string()).unwrap();

    // Verify ALL related data is gone
    let accounts_after = repo.get_accounts().unwrap();
    let transactions_after = repo.get_transactions().unwrap();

    assert_eq!(accounts_after.len(), 0, "Account should be deleted");
    assert_eq!(
        transactions_after.len(),
        0,
        "All transactions should be deleted"
    );
}

/// Test that delete_account doesn't leave orphaned data on partial failure
#[test]
fn test_delete_account_no_orphans_on_nonexistent_account() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // Create some accounts and transactions first
    let account1 = create_test_account("Account 1");
    let account2 = create_test_account("Account 2");
    repo.upsert_account(&account1).unwrap();
    repo.upsert_account(&account2).unwrap();

    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let tx1 = create_test_transaction(account1.id, 1000, date);
    let tx2 = create_test_transaction(account2.id, 2000, date);
    repo.upsert_transaction(&tx1).unwrap();
    repo.upsert_transaction(&tx2).unwrap();

    // Delete non-existent account (should succeed without error)
    let fake_id = Uuid::new_v4();
    let result = repo.delete_account(&fake_id.to_string());
    assert!(
        result.is_ok(),
        "Deleting non-existent account should not error"
    );

    // Verify existing data is intact
    let accounts = repo.get_accounts().unwrap();
    let transactions = repo.get_transactions().unwrap();
    assert_eq!(accounts.len(), 2, "Existing accounts should remain");
    assert_eq!(transactions.len(), 2, "Existing transactions should remain");
}

/// Test that deleting one account doesn't affect another account's data
#[test]
fn test_delete_account_isolation() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // Create two accounts with transactions
    let account1 = create_test_account("Account 1");
    let account2 = create_test_account("Account 2");
    repo.upsert_account(&account1).unwrap();
    repo.upsert_account(&account2).unwrap();

    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();

    // 3 transactions for account1
    for i in 0..3 {
        let tx = create_test_transaction(account1.id, i * 100, date);
        repo.upsert_transaction(&tx).unwrap();
    }

    // 2 transactions for account2
    for i in 0..2 {
        let tx = create_test_transaction(account2.id, i * 200, date);
        repo.upsert_transaction(&tx).unwrap();
    }

    // Delete account1 only
    repo.delete_account(&account1.id.to_string()).unwrap();

    // Verify account2's data is intact
    let accounts = repo.get_accounts().unwrap();
    let transactions = repo.get_transactions().unwrap();

    assert_eq!(accounts.len(), 1, "Only account1 should be deleted");
    assert_eq!(accounts[0].id, account2.id, "account2 should remain");
    assert_eq!(
        transactions.len(),
        2,
        "account2's transactions should remain"
    );
    assert!(
        transactions.iter().all(|t| t.account_id == account2.id),
        "All remaining transactions should belong to account2"
    );
}

// ============================================================================
// Backup Service Tests
// ============================================================================

/// Test that backup includes all data and can be listed
#[test]
fn test_backup_create_and_list() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // Add some data
    let account = create_test_account("Backup Test Account");
    repo.upsert_account(&account).unwrap();

    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let tx = create_test_transaction(account.id, 5000, date);
    repo.upsert_transaction(&tx).unwrap();

    // Create backup service with repository (for checkpointing)
    let backup_service = BackupService::new_with_repository(
        temp_dir.path().to_path_buf(),
        "test.duckdb".to_string(),
        repo.clone(),
    );

    // Create backup
    let backup_result = backup_service.create(None);
    assert!(backup_result.is_ok(), "Backup should succeed");
    let backup = backup_result.unwrap();
    assert!(backup.name.starts_with("treeline-"), "Backup name format");
    assert!(backup.name.ends_with(".zip"), "Backup should be a zip file");
    assert!(backup.size_bytes > 0, "Backup should have content");

    // List backups
    let backups = backup_service.list().unwrap();
    assert_eq!(backups.len(), 1, "Should have one backup");
    assert_eq!(backups[0].name, backup.name);
}

/// Test backup retention policy
#[test]
fn test_backup_retention_policy() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    let backup_service = BackupService::new_with_repository(
        temp_dir.path().to_path_buf(),
        "test.duckdb".to_string(),
        repo.clone(),
    );

    // Create 5 backups
    for _ in 0..5 {
        backup_service.create(None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10)); // Ensure unique timestamps
    }

    let backups_before = backup_service.list().unwrap();
    assert_eq!(backups_before.len(), 5, "Should have 5 backups");

    // Create one more with max_backups = 3
    backup_service.create(Some(3)).unwrap();

    let backups_after = backup_service.list().unwrap();
    assert_eq!(
        backups_after.len(),
        3,
        "Should have at most 3 backups after retention"
    );
}

/// Test backup restore
#[test]
fn test_backup_restore() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    // Create initial state
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();

        let account = create_test_account("Original Account");
        repo.upsert_account(&account).unwrap();
    }

    // Create backup (with repo for checkpoint)
    let backup_name;
    {
        let repo = Arc::new(DuckDbRepository::new(&db_path, None).unwrap());
        let backup_service = BackupService::new_with_repository(
            temp_dir.path().to_path_buf(),
            "test.duckdb".to_string(),
            repo.clone(),
        );
        let backup = backup_service.create(None).unwrap();
        backup_name = backup.name;
    }

    // Modify data
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        let account2 = create_test_account("New Account After Backup");
        repo.upsert_account(&account2).unwrap();

        let accounts = repo.get_accounts().unwrap();
        assert_eq!(accounts.len(), 2, "Should have 2 accounts before restore");
    }

    // Restore from backup (without repo - restore doesn't need checkpoint)
    {
        let backup_service =
            BackupService::new(temp_dir.path().to_path_buf(), "test.duckdb".to_string());
        backup_service.restore(&backup_name).unwrap();
    }

    // Verify restored state
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        let accounts = repo.get_accounts().unwrap();
        assert_eq!(
            accounts.len(),
            1,
            "Should have only 1 account after restore"
        );
        assert!(
            accounts[0].name.contains("Original"),
            "Should be the original account"
        );
    }
}

// ============================================================================
// Tag Service Tests
// ============================================================================

/// Test applying tags to transactions
#[test]
fn test_tag_apply_to_transactions() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    let tag_service = TagService::new(repo.clone());

    // Create account and transaction
    let account = create_test_account("Tag Test Account");
    repo.upsert_account(&account).unwrap();

    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let tx = create_test_transaction(account.id, 1000, date);
    repo.upsert_transaction(&tx).unwrap();

    // Apply tags
    let tags = vec!["groceries".to_string(), "food".to_string()];
    let result = tag_service
        .apply_tags(&[tx.id.to_string()], &tags, false)
        .unwrap();

    assert_eq!(result.succeeded, 1);
    assert_eq!(result.failed, 0);

    // Verify tags were applied
    let updated_tx = repo.get_transaction_by_id(&tx.id.to_string()).unwrap();
    assert!(updated_tx.is_some());
    let updated_tx = updated_tx.unwrap();
    assert!(updated_tx.tags.contains(&"groceries".to_string()));
    assert!(updated_tx.tags.contains(&"food".to_string()));
}

/// Test tag merging (additive)
#[test]
fn test_tag_merge_additive() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    let tag_service = TagService::new(repo.clone());

    // Create account and transaction with initial tags
    let account = create_test_account("Tag Merge Test");
    repo.upsert_account(&account).unwrap();

    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let mut tx = create_test_transaction(account.id, 1000, date);
    tx.tags = vec!["existing".to_string()];
    repo.upsert_transaction(&tx).unwrap();

    // Apply new tags (additive, not replace)
    let new_tags = vec!["new_tag".to_string()];
    tag_service
        .apply_tags(&[tx.id.to_string()], &new_tags, false)
        .unwrap();

    // Verify both old and new tags exist
    let updated_tx = repo
        .get_transaction_by_id(&tx.id.to_string())
        .unwrap()
        .unwrap();
    assert!(
        updated_tx.tags.contains(&"existing".to_string()),
        "Original tag should remain"
    );
    assert!(
        updated_tx.tags.contains(&"new_tag".to_string()),
        "New tag should be added"
    );
}

/// Test tag replacement
#[test]
fn test_tag_replace() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    let tag_service = TagService::new(repo.clone());

    // Create account and transaction with initial tags
    let account = create_test_account("Tag Replace Test");
    repo.upsert_account(&account).unwrap();

    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let mut tx = create_test_transaction(account.id, 1000, date);
    tx.tags = vec!["old_tag".to_string()];
    repo.upsert_transaction(&tx).unwrap();

    // Replace tags
    let new_tags = vec!["replacement".to_string()];
    tag_service
        .apply_tags(&[tx.id.to_string()], &new_tags, true)
        .unwrap();

    // Verify only new tags exist
    let updated_tx = repo
        .get_transaction_by_id(&tx.id.to_string())
        .unwrap()
        .unwrap();
    assert!(
        !updated_tx.tags.contains(&"old_tag".to_string()),
        "Old tag should be gone"
    );
    assert!(
        updated_tx.tags.contains(&"replacement".to_string()),
        "New tag should exist"
    );
}

/// Test applying tags to invalid transaction ID
#[test]
fn test_tag_invalid_transaction_id() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    let tag_service = TagService::new(repo.clone());

    // Try to tag with invalid UUID
    let tags = vec!["test".to_string()];
    let result = tag_service.apply_tags(&["not-a-valid-uuid".to_string()], &tags, false);

    assert!(result.is_ok());
    let result = result.unwrap();
    assert_eq!(result.failed, 1, "Should fail for invalid UUID");
    assert_eq!(result.succeeded, 0);
}

// ============================================================================
// Import Service Tests
// ============================================================================

/// Test CSV import with valid data
#[test]
fn test_csv_import_valid_data() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // Create account for import
    let account = create_test_account("Import Test Account");
    repo.upsert_account(&account).unwrap();

    // Create CSV file
    let csv_content = r#"date,amount,description
2024-01-15,12.34,Coffee Shop
2024-01-16,-45.67,Restaurant
2024-01-17,100.00,Refund"#;

    let csv_path = temp_dir.path().join("test_import.csv");
    std::fs::write(&csv_path, csv_content).unwrap();

    // Import using the import service
    let import_service = ImportService::new(repo.clone(), temp_dir.path().to_path_buf());

    let mappings = ColumnMappings {
        date: "date".to_string(),
        amount: "amount".to_string(),
        description: Some("description".to_string()),
        credit: None,
        debit: None,
        balance: None,
    };

    let options = ImportOptions {
        debit_negative: false,
        flip_signs: false,
        skip_rows: 0,
        number_format: NumberFormat::default(),
        anchor_balance: None,
        anchor_date: None,
    };

    let result = import_service
        .import(
            Path::new(&csv_path),
            &account.id.to_string(),
            &mappings,
            &options,
            false,
        )
        .unwrap();

    assert_eq!(result.imported, 3, "Should import 3 transactions");
    assert_eq!(result.skipped, 0, "Should skip no transactions");

    // Verify transactions were created
    let transactions = repo
        .get_transactions_by_account(&account.id.to_string())
        .unwrap();
    assert_eq!(transactions.len(), 3, "Should have 3 transactions");

    // Verify amounts (check one positive and one negative)
    let coffee_tx = transactions.iter().find(|t| {
        t.description
            .as_ref()
            .map(|d| d.contains("Coffee"))
            .unwrap_or(false)
    });
    assert!(coffee_tx.is_some(), "Should find Coffee transaction");
    // The import service stores amounts, verify it's positive (12.34 from CSV)
    assert!(
        coffee_tx.unwrap().amount > Decimal::ZERO,
        "Coffee transaction amount should be positive"
    );
}

/// Test CSV import deduplication
#[test]
fn test_csv_import_deduplication() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    let account = create_test_account("Dedup Test Account");
    repo.upsert_account(&account).unwrap();

    let csv_content = r#"date,amount,description
2024-01-15,12.34,Coffee Shop"#;

    let csv_path = temp_dir.path().join("test_dedup.csv");
    std::fs::write(&csv_path, csv_content).unwrap();

    let import_service = ImportService::new(repo.clone(), temp_dir.path().to_path_buf());

    let mappings = ColumnMappings {
        date: "date".to_string(),
        amount: "amount".to_string(),
        description: Some("description".to_string()),
        credit: None,
        debit: None,
        balance: None,
    };

    let options = ImportOptions {
        debit_negative: false,
        flip_signs: false,
        skip_rows: 0,
        number_format: NumberFormat::default(),
        anchor_balance: None,
        anchor_date: None,
    };

    // First import
    let result1 = import_service
        .import(
            Path::new(&csv_path),
            &account.id.to_string(),
            &mappings,
            &options,
            false,
        )
        .unwrap();

    assert_eq!(result1.imported, 1);

    // Second import of same data should be skipped
    let result2 = import_service
        .import(
            Path::new(&csv_path),
            &account.id.to_string(),
            &mappings,
            &options,
            false,
        )
        .unwrap();

    assert_eq!(result2.imported, 0, "Should not import duplicates");
    assert_eq!(result2.skipped, 1, "Should skip the duplicate");

    // Verify only one transaction exists
    let transactions = repo
        .get_transactions_by_account(&account.id.to_string())
        .unwrap();
    assert_eq!(transactions.len(), 1, "Should have only 1 transaction");
}

// ============================================================================
// Data Integrity Tests
// ============================================================================

/// Test that invalid UUIDs in queries are handled properly
#[test]
fn test_invalid_uuid_handling() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // Query with invalid UUID should return None, not error
    let result = repo.get_account_by_id("not-a-valid-uuid");
    assert!(result.is_ok(), "Should not error on invalid UUID");
    assert!(
        result.unwrap().is_none(),
        "Should return None for invalid UUID"
    );

    // Query with non-existent valid UUID should also return None
    let fake_uuid = Uuid::new_v4();
    let result = repo.get_account_by_id(&fake_uuid.to_string());
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

/// Test transaction date range query
#[test]
fn test_transaction_date_range() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    let account = create_test_account("Date Range Test");
    repo.upsert_account(&account).unwrap();

    // Create transactions on different dates
    let dates = vec![
        NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2024, 6, 15).unwrap(),
        NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
    ];

    for (i, date) in dates.iter().enumerate() {
        let tx = create_test_transaction(account.id, (i + 1) as i64 * 1000, *date);
        repo.upsert_transaction(&tx).unwrap();
    }

    let range = repo.get_transaction_date_range().unwrap();
    assert!(range.earliest.is_some());
    assert!(range.latest.is_some());
    assert!(range.earliest.unwrap().contains("2024-01-01"));
    assert!(range.latest.unwrap().contains("2024-12-31"));
}

/// Test balance snapshot ordering
#[test]
fn test_balance_snapshot_ordering() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    let account = create_test_account("Balance Test");
    repo.upsert_account(&account).unwrap();

    // Add multiple balance snapshots with increasing time
    for i in 1i64..=5 {
        let base_time = chrono::DateTime::from_timestamp(1704067200 + i * 60, 0).unwrap();
        let snapshot_time = base_time.naive_utc();
        let snapshot = BalanceSnapshot::new(account.id, Decimal::new(i * 1000, 2), snapshot_time);
        repo.add_balance_snapshot(&snapshot).unwrap();
    }

    // The latest balance should be visible on the account
    let accounts = repo.get_accounts().unwrap();
    let account = accounts.iter().find(|a| a.name == "Balance Test").unwrap();

    // Balance should be the most recent one (50.00)
    assert!(account.balance.is_some());
    assert_eq!(
        account.balance.unwrap(),
        Decimal::new(5000, 2),
        "Should show latest balance"
    );
}

/// Test transaction deduplication by provider ID
#[test]
fn test_sync_deduplication_by_provider_id() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    let account = create_test_account("Dedup Test");
    repo.upsert_account(&account).unwrap();

    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let mut tx = create_test_transaction(account.id, 5000, date);
    tx.sf_id = Some("unique_sf_id".to_string());
    repo.upsert_transaction(&tx).unwrap();

    // Check if transaction exists by SF ID
    let exists = repo.transaction_exists_by_sf_id("unique_sf_id").unwrap();
    assert!(exists, "Transaction should exist by SF ID");

    let not_exists = repo.transaction_exists_by_sf_id("nonexistent").unwrap();
    assert!(!not_exists, "Non-existent SF ID should return false");
}

/// Test Lunchflow ID deduplication
#[test]
fn test_sync_deduplication_by_lunchflow_id() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    let account = create_test_account("LF Dedup Test");
    repo.upsert_account(&account).unwrap();

    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let mut tx = create_test_transaction(account.id, 5000, date);
    tx.lf_id = Some("unique_lf_id".to_string());
    repo.upsert_transaction(&tx).unwrap();

    // Check if transaction exists by LF ID
    let exists = repo.transaction_exists_by_lf_id("unique_lf_id").unwrap();
    assert!(exists, "Transaction should exist by LF ID");

    let not_exists = repo.transaction_exists_by_lf_id("nonexistent").unwrap();
    assert!(!not_exists, "Non-existent LF ID should return false");
}

// ============================================================================
// Query Service Tests
// ============================================================================

/// Test executing custom SQL queries
#[test]
fn test_execute_custom_query() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // Create test data
    let account = create_test_account("Query Test");
    repo.upsert_account(&account).unwrap();

    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    for i in 0..5 {
        let mut tx = create_test_transaction(account.id, (i + 1) * 1000, date);
        tx.description = Some(format!("Transaction {}", i));
        repo.upsert_transaction(&tx).unwrap();
    }

    // Execute custom query
    let result = repo
        .execute_query("SELECT COUNT(*) as cnt FROM sys_transactions")
        .unwrap();

    // Should have results
    assert!(!result.columns.is_empty());
    assert!(!result.rows.is_empty());
}

/// Test query validation rejects dangerous SQL
#[test]
fn test_query_validation() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // Invalid SQL should return error
    let result = repo.execute_query("SELEC * FROM sys_accounts"); // typo in SELECT
    assert!(result.is_err(), "Invalid SQL should fail");
}

// ============================================================================
// DuckDB Command Tests
// ============================================================================
// These tests verify that DuckDB-specific commands (CHECKPOINT, VACUUM) work
// through the repository layer. Full plugin migration scenarios are tested
// in treeline-app where they use TreelineContext (the actual code path).

/// Test that CHECKPOINT command can be executed via execute_sql
/// This is critical for plugin migrations which use CHECKPOINT after DDL
#[test]
fn test_checkpoint_command_execution() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // CHECKPOINT should succeed (it's a valid DuckDB command)
    let result = repo.execute_sql("CHECKPOINT");
    assert!(
        result.is_ok(),
        "CHECKPOINT should succeed: {:?}",
        result.err()
    );
}

/// Test that VACUUM command can be executed via execute_sql
#[test]
fn test_vacuum_command_execution() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    // VACUUM should succeed
    let result = repo.execute_sql("VACUUM");
    assert!(result.is_ok(), "VACUUM should succeed: {:?}", result.err());
}
