//! Integration tests for bulk transaction deduplication
//!
//! These tests verify that the bulk deduplication fix prevents duplicate transactions
//! during CSV imports and provider syncs.
//!
//! ## Running with real credentials
//!
//! To run SimpleFin tests:
//!   SIMPLEFIN_ACCESS_URL="https://user:pass@bridge.simplefin.org/..." cargo test --test bulk_dedup_tests
//!
//! To run Lunchflow tests:
//!   LUNCHFLOW_API_KEY="your-api-key" cargo test --test bulk_dedup_tests
//!
//! Without credentials, those tests are skipped.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::config::ColumnMappings;
use treeline_core::domain::{Account, Transaction};
use treeline_core::services::{ImportOptions, ImportService, SyncService};

// Re-export for test convenience
use serde_json::json;

/// Create a temporary treeline directory with a fresh database
fn setup_test_env() -> (TempDir, Arc<DuckDbRepository>, PathBuf) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let treeline_dir = temp_dir.path().to_path_buf();
    let db_path = treeline_dir.join("data.duckdb");

    let repo = Arc::new(
        DuckDbRepository::new(&db_path, None).expect("Failed to create repository"),
    );
    repo.ensure_schema().expect("Failed to run migrations");

    (temp_dir, repo, treeline_dir)
}

/// Create a test account
fn create_test_account(repo: &DuckDbRepository, name: &str) -> Uuid {
    let account = Account {
        id: Uuid::new_v4(),
        name: name.to_string(),
        nickname: None,
        account_type: Some("checking".to_string()),
        classification: None,
        currency: "USD".to_string(),
        balance: None,
        institution_name: None,
        institution_url: None,
        institution_domain: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        is_manual: true,
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
        lf_id: None,
        lf_name: None,
        lf_institution_name: None,
        lf_institution_logo: None,
        lf_provider: None,
        lf_currency: None,
        lf_status: None,
    };
    repo.upsert_account(&account).expect("Failed to create account");
    account.id
}

/// Create a CSV file with test data
fn create_csv_file(dir: &std::path::Path, filename: &str, content: &str) -> PathBuf {
    let path = dir.join(filename);
    let mut file = std::fs::File::create(&path).expect("Failed to create CSV file");
    file.write_all(content.as_bytes()).expect("Failed to write CSV");
    path
}

// =============================================================================
// CSV Import Deduplication Tests
// =============================================================================

#[test]
fn test_csv_import_no_duplicates_on_reimport() {
    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Checking");

    let import_service = ImportService::new(repo.clone(), treeline_dir.clone());

    // Create a CSV file
    let csv_content = r#"Date,Amount,Description
2024-01-15,100.00,Paycheck
2024-01-16,-25.50,Grocery Store
2024-01-17,-15.00,Coffee Shop
2024-01-18,50.00,Refund
2024-01-19,-200.00,Rent Payment"#;

    let csv_path = create_csv_file(&treeline_dir, "test_import.csv", csv_content);

    let mappings = ColumnMappings {
        date: "Date".to_string(),
        amount: "Amount".to_string(),
        description: Some("Description".to_string()),
        debit: None,
        credit: None,
        balance: None,
    };
    let options = ImportOptions::default();

    // First import
    let result1 = import_service
        .import(&csv_path, &account_id.to_string(), &mappings, &options, false)
        .expect("First import failed");

    assert_eq!(result1.imported, 5, "First import should import 5 transactions");

    // Get transaction count
    let count_after_first = repo.get_transaction_count().expect("Failed to get count");
    assert_eq!(count_after_first, 5);

    // Second import of the same file - should not create duplicates
    let result2 = import_service
        .import(&csv_path, &account_id.to_string(), &mappings, &options, false)
        .expect("Second import failed");

    assert_eq!(result2.imported, 0, "Second import should import 0 (all duplicates)");
    assert_eq!(result2.skipped, 5, "Second import should skip 5 duplicates");

    // Verify total count unchanged
    let count_after_second = repo.get_transaction_count().expect("Failed to get count");
    assert_eq!(count_after_second, 5, "Total count should still be 5");
}

