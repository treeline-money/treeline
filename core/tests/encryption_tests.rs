//! Encryption service integration tests
//!
//! Tests for EncryptionService: encrypt, decrypt, status, key derivation, error cases.
//! All tests use real DuckDB databases in TempDir (no demo databases).
//!
//! Run with: cargo test --test encryption_tests -- --nocapture

use tempfile::TempDir;

use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::services::{BackupService, EncryptionService};

// ============================================================================
// Test Helpers
// ============================================================================

/// Set up a test environment: creates a TempDir with a real DuckDB database
/// containing schema and sample data. Returns (temp_dir, encryption_service, backup_service).
fn setup_test_env() -> (TempDir, EncryptionService, BackupService) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("treeline.duckdb");

    // Create a real database with schema and some data
    let repo = DuckDbRepository::new(&db_path, None).expect("Failed to create repository");
    repo.ensure_schema().expect("Failed to initialize schema");

    // Insert sample data into base tables (sys_accounts, sys_transactions)
    repo.execute_sql(
        "INSERT INTO sys_accounts (account_id, name, created_at, updated_at) \
         VALUES ('00000000-0000-0000-0000-000000000001', 'Test Checking', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .expect("Failed to insert test account");
    repo.execute_sql(
        "INSERT INTO sys_transactions (transaction_id, account_id, amount, transaction_date, posted_date, created_at, updated_at) \
         VALUES ('00000000-0000-0000-0000-000000000010', '00000000-0000-0000-0000-000000000001', 42.50, '2024-06-15', '2024-06-15', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .expect("Failed to insert test transaction");

    // Drop the repo so the DB file is not locked
    drop(repo);

    let encryption_service = EncryptionService::new(temp_dir.path().to_path_buf(), db_path.clone());
    let backup_service =
        BackupService::new(temp_dir.path().to_path_buf(), "treeline.duckdb".to_string());

    (temp_dir, encryption_service, backup_service)
}

/// Verify that the database contains the expected sample data
fn verify_data_intact(db_path: &std::path::Path, encryption_key: Option<&str>) {
    let repo = DuckDbRepository::new(db_path, encryption_key).expect("Failed to open database");
    let result = repo
        .execute_query("SELECT name FROM sys_accounts WHERE account_id = '00000000-0000-0000-0000-000000000001'")
        .expect("Failed to query accounts");
    assert_eq!(result.row_count, 1, "Should find the test account");
    assert_eq!(
        result.rows[0][0],
        serde_json::json!("Test Checking"),
        "Account name should match"
    );

    let result = repo
        .execute_query("SELECT amount FROM sys_transactions WHERE transaction_id = '00000000-0000-0000-0000-000000000010'")
        .expect("Failed to query transactions");
    assert_eq!(result.row_count, 1, "Should find the test transaction");
}

// ============================================================================
// EncryptionService Tests
// ============================================================================

#[test]
fn test_encrypt_database() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();

    // Before encryption
    assert!(!encryption_service.is_encrypted().unwrap());

    // Encrypt
    let result = encryption_service.encrypt("test-password-123", &backup_service);
    assert!(result.is_ok(), "Encrypt should succeed: {:?}", result.err());
    let result = result.unwrap();
    assert!(result.encrypted, "Result should show encrypted=true");
    assert!(result.backup_name.is_some(), "Should create a backup");

    // After encryption
    assert!(
        encryption_service.is_encrypted().unwrap(),
        "is_encrypted() should return true"
    );

    // Verify encryption.json was created
    let enc_file = _temp_dir.path().join("encryption.json");
    assert!(enc_file.exists(), "encryption.json should be created");
}

#[test]
fn test_decrypt_database() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();
    let db_path = _temp_dir.path().join("treeline.duckdb");
    let password = "decrypt-test-pwd";

    // Encrypt first
    encryption_service
        .encrypt(password, &backup_service)
        .expect("Encrypt should succeed");
    assert!(encryption_service.is_encrypted().unwrap());

    // Verify data accessible with key
    let key_hex = encryption_service
        .derive_key_for_connection(password)
        .unwrap();
    verify_data_intact(&db_path, Some(&key_hex));

    // Decrypt
    let result = encryption_service.decrypt(password, &backup_service);
    assert!(result.is_ok(), "Decrypt should succeed: {:?}", result.err());
    let result = result.unwrap();
    assert!(!result.encrypted, "Result should show encrypted=false");
    assert!(result.backup_name.is_some(), "Should create a backup");

    // After decryption
    assert!(
        !encryption_service.is_encrypted().unwrap(),
        "is_encrypted() should return false"
    );

    // Verify encryption.json was removed
    let enc_file = _temp_dir.path().join("encryption.json");
    assert!(!enc_file.exists(), "encryption.json should be removed");

    // Verify data intact without encryption key
    verify_data_intact(&db_path, None);
}

#[test]
fn test_encrypt_already_encrypted() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();

    // Encrypt once
    encryption_service
        .encrypt("password1", &backup_service)
        .expect("First encrypt should succeed");

    // Try to encrypt again — should fail
    let result = encryption_service.encrypt("password2", &backup_service);
    assert!(result.is_err(), "Should not allow double encryption");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("already encrypted"),
        "Error should mention already encrypted"
    );
}

