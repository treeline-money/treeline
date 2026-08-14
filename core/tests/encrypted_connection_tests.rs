//! Encrypted database connection tests
//!
//! Tests for opening encrypted DuckDB databases with correct/wrong keys,
//! read-only vs read-write access, and data integrity through encryption.
//!
//! Run with: cargo test --test encrypted_connection_tests -- --nocapture

use tempfile::TempDir;

use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::services::{BackupService, EncryptionService};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create an encrypted database and return (temp_dir, db_path, key_hex).
fn create_encrypted_db() -> (TempDir, std::path::PathBuf, String) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("treeline.duckdb");
    let password = "connection-test-pwd";

    // Create database with schema and data
    let repo = DuckDbRepository::new(&db_path, None).unwrap();
    repo.ensure_schema().unwrap();
    repo.execute_sql(
        "INSERT INTO sys_accounts (account_id, name, created_at, updated_at) \
         VALUES ('00000000-0000-0000-0000-000000000001', 'Checking', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .unwrap();
    repo.execute_sql(
        "INSERT INTO sys_transactions (transaction_id, account_id, amount, transaction_date, posted_date, created_at, updated_at) \
         VALUES ('00000000-0000-0000-0000-000000000010', '00000000-0000-0000-0000-000000000001', 100.00, '2024-01-15', '2024-01-15', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .unwrap();
    drop(repo);

    // Encrypt
    let encryption_service = EncryptionService::new(temp_dir.path().to_path_buf(), db_path.clone());
    let backup_service =
        BackupService::new(temp_dir.path().to_path_buf(), "treeline.duckdb".to_string());
    encryption_service
        .encrypt(password, &backup_service)
        .unwrap();

    // Derive the key
    let key_hex = encryption_service
        .derive_key_for_connection(password)
        .unwrap();

    (temp_dir, db_path, key_hex)
}

// ============================================================================
// Encrypted Connection Tests
// ============================================================================

#[test]
fn test_open_encrypted_db_with_correct_key() {
    let (_temp_dir, db_path, key_hex) = create_encrypted_db();

    // Open with correct key
    let repo = DuckDbRepository::new(&db_path, Some(&key_hex));
    assert!(
        repo.is_ok(),
        "Should open with correct key: {:?}",
        repo.err()
    );
    let repo = repo.unwrap();

    // Run queries to verify it works
    let result = repo
        .execute_query("SELECT name FROM sys_accounts WHERE account_id = '00000000-0000-0000-0000-000000000001'")
        .unwrap();
    assert_eq!(result.row_count, 1);
    assert_eq!(result.rows[0][0], serde_json::json!("Checking"));
}

#[test]
fn test_open_encrypted_db_with_wrong_key() {
    let (_temp_dir, db_path, _key_hex) = create_encrypted_db();

    // Try to open with a wrong key (valid hex, but not the right key)
    let wrong_key = "0000000000000000000000000000000000000000000000000000000000000000";
    let result = DuckDbRepository::new(&db_path, Some(wrong_key));
    assert!(result.is_err(), "Should fail to open with wrong key");
}

#[test]
fn test_open_encrypted_db_without_key() {
    let (_temp_dir, db_path, _key_hex) = create_encrypted_db();

    // Try to open encrypted database without any key
    let result = DuckDbRepository::new(&db_path, None);
    assert!(
        result.is_err(),
        "Should fail to open encrypted DB without key"
    );
}

#[test]
fn test_open_encrypted_db_readonly() {
    let (_temp_dir, db_path, key_hex) = create_encrypted_db();

    // Open in read-only mode via execute_query_readonly
    let repo = DuckDbRepository::new(&db_path, Some(&key_hex)).unwrap();

    let result = repo.execute_query_readonly("SELECT COUNT(*) AS cnt FROM sys_accounts");
    assert!(
        result.is_ok(),
        "Read-only query should work: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().row_count, 1);
}

#[test]
fn test_encrypted_readonly_rejects_writes_and_host_access() {
    // Regression: the encrypted read-only path opens an in-memory connection
    // and ATTACHes the file READ_ONLY, so writes to the in-memory catalog and
    // filesystem access used to slip through. All of these must be rejected.
    let (temp_dir, db_path, key_hex) = create_encrypted_db();
    let repo = DuckDbRepository::new(&db_path, Some(&key_hex)).unwrap();

    // In-memory catalog write (statement gate).
    assert!(
        repo.execute_query_readonly("CREATE TABLE memory.evil AS SELECT 1")
            .is_err(),
        "in-memory CREATE must be rejected on the read-only path"
    );

    // Reading a host file via a table function inside a SELECT (external
    // access disabled — the statement is a valid read, so only the setting
    // stops it).
    let secret = temp_dir.path().join("secret.txt");
    std::fs::write(&secret, "TOPSECRET").unwrap();
    let read_file = repo.execute_query_readonly(&format!(
        "SELECT content FROM read_text('{}')",
        secret.display()
    ));
    assert!(
        read_file.is_err(),
        "reading a host file must be rejected on the read-only path"
    );

    // Writing a host file via COPY.
    let out = temp_dir.path().join("exfil.csv");
    assert!(
        repo.execute_query_readonly(&format!("COPY (SELECT 1) TO '{}'", out.display()))
            .is_err(),
        "COPY TO a host file must be rejected"
    );
    assert!(!out.exists(), "COPY must not have written a file");

    // A statement hiding behind a CHECKPOINT prefix.
    assert!(
        repo.execute_query_readonly("CHECKPOINT ; CREATE TABLE memory.evil2 AS SELECT 1")
            .is_err(),
        "a statement after CHECKPOINT must not slip through"
    );

    // A legitimate read still works.
    assert!(repo
        .execute_query_readonly("SELECT COUNT(*) FROM sys_accounts")
        .is_ok());
}

#[test]
fn test_open_encrypted_db_readwrite() {
    let (_temp_dir, db_path, key_hex) = create_encrypted_db();

    // Open with write access and insert data
    let repo = DuckDbRepository::new(&db_path, Some(&key_hex)).unwrap();

    let result = repo.execute_sql(
        "INSERT INTO sys_accounts (account_id, name, created_at, updated_at) \
         VALUES ('00000000-0000-0000-0000-000000000002', 'Savings', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    );
    assert!(result.is_ok(), "Write should succeed: {:?}", result.err());

    // Verify the inserted data
    let result = repo
        .execute_query("SELECT COUNT(*) AS cnt FROM sys_accounts")
        .unwrap();
    assert_eq!(result.row_count, 1);
    // Count should be 2 (original + new)
    let count = result.rows[0][0].as_i64().unwrap();
    assert_eq!(count, 2, "Should have 2 accounts after insert");
}

#[test]
fn test_encrypted_db_data_integrity() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("treeline.duckdb");
    let password = "integrity-test";

    // Create database with varied data
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();

        // Multiple accounts
        repo.execute_sql(
            "INSERT INTO sys_accounts (account_id, name, created_at, updated_at) VALUES \
             ('00000000-0000-0000-0000-000000000001', 'Checking', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), \
             ('00000000-0000-0000-0000-000000000002', 'Savings', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), \
             ('00000000-0000-0000-0000-000000000003', 'Credit Card', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .unwrap();

        // Multiple transactions with different amounts
        repo.execute_sql(
            "INSERT INTO sys_transactions (transaction_id, account_id, amount, transaction_date, posted_date, created_at, updated_at) VALUES \
             ('00000000-0000-0000-0000-000000000010', '00000000-0000-0000-0000-000000000001', 1500.75, '2024-01-15', '2024-01-15', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), \
             ('00000000-0000-0000-0000-000000000011', '00000000-0000-0000-0000-000000000001', -42.99, '2024-01-16', '2024-01-16', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), \
             ('00000000-0000-0000-0000-000000000012', '00000000-0000-0000-0000-000000000002', 10000.00, '2024-02-01', '2024-02-01', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), \
             ('00000000-0000-0000-0000-000000000013', '00000000-0000-0000-0000-000000000003', -250.50, '2024-03-10', '2024-03-10', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .unwrap();
    }

    // Encrypt
    let encryption_service = EncryptionService::new(temp_dir.path().to_path_buf(), db_path.clone());
    let backup_service =
        BackupService::new(temp_dir.path().to_path_buf(), "treeline.duckdb".to_string());
    encryption_service
        .encrypt(password, &backup_service)
        .unwrap();

    // Reopen with encryption key and verify all data
    let key_hex = encryption_service
        .derive_key_for_connection(password)
        .unwrap();
    let repo = DuckDbRepository::new(&db_path, Some(&key_hex)).unwrap();

    // Verify account count
    let result = repo
        .execute_query("SELECT COUNT(*) AS cnt FROM sys_accounts")
        .unwrap();
    assert_eq!(
        result.rows[0][0].as_i64().unwrap(),
        3,
        "Should have 3 accounts"
    );

    // Verify transaction count
    let result = repo
        .execute_query("SELECT COUNT(*) AS cnt FROM sys_transactions")
        .unwrap();
    assert_eq!(
        result.rows[0][0].as_i64().unwrap(),
        4,
        "Should have 4 transactions"
    );

    // Verify specific amounts survived encryption
    let result = repo
        .execute_query("SELECT amount FROM sys_transactions WHERE transaction_id = '00000000-0000-0000-0000-000000000010'")
        .unwrap();
    assert_eq!(result.row_count, 1);
    // DuckDB returns DECIMAL as string in JSON
    let amount_str = result.rows[0][0].to_string();
    assert!(
        amount_str.contains("1500.75"),
        "Amount 1500.75 should be preserved, got: {}",
        amount_str
    );

    // Verify negative amount
    let result = repo
        .execute_query("SELECT amount FROM sys_transactions WHERE transaction_id = '00000000-0000-0000-0000-000000000011'")
        .unwrap();
    let amount_str = result.rows[0][0].to_string();
    assert!(
        amount_str.contains("-42.99"),
        "Amount -42.99 should be preserved, got: {}",
        amount_str
    );
}