#[test]
fn test_csv_import_partial_overlap() {
    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Checking");

    let import_service = ImportService::new(repo.clone(), treeline_dir.clone());

    // First CSV - 3 transactions
    let csv1 = r#"Date,Amount,Description
2024-01-15,100.00,Paycheck
2024-01-16,-25.50,Grocery Store
2024-01-17,-15.00,Coffee Shop"#;

    let csv1_path = create_csv_file(&treeline_dir, "import1.csv", csv1);

    // Second CSV - 2 overlapping, 2 new
    let csv2 = r#"Date,Amount,Description
2024-01-16,-25.50,Grocery Store
2024-01-17,-15.00,Coffee Shop
2024-01-18,50.00,Refund
2024-01-19,-200.00,Rent Payment"#;

    let csv2_path = create_csv_file(&treeline_dir, "import2.csv", csv2);

    let mappings = ColumnMappings {
        date: "Date".to_string(),
        amount: "Amount".to_string(),
        description: Some("Description".to_string()),
        debit: None,
        credit: None,
        balance: None,
    };
    let options = ImportOptions::default();

    // First import
    let result1 = import_service
        .import(&csv1_path, &account_id.to_string(), &mappings, &options, false)
        .expect("First import failed");
    assert_eq!(result1.imported, 3);

    // Second import - should only add 2 new ones
    let result2 = import_service
        .import(&csv2_path, &account_id.to_string(), &mappings, &options, false)
        .expect("Second import failed");

    assert_eq!(result2.imported, 2, "Should only import 2 new transactions");
    assert_eq!(result2.skipped, 2, "Should skip 2 duplicates");

    // Verify total count
    let total = repo.get_transaction_count().expect("Failed to get count");
    assert_eq!(total, 5, "Total should be 5 unique transactions");
}

// =============================================================================
// Bulk Repository Method Tests
// =============================================================================

#[test]
fn test_bulk_insert_transactions() {
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Create 100 test transactions
    let mut transactions: Vec<Transaction> = Vec::new();
    for i in 0..100 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new((i + 1) * 100, 2), // $1.00, $2.00, etc.
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap() + chrono::Duration::days(i),
        );
        tx.description = Some(format!("Transaction {}", i));
        tx.sf_id = Some(format!("sf_test_{}", i));
        transactions.push(tx);
    }

    // Bulk insert
    let inserted = repo
        .bulk_insert_transactions(&transactions)
        .expect("Bulk insert failed");

    assert_eq!(inserted, 100, "Should insert 100 transactions");

    // Verify count
    let count = repo.get_transaction_count().expect("Failed to get count");
    assert_eq!(count, 100);

    // Try inserting same transactions again (should be skipped due to ON CONFLICT DO NOTHING)
    let inserted_again = repo
        .bulk_insert_transactions(&transactions)
        .expect("Second bulk insert failed");

    assert_eq!(inserted_again, 0, "Should insert 0 on second attempt");

    // Verify count unchanged
    let count_after = repo.get_transaction_count().expect("Failed to get count");
    assert_eq!(count_after, 100);
}

#[test]
fn test_get_existing_sf_ids() {
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Insert some transactions with sf_ids
    let mut transactions: Vec<Transaction> = Vec::new();
    for i in 0..10 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new(100, 2),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        );
        tx.sf_id = Some(format!("existing_sf_{}", i));
        transactions.push(tx);
    }
    repo.bulk_insert_transactions(&transactions).expect("Insert failed");

    // Query for a mix of existing and non-existing IDs
    let query_ids: Vec<String> = (0..15)
        .map(|i| {
            if i < 10 {
                format!("existing_sf_{}", i) // These exist
            } else {
                format!("new_sf_{}", i) // These don't exist
            }
        })
        .collect();

    let existing = repo
        .get_existing_sf_ids(&query_ids)
        .expect("Query failed");

    assert_eq!(existing.len(), 10, "Should find 10 existing IDs");

    // Verify the right ones are found
    for i in 0..10 {
        assert!(
            existing.contains(&format!("existing_sf_{}", i)),
            "Should contain existing_sf_{}",
            i
        );
    }
    for i in 10..15 {
        assert!(
            !existing.contains(&format!("new_sf_{}", i)),
            "Should NOT contain new_sf_{}",
            i
        );
    }
}

#[test]
fn test_get_existing_lf_ids() {
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Insert some transactions with lf_ids
    let mut transactions: Vec<Transaction> = Vec::new();
    for i in 0..10 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new(100, 2),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        );
        tx.lf_id = Some(format!("existing_lf_{}", i));
        transactions.push(tx);
    }
    repo.bulk_insert_transactions(&transactions).expect("Insert failed");

    // Query for a mix of existing and non-existing IDs
    let query_ids: Vec<String> = (0..15)
        .map(|i| {
            if i < 10 {
                format!("existing_lf_{}", i)
            } else {
                format!("new_lf_{}", i)
            }
        })
        .collect();

    let existing = repo
        .get_existing_lf_ids(&query_ids)
        .expect("Query failed");

    assert_eq!(existing.len(), 10, "Should find 10 existing IDs");
}