#[test]
fn test_decrypt_not_encrypted() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();

    // Try to decrypt a non-encrypted database
    let result = encryption_service.decrypt("password", &backup_service);
    assert!(
        result.is_err(),
        "Should not allow decrypting unencrypted DB"
    );
    assert!(
        result.unwrap_err().to_string().contains("not encrypted"),
        "Error should mention not encrypted"
    );
}

#[test]
fn test_derive_key_for_connection() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();
    let db_path = _temp_dir.path().join("treeline.duckdb");
    let password = "key-derivation-test";

    // Encrypt
    encryption_service
        .encrypt(password, &backup_service)
        .unwrap();

    // Derive key
    let key_hex = encryption_service.derive_key_for_connection(password);
    assert!(
        key_hex.is_ok(),
        "Key derivation should succeed: {:?}",
        key_hex.err()
    );
    let key_hex = key_hex.unwrap();

    // Key should be 64 hex chars (32 bytes)
    assert_eq!(key_hex.len(), 64, "Key should be 64 hex chars (256 bits)");

    // Use the derived key to open the encrypted database and run a query
    verify_data_intact(&db_path, Some(&key_hex));
}

#[test]
fn test_derive_key_not_encrypted() {
    let (_temp_dir, encryption_service, _backup_service) = setup_test_env();

    // Try to derive key for non-encrypted database
    let result = encryption_service.derive_key_for_connection("password");
    assert!(result.is_err(), "Should fail for non-encrypted database");
    assert!(result.unwrap_err().to_string().contains("not encrypted"));
}

#[test]
fn test_wrong_password() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();
    let db_path = _temp_dir.path().join("treeline.duckdb");

    // Encrypt with one password
    encryption_service
        .encrypt("correct-password", &backup_service)
        .unwrap();

    // Derive key with wrong password — key derivation itself succeeds
    // (Argon2 will produce a key from any password), but opening the DB should fail
    let wrong_key = encryption_service
        .derive_key_for_connection("wrong-password")
        .unwrap();

    // Attempting to open the database with the wrong key should fail
    let result = DuckDbRepository::new(&db_path, Some(&wrong_key));
    assert!(result.is_err(), "Opening with wrong key should fail");
}

#[test]
fn test_is_encrypted_no_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("treeline.duckdb");

    // Create a database but no encryption.json
    let repo = DuckDbRepository::new(&db_path, None).unwrap();
    repo.ensure_schema().unwrap();
    drop(repo);

    let encryption_service = EncryptionService::new(temp_dir.path().to_path_buf(), db_path);

    assert!(
        !encryption_service.is_encrypted().unwrap(),
        "Should return false when no encryption.json exists"
    );
}

#[test]
fn test_get_status_unencrypted() {
    let (_temp_dir, encryption_service, _backup_service) = setup_test_env();

    let status = encryption_service.get_status().unwrap();
    assert!(!status.encrypted, "Should report not encrypted");
    assert!(status.algorithm.is_none());
    assert!(status.version.is_none());
}

#[test]
fn test_get_status_encrypted() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();

    encryption_service
        .encrypt("status-test", &backup_service)
        .unwrap();

    let status = encryption_service.get_status().unwrap();
    assert!(status.encrypted, "Should report encrypted");
    assert_eq!(status.algorithm, Some("argon2id".to_string()));
    assert_eq!(status.version, Some(1));
}

#[test]
fn test_encrypt_decrypt_roundtrip_data_integrity() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();
    let db_path = _temp_dir.path().join("treeline.duckdb");
    let password = "roundtrip-test";

    // Verify data before encryption
    verify_data_intact(&db_path, None);

    // Encrypt
    encryption_service
        .encrypt(password, &backup_service)
        .unwrap();

    // Verify data after encryption (with key)
    let key_hex = encryption_service
        .derive_key_for_connection(password)
        .unwrap();
    verify_data_intact(&db_path, Some(&key_hex));

    // Decrypt
    encryption_service
        .decrypt(password, &backup_service)
        .unwrap();

    // Verify data after decryption (without key)
    verify_data_intact(&db_path, None);
}

#[test]
fn test_decrypt_wrong_password_fails() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();

    // Encrypt
    encryption_service
        .encrypt("correct-password", &backup_service)
        .unwrap();

    // Try to decrypt with wrong password — should fail with "Invalid password"
    let result = encryption_service.decrypt("wrong-password", &backup_service);
    assert!(result.is_err(), "Decrypt with wrong password should fail");
    assert!(
        result.unwrap_err().to_string().contains("Invalid password"),
        "Error should mention invalid password"
    );
}

#[test]
fn test_key_derivation_deterministic() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();
    let password = "deterministic-test";

    encryption_service
        .encrypt(password, &backup_service)
        .unwrap();

    // Derive key twice with the same password — should produce identical keys
    let key1 = encryption_service
        .derive_key_for_connection(password)
        .unwrap();
    let key2 = encryption_service
        .derive_key_for_connection(password)
        .unwrap();

    assert_eq!(key1, key2, "Same password should derive same key");
}

#[test]
fn test_encrypt_creates_backup() {
    let (_temp_dir, encryption_service, backup_service) = setup_test_env();

    let result = encryption_service
        .encrypt("backup-test", &backup_service)
        .unwrap();

    assert!(result.backup_name.is_some());
    let backup_name = result.backup_name.unwrap();
    assert!(backup_name.ends_with(".zip"), "Backup should be a zip file");

    // Verify backup file exists
    let backups_dir = _temp_dir.path().join("backups");
    assert!(backups_dir.exists(), "Backups directory should exist");
}