#[test]
fn test_check_duplicate_sf_ids() {
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Insert transactions - some with duplicate sf_ids
    // We'll use upsert_transaction to bypass the bulk insert's ON CONFLICT
    for i in 0..5 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new(100, 2),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        );
        tx.sf_id = Some(format!("unique_sf_{}", i));
        repo.upsert_transaction(&tx).expect("Insert failed");
    }

    // Insert duplicates (different transaction IDs, same sf_id)
    for _ in 0..3 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new(100, 2),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        );
        tx.sf_id = Some("duplicate_sf_id".to_string());
        repo.upsert_transaction(&tx).expect("Insert failed");
    }

    // Check for duplicates
    let duplicates = repo.check_duplicate_sf_ids().expect("Check failed");

    assert_eq!(duplicates.len(), 1, "Should find 1 duplicate sf_id");
    assert_eq!(duplicates[0], "duplicate_sf_id");
}

#[test]
fn test_bulk_dedup_workflow() {
    // This test simulates the actual sync workflow
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Simulate incoming transactions from a provider (e.g., SimpleFin)
    let incoming_sf_ids: Vec<String> = (0..50).map(|i| format!("sf_tx_{}", i)).collect();

    // First sync - all are new
    let existing = repo.get_existing_sf_ids(&incoming_sf_ids).expect("Query failed");
    assert_eq!(existing.len(), 0, "No transactions should exist yet");

    // Create and insert the transactions
    let transactions: Vec<Transaction> = incoming_sf_ids
        .iter()
        .enumerate()
        .map(|(i, sf_id)| {
            let mut tx = Transaction::new(
                Uuid::new_v4(),
                account_id,
                Decimal::new((i as i64 + 1) * 100, 2),
                NaiveDate::from_ymd_opt(2024, 1, 1).unwrap() + chrono::Duration::days(i as i64),
            );
            tx.sf_id = Some(sf_id.clone());
            tx.description = Some(format!("Transaction {}", i));
            tx
        })
        .collect();

    let inserted = repo.bulk_insert_transactions(&transactions).expect("Insert failed");
    assert_eq!(inserted, 50);

    // Second sync - simulate same transactions coming in again
    let existing_after = repo.get_existing_sf_ids(&incoming_sf_ids).expect("Query failed");
    assert_eq!(existing_after.len(), 50, "All 50 should now exist");

    // Filter to new only (should be none)
    let new_txs: Vec<&Transaction> = transactions
        .iter()
        .filter(|tx| {
            tx.sf_id
                .as_ref()
                .map(|id| !existing_after.contains(id))
                .unwrap_or(true)
        })
        .collect();

    assert_eq!(new_txs.len(), 0, "No new transactions after filter");

    // Verify no duplicates
    let duplicates = repo.check_duplicate_sf_ids().expect("Check failed");
    assert_eq!(duplicates.len(), 0, "Should have no duplicates");
}

// =============================================================================
// SF/LF ID Chunking Tests
// =============================================================================

#[test]
fn test_get_existing_sf_ids_large_batch() {
    // Test chunking with >500 IDs
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Insert 600 transactions with sf_ids
    let mut transactions: Vec<Transaction> = Vec::new();
    for i in 0..600 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new(100, 2),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
                + chrono::Duration::days(i % 365),
        );
        tx.sf_id = Some(format!("sf_{:04}", i));
        transactions.push(tx);
    }
    repo.bulk_insert_transactions(&transactions).expect("Insert failed");

    // Query all 600 plus 100 new ones
    let query_ids: Vec<String> = (0..700).map(|i| format!("sf_{:04}", i)).collect();

    let existing = repo
        .get_existing_sf_ids(&query_ids)
        .expect("Query failed");

    assert_eq!(
        existing.len(),
        600,
        "Should find all 600 existing sf_ids across chunk boundary"
    );
}

#[test]
fn test_get_existing_lf_ids_large_batch() {
    // Test chunking with >500 IDs
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Insert 600 transactions with lf_ids
    let mut transactions: Vec<Transaction> = Vec::new();
    for i in 0..600 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new(100, 2),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
                + chrono::Duration::days(i % 365),
        );
        tx.lf_id = Some(format!("lf_{:04}", i));
        transactions.push(tx);
    }
    repo.bulk_insert_transactions(&transactions).expect("Insert failed");

    // Query all 600 plus 100 new ones
    let query_ids: Vec<String> = (0..700).map(|i| format!("lf_{:04}", i)).collect();

    let existing = repo
        .get_existing_lf_ids(&query_ids)
        .expect("Query failed");

    assert_eq!(
        existing.len(),
        600,
        "Should find all 600 existing lf_ids across chunk boundary"
    );
}

// =============================================================================
// SimpleFin Integration Tests (requires SIMPLEFIN_ACCESS_URL env var)
// =============================================================================

#[test]
fn test_simplefin_sync_no_duplicates() {
    let access_url = match std::env::var("SIMPLEFIN_ACCESS_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("SIMPLEFIN_ACCESS_URL not set, skipping SimpleFin test");
            return;
        }
    };

    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let sync_service = SyncService::new(repo.clone(), treeline_dir.clone());

    // Set up SimpleFin integration directly with access URL
    // (bypass setup_simplefin which expects a setup token)
    repo.upsert_integration(
        "simplefin",
        &json!({ "accessUrl": access_url }),
    )
    .expect("Failed to setup SimpleFin integration");

    // First sync
    let result1 = sync_service
        .sync(Some("simplefin"), false, false)
        .expect("First sync failed");

    let first_sync_new = result1.results[0].transaction_stats.new;
    println!("First sync: {} new transactions", first_sync_new);

    // Get transaction count after first sync
    let count_after_first = repo.get_transaction_count().expect("Failed to get count");

    // Second sync - should not create any duplicates
    let result2 = sync_service
        .sync(Some("simplefin"), false, false)
        .expect("Second sync failed");

    let second_sync_new = result2.results[0].transaction_stats.new;
    let second_sync_skipped = result2.results[0].transaction_stats.skipped;
    println!(
        "Second sync: {} new, {} skipped",
        second_sync_new, second_sync_skipped
    );

    // The second sync might have some new transactions if new ones appeared,
    // but importantly, we should not have created duplicates
    let count_after_second = repo.get_transaction_count().expect("Failed to get count");

    // Check for duplicates
    let duplicates = repo.check_duplicate_sf_ids().expect("Check failed");
    assert!(
        duplicates.is_empty(),
        "Should have no duplicate sf_ids after two syncs. Found: {:?}",
        duplicates
    );

    println!(
        "Transaction count: {} after first, {} after second",
        count_after_first, count_after_second
    );
}

#[test]
fn test_simplefin_rapid_sync_no_duplicates() {
    // This test runs multiple syncs rapidly to stress-test the deduplication
    let access_url = match std::env::var("SIMPLEFIN_ACCESS_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("SIMPLEFIN_ACCESS_URL not set, skipping SimpleFin rapid test");
            return;
        }
    };

    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let sync_service = SyncService::new(repo.clone(), treeline_dir.clone());

    // Set up SimpleFin integration directly with access URL
    repo.upsert_integration(
        "simplefin",
        &json!({ "accessUrl": access_url }),
    )
    .expect("Failed to setup SimpleFin integration");

    // Run 5 syncs rapidly
    for i in 0..5 {
        let result = sync_service
            .sync(Some("simplefin"), false, false)
            .expect(&format!("Sync {} failed", i));

        println!(
            "Sync {}: {} new, {} skipped",
            i,
            result.results[0].transaction_stats.new,
            result.results[0].transaction_stats.skipped
        );
    }

    // Verify no duplicates
    let duplicates = repo.check_duplicate_sf_ids().expect("Check failed");
    assert!(
        duplicates.is_empty(),
        "Should have no duplicate sf_ids after rapid syncs. Found: {:?}",
        duplicates
    );

    let total = repo.get_transaction_count().expect("Failed to get count");
    println!("Total transactions after 5 syncs: {}", total);
}

// =============================================================================
// Lunchflow Integration Tests (requires LUNCHFLOW_API_KEY env var)
// =============================================================================

#[test]
fn test_lunchflow_sync_no_duplicates() {
    let api_key = match std::env::var("LUNCHFLOW_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("LUNCHFLOW_API_KEY not set, skipping Lunchflow test");
            return;
        }
    };

    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let sync_service = SyncService::new(repo.clone(), treeline_dir.clone());

    // Set up Lunchflow integration
    sync_service
        .setup_lunchflow(&api_key, None)
        .expect("Failed to setup Lunchflow");

    // First sync
    let result1 = sync_service
        .sync(Some("lunchflow"), false, false)
        .expect("First sync failed");

    let first_sync_new = result1.results[0].transaction_stats.new;
    println!("First Lunchflow sync: {} new transactions", first_sync_new);

    // Second sync
    let result2 = sync_service
        .sync(Some("lunchflow"), false, false)
        .expect("Second sync failed");

    let second_sync_new = result2.results[0].transaction_stats.new;
    let second_sync_skipped = result2.results[0].transaction_stats.skipped;
    println!(
        "Second Lunchflow sync: {} new, {} skipped",
        second_sync_new, second_sync_skipped
    );

    // Check for duplicates
    let duplicates = repo.check_duplicate_lf_ids().expect("Check failed");
    assert!(
        duplicates.is_empty(),
        "Should have no duplicate lf_ids after two syncs. Found: {:?}",
        duplicates
    );
}

#[test]
fn test_lunchflow_rapid_sync_no_duplicates() {
    let api_key = match std::env::var("LUNCHFLOW_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("LUNCHFLOW_API_KEY not set, skipping Lunchflow rapid test");
            return;
        }
    };

    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let sync_service = SyncService::new(repo.clone(), treeline_dir.clone());

    sync_service
        .setup_lunchflow(&api_key, None)
        .expect("Failed to setup Lunchflow");

    // Run 5 syncs rapidly
    for i in 0..5 {
        let result = sync_service
            .sync(Some("lunchflow"), false, false)
            .expect(&format!("Lunchflow sync {} failed", i));

        println!(
            "Lunchflow sync {}: {} new, {} skipped",
            i,
            result.results[0].transaction_stats.new,
            result.results[0].transaction_stats.skipped
        );
    }

    // Verify no duplicates
    let duplicates = repo.check_duplicate_lf_ids().expect("Check failed");
    assert!(
        duplicates.is_empty(),
        "Should have no duplicate lf_ids after rapid syncs. Found: {:?}",
        duplicates
    );
}

// =============================================================================
// Import Profile Persistence Tests
// =============================================================================

#[test]
fn test_save_profile_preserves_skip_rows() {
    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let import_service = ImportService::new(repo.clone(), treeline_dir.clone());

    let mappings = ColumnMappings {
        date: "Date".to_string(),
        amount: "Amount".to_string(),
        description: Some("Description".to_string()),
        debit: None,
        credit: None,
        balance: None,
    };

    let options = ImportOptions {
        skip_rows: 3,
        ..ImportOptions::default()
    };

    import_service
        .save_profile("test_profile", &mappings, &options)
        .expect("Save profile failed");

    let profile = import_service
        .get_profile("test_profile")
        .expect("Get profile failed")
        .expect("Profile not found");

    assert_eq!(profile.skip_rows, 3, "skip_rows should be preserved as 3");
}

#[test]
fn test_save_profile_preserves_number_format() {
    use treeline_core::services::NumberFormat;

    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let import_service = ImportService::new(repo.clone(), treeline_dir.clone());

    let mappings = ColumnMappings {
        date: "Date".to_string(),
        amount: "Amount".to_string(),
        description: Some("Description".to_string()),
        debit: None,
        credit: None,
        balance: None,
    };

    let options = ImportOptions {
        number_format: NumberFormat::Eu,
        ..ImportOptions::default()
    };

    import_service
        .save_profile("eu_profile", &mappings, &options)
        .expect("Save profile failed");

    let profile = import_service
        .get_profile("eu_profile")
        .expect("Get profile failed")
        .expect("Profile not found");

    assert_eq!(
        profile.options.number_format,
        Some("eu".to_string()),
        "number_format should be persisted as 'eu'"
    );
}

// =============================================================================
// Count-based CSV Fingerprint Deduplication Tests
// =============================================================================

#[test]
fn test_csv_import_identical_rows_all_imported() {
    // 3 identical CSV rows should all be imported on first import
    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Checking");

    let import_service = ImportService::new(repo.clone(), treeline_dir.clone());

    // CSV with 3 identical rows
    let csv_content = r#"Date,Amount,Description
2024-01-15,-25.50,Coffee Shop
2024-01-15,-25.50,Coffee Shop
2024-01-15,-25.50,Coffee Shop"#;

    let csv_path = create_csv_file(&treeline_dir, "identical.csv", csv_content);

    let mappings = ColumnMappings {
        date: "Date".to_string(),
        amount: "Amount".to_string(),
        description: Some("Description".to_string()),
        debit: None,
        credit: None,
        balance: None,
    };
    let options = ImportOptions::default();

    let result = import_service
        .import(&csv_path, &account_id.to_string(), &mappings, &options, false)
        .expect("Import failed");

    assert_eq!(result.imported, 3, "All 3 identical rows should be imported");

    let count = repo.get_transaction_count().expect("Failed to get count");
    assert_eq!(count, 3, "DB should have 3 transactions");
}

#[test]
fn test_csv_import_reimport_after_delete() {
    // Import 3 identical, delete 2, re-import → 2 re-added (3 total)
    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Checking");

    let import_service = ImportService::new(repo.clone(), treeline_dir.clone());

    let csv_content = r#"Date,Amount,Description
2024-01-15,-25.50,Coffee Shop
2024-01-15,-25.50,Coffee Shop
2024-01-15,-25.50,Coffee Shop"#;

    let csv_path = create_csv_file(&treeline_dir, "reimport.csv", csv_content);

    let mappings = ColumnMappings {
        date: "Date".to_string(),
        amount: "Amount".to_string(),
        description: Some("Description".to_string()),
        debit: None,
        credit: None,
        balance: None,
    };
    let options = ImportOptions::default();

    // First import: all 3
    let result1 = import_service
        .import(&csv_path, &account_id.to_string(), &mappings, &options, false)
        .expect("First import failed");
    assert_eq!(result1.imported, 3);

    // Delete 2 of the 3 (keep 1)
    // Use execute_sql to delete by LIMIT
    repo.execute_sql(
        "DELETE FROM sys_transactions WHERE transaction_id IN (SELECT transaction_id FROM sys_transactions ORDER BY transaction_id LIMIT 2)",
    )
    .expect("Delete failed");

    let count_after_delete = repo.get_transaction_count().expect("Failed to get count");
    assert_eq!(count_after_delete, 1, "Should have 1 remaining after deleting 2");

    // Re-import same CSV: should add 2 back (csv has 3, db has 1 → import 2)
    let result2 = import_service
        .import(&csv_path, &account_id.to_string(), &mappings, &options, false)
        .expect("Re-import failed");

    assert_eq!(result2.imported, 2, "Should re-import 2 deleted transactions");
    assert_eq!(result2.skipped, 1, "Should skip 1 that still exists");

    let final_count = repo.get_transaction_count().expect("Failed to get count");
    assert_eq!(final_count, 3, "Should have 3 total after re-import");
}

#[test]
fn test_csv_import_reimport_no_delete() {
    // Import 3 identical, re-import → 0 added (still 3)
    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Checking");

    let import_service = ImportService::new(repo.clone(), treeline_dir.clone());

    let csv_content = r#"Date,Amount,Description
2024-01-15,-25.50,Coffee Shop
2024-01-15,-25.50,Coffee Shop
2024-01-15,-25.50,Coffee Shop"#;

    let csv_path = create_csv_file(&treeline_dir, "no_delete.csv", csv_content);

    let mappings = ColumnMappings {
        date: "Date".to_string(),
        amount: "Amount".to_string(),
        description: Some("Description".to_string()),
        debit: None,
        credit: None,
        balance: None,
    };
    let options = ImportOptions::default();

    // First import
    let result1 = import_service
        .import(&csv_path, &account_id.to_string(), &mappings, &options, false)
        .expect("First import failed");
    assert_eq!(result1.imported, 3);

    // Re-import without deleting anything
    let result2 = import_service
        .import(&csv_path, &account_id.to_string(), &mappings, &options, false)
        .expect("Re-import failed");

    assert_eq!(result2.imported, 0, "Should import 0 on re-import");
    assert_eq!(result2.skipped, 3, "Should skip all 3 as duplicates");

    let count = repo.get_transaction_count().expect("Failed to get count");
    assert_eq!(count, 3, "Should still have 3 total");
}

#[test]
fn test_csv_fingerprint_counts() {
    // Test the get_csv_fingerprint_counts method directly
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Insert transactions with duplicate fingerprints
    let mut transactions: Vec<Transaction> = Vec::new();
    for i in 0..3 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new(100, 2),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        );
        tx.csv_fingerprint = Some("same_fp".to_string());
        tx.csv_batch_id = Some("batch_1".to_string());
        tx.description = Some(format!("TX {}", i));
        transactions.push(tx);
    }

    // Add one with a different fingerprint
    let mut unique_tx = Transaction::new(
        Uuid::new_v4(),
        account_id,
        Decimal::new(200, 2),
        NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
    );
    unique_tx.csv_fingerprint = Some("unique_fp".to_string());
    unique_tx.csv_batch_id = Some("batch_1".to_string());
    transactions.push(unique_tx);

    repo.bulk_insert_transactions(&transactions).expect("Insert failed");

    let counts = repo
        .get_csv_fingerprint_counts(&["same_fp".to_string(), "unique_fp".to_string(), "missing_fp".to_string()])
        .expect("Count query failed");

    assert_eq!(counts.get("same_fp"), Some(&3), "same_fp should have count 3");
    assert_eq!(counts.get("unique_fp"), Some(&1), "unique_fp should have count 1");
    assert_eq!(counts.get("missing_fp"), None, "missing_fp should not be present");
}

// =============================================================================
// Combined Provider Tests
// =============================================================================

#[test]
fn test_get_existing_csv_fingerprints() {
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Insert some transactions with csv_fingerprints
    let mut transactions: Vec<Transaction> = Vec::new();
    for i in 0..10 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new(100, 2),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        );
        tx.csv_fingerprint = Some(format!("existing_fp_{}", i));
        tx.csv_batch_id = Some("batch_1".to_string());
        transactions.push(tx);
    }
    repo.bulk_insert_transactions(&transactions)
        .expect("Insert failed");

    // Query for a mix of existing and non-existing fingerprints
    let query_fps: Vec<String> = (0..15)
        .map(|i| {
            if i < 10 {
                format!("existing_fp_{}", i) // These exist
            } else {
                format!("new_fp_{}", i) // These don't exist
            }
        })
        .collect();

    let existing = repo
        .get_existing_csv_fingerprints(&query_fps)
        .expect("Query failed");

    assert_eq!(existing.len(), 10, "Should find 10 existing fingerprints");

    // Verify the right ones are found
    for i in 0..10 {
        assert!(
            existing.contains(&format!("existing_fp_{}", i)),
            "Should contain existing_fp_{}",
            i
        );
    }
    for i in 10..15 {
        assert!(
            !existing.contains(&format!("new_fp_{}", i)),
            "Should NOT contain new_fp_{}",
            i
        );
    }

    // Test with empty input
    let empty_result = repo
        .get_existing_csv_fingerprints(&[])
        .expect("Empty query failed");
    assert_eq!(empty_result.len(), 0);
}

#[test]
fn test_get_existing_csv_fingerprints_large_batch() {
    // Test the chunking logic (chunks of 500)
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Insert 600 transactions with fingerprints (crosses the 500-chunk boundary)
    let mut transactions: Vec<Transaction> = Vec::new();
    for i in 0..600 {
        let mut tx = Transaction::new(
            Uuid::new_v4(),
            account_id,
            Decimal::new((i + 1) * 10, 2),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
                + chrono::Duration::days(i % 365),
        );
        tx.csv_fingerprint = Some(format!("fp_{:04}", i));
        tx.csv_batch_id = Some("batch_large".to_string());
        transactions.push(tx);
    }
    repo.bulk_insert_transactions(&transactions)
        .expect("Insert failed");

    // Query all 600 plus 100 new ones
    let query_fps: Vec<String> = (0..700)
        .map(|i| format!("fp_{:04}", i))
        .collect();

    let existing = repo
        .get_existing_csv_fingerprints(&query_fps)
        .expect("Query failed");

    assert_eq!(
        existing.len(),
        600,
        "Should find all 600 existing fingerprints across chunk boundary"
    );
}

#[test]
fn test_bulk_insert_balance_snapshots() {
    use treeline_core::domain::BalanceSnapshot;

    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Account");

    // Create balance snapshots for 30 days
    let mut snapshots: Vec<BalanceSnapshot> = Vec::new();
    for i in 0..30 {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            + chrono::Duration::days(i);
        let snapshot_time = chrono::NaiveDateTime::new(
            date,
            chrono::NaiveTime::from_hms_micro_opt(23, 59, 59, 999999).unwrap(),
        );
        snapshots.push(BalanceSnapshot {
            id: Uuid::new_v4(),
            account_id,
            balance: Decimal::new(100000 + i * 100, 2), // $1000 + $1/day
            snapshot_time,
            source: Some("csv_import".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
    }

    // Bulk insert
    let count = repo
        .bulk_insert_balance_snapshots(&snapshots)
        .expect("Bulk snapshot insert failed");
    assert_eq!(count, 30, "Should insert 30 snapshots");

    // Verify they exist
    let stored = repo
        .get_balance_snapshots(Some(&account_id.to_string()))
        .expect("Get snapshots failed");
    assert_eq!(stored.len(), 30, "Should have 30 snapshots stored");

    // Test with empty input
    let empty_count = repo
        .bulk_insert_balance_snapshots(&[])
        .expect("Empty insert failed");
    assert_eq!(empty_count, 0);
}

#[test]
fn test_csv_import_with_balance_column() {
    let (_temp_dir, repo, treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Test Checking");

    let import_service = ImportService::new(repo.clone(), treeline_dir.clone());

    // CSV with a balance column
    let csv_content = r#"Date,Amount,Description,Balance
2024-01-15,100.00,Paycheck,1100.00
2024-01-16,-25.50,Grocery Store,1074.50
2024-01-17,-15.00,Coffee Shop,1059.50
2024-01-18,50.00,Refund,1109.50
2024-01-19,-200.00,Rent Payment,909.50"#;

    let csv_path = create_csv_file(&treeline_dir, "balance_import.csv", csv_content);

    let mappings = ColumnMappings {
        date: "Date".to_string(),
        amount: "Amount".to_string(),
        description: Some("Description".to_string()),
        debit: None,
        credit: None,
        balance: Some("Balance".to_string()),
    };
    let options = ImportOptions::default();

    let result = import_service
        .import(
            &csv_path,
            &account_id.to_string(),
            &mappings,
            &options,
            false,
        )
        .expect("Import with balance column failed");

    assert_eq!(result.imported, 5, "Should import 5 transactions");
    assert!(
        result.balance_snapshots_created > 0,
        "Should create balance snapshots (got {})",
        result.balance_snapshots_created
    );

    // Verify snapshots were stored
    let snapshots = repo
        .get_balance_snapshots(Some(&account_id.to_string()))
        .expect("Get snapshots failed");
    assert_eq!(
        snapshots.len() as i64,
        result.balance_snapshots_created,
        "Stored snapshot count should match result"
    );
}

#[test]
fn test_multiple_providers_no_cross_contamination() {
    // Test that transactions from different providers don't interfere
    let (_temp_dir, repo, _treeline_dir) = setup_test_env();
    let account_id = create_test_account(&repo, "Multi-Provider Account");

    // Insert SimpleFin transactions
    let sf_transactions: Vec<Transaction> = (0..10)
        .map(|i| {
            let mut tx = Transaction::new(
                Uuid::new_v4(),
                account_id,
                Decimal::new(100, 2),
                NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            );
            tx.sf_id = Some(format!("sf_{}", i));
            tx.description = Some(format!("SimpleFin tx {}", i));
            tx
        })
        .collect();

    // Insert Lunchflow transactions
    let lf_transactions: Vec<Transaction> = (0..10)
        .map(|i| {
            let mut tx = Transaction::new(
                Uuid::new_v4(),
                account_id,
                Decimal::new(200, 2),
                NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            );
            tx.lf_id = Some(format!("lf_{}", i));
            tx.description = Some(format!("Lunchflow tx {}", i));
            tx
        })
        .collect();

    // Insert CSV transactions
    let csv_transactions: Vec<Transaction> = (0..10)
        .map(|i| {
            let mut tx = Transaction::new(
                Uuid::new_v4(),
                account_id,
                Decimal::new(300, 2),
                NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            );
            tx.csv_fingerprint = Some(format!("csv_fp_{}", i));
            tx.csv_batch_id = Some("test_batch".to_string());
            tx.description = Some(format!("CSV tx {}", i));
            tx
        })
        .collect();

    repo.bulk_insert_transactions(&sf_transactions).expect("SF insert failed");
    repo.bulk_insert_transactions(&lf_transactions).expect("LF insert failed");
    repo.bulk_insert_transactions(&csv_transactions).expect("CSV insert failed");

    // Verify counts
    let total = repo.get_transaction_count().expect("Count failed");
    assert_eq!(total, 30, "Should have 30 total transactions");

    // Verify no cross-provider duplicates
    let sf_ids: Vec<String> = (0..10).map(|i| format!("sf_{}", i)).collect();
    let lf_ids: Vec<String> = (0..10).map(|i| format!("lf_{}", i)).collect();

    let existing_sf = repo.get_existing_sf_ids(&sf_ids).expect("Query failed");
    let existing_lf = repo.get_existing_lf_ids(&lf_ids).expect("Query failed");

    assert_eq!(existing_sf.len(), 10);
    assert_eq!(existing_lf.len(), 10);

    // Verify no duplicates
    assert!(repo.check_duplicate_sf_ids().expect("Check failed").is_empty());
    assert!(repo.check_duplicate_lf_ids().expect("Check failed").is_empty());
}
