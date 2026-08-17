//! Tests for plugin-published doctor checks.
//!
//! A plugin's headless surface is its schema: if it owns a view
//! `plugin_<id>.doctor`, core reads it and turns each row into a check keyed
//! `<plugin_id>.<check_id>`. No plugin code ever runs.
//!
//! Run with: cargo test --test doctor_tests

use std::fs;
use std::sync::Arc;

use tempfile::TempDir;

use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::services::{BackupService, DoctorService, EncryptionService};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a repository inside a temp treeline dir, with schema initialized
fn create_test_repo(temp_dir: &TempDir) -> Arc<DuckDbRepository> {
    let db_path = temp_dir.path().join("treeline.duckdb");
    let repo = DuckDbRepository::new(&db_path, None).expect("Failed to create repository");
    repo.ensure_schema().expect("Failed to initialize schema");
    Arc::new(repo)
}

/// Install a fake plugin by writing only its manifest - the CLI installs
/// plugins by copying files, so a manifest is all core ever sees.
fn install_fake_plugin(temp_dir: &TempDir, id: &str, schema_name: &str) {
    let plugin_dir = temp_dir.path().join("plugins").join(id);
    fs::create_dir_all(&plugin_dir).unwrap();
    let manifest = serde_json::json!({
        "id": id,
        "name": "Fake Plugin",
        "version": "0.1.0",
        "permissions": {
            "read": ["transactions"],
            "schemaName": schema_name
        }
    });
    fs::write(
        plugin_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn exec(repo: &DuckDbRepository, sql: &str) {
    repo.execute_sql(sql)
        .unwrap_or_else(|e| panic!("failed to execute {sql}: {e}"));
}

// ============================================================================
// Tests
// ============================================================================

/// A doctor view's rows become checks, keyed `<plugin_id>.<check_id>`.
#[test]
fn test_plugin_doctor_view_rows_become_checks() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    install_fake_plugin(&temp_dir, "fake", "plugin_fake");

    exec(&repo, "CREATE SCHEMA plugin_fake");
    exec(
        &repo,
        "CREATE VIEW plugin_fake.doctor AS
         SELECT 'all_good' AS check_id,
                'Everything fine' AS name,
                'pass' AS status,
                'Nothing to report' AS message,
                NULL AS details
         UNION ALL
         SELECT 'double_counted',
                'Double counted transactions',
                'warning',
                '2 transaction(s) counted in more than one category',
                [{'transaction_id': 'tx-1', 'amount': 12.5},
                 {'transaction_id': 'tx-2', 'amount': 3.0}]",
    );

    let doctor = DoctorService::new(Arc::clone(&repo), temp_dir.path().to_path_buf());
    let result = doctor.run_checks().unwrap();

    let pass = result
        .checks
        .get("fake.all_good")
        .expect("missing fake.all_good");
    assert_eq!(pass.status, "pass");
    assert_eq!(pass.message, "Nothing to report");
    assert_eq!(pass.name.as_deref(), Some("Everything fine"));
    assert!(pass.details.is_none());

    let warning = result
        .checks
        .get("fake.double_counted")
        .expect("missing fake.double_counted");
    assert_eq!(warning.status, "warning");
    assert_eq!(
        warning.message,
        "2 transaction(s) counted in more than one category"
    );
    assert_eq!(warning.name.as_deref(), Some("Double counted transactions"));

    let details = warning.details.as_ref().expect("missing details");
    assert_eq!(details.len(), 2);
    assert_eq!(details[0]["transaction_id"], "tx-1");
    assert_eq!(details[0]["amount"], 12.5);
    assert_eq!(details[1]["transaction_id"], "tx-2");

    // Built-in checks still run, and the summary counts plugin checks
    assert!(result.checks.contains_key("orphaned_transactions"));
    assert!(
        result.summary.warnings >= 1,
        "plugin warning must be counted in the summary"
    );

    // The hardcoded budget stubs are gone - plugins publish their own checks
    assert!(!result.checks.contains_key("budget_double_counting"));
    assert!(!result.checks.contains_key("uncategorized_expenses"));
}

/// Installed but never opened in the desktop app: no schema, so no migrations
/// have run.
#[test]
fn test_plugin_without_schema_reports_not_initialized() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    install_fake_plugin(&temp_dir, "fake", "plugin_fake");

    let doctor = DoctorService::new(Arc::clone(&repo), temp_dir.path().to_path_buf());
    let result = doctor.run_checks().unwrap();

    let check = result
        .checks
        .get("fake.initialized")
        .expect("missing fake.initialized");
    assert_eq!(check.status, "warning");
    assert!(
        check.message.contains("not initialized"),
        "unexpected message: {}",
        check.message
    );
}

/// A schema without a doctor view contributes nothing - the convention is
/// opt-in.
#[test]
fn test_plugin_without_doctor_view_adds_nothing() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    install_fake_plugin(&temp_dir, "fake", "plugin_fake");

    exec(&repo, "CREATE SCHEMA plugin_fake");
    exec(&repo, "CREATE TABLE plugin_fake.items (id VARCHAR)");

    let doctor = DoctorService::new(Arc::clone(&repo), temp_dir.path().to_path_buf());
    let result = doctor.run_checks().unwrap();

    assert!(!result.checks.keys().any(|k| k.starts_with("fake.")));
    assert!(result.checks.contains_key("orphaned_transactions"));
}

/// A doctor view that fails at query time is contained to one error check.
#[test]
fn test_failing_doctor_view_is_contained() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    install_fake_plugin(&temp_dir, "fake", "plugin_fake");

    exec(&repo, "CREATE SCHEMA plugin_fake");
    exec(&repo, "CREATE TABLE plugin_fake.src (check_id VARCHAR)");
    exec(
        &repo,
        "CREATE VIEW plugin_fake.doctor AS SELECT * FROM plugin_fake.src",
    );
    // View survives, but selecting from it now fails
    exec(&repo, "DROP TABLE plugin_fake.src");

    let doctor = DoctorService::new(Arc::clone(&repo), temp_dir.path().to_path_buf());
    let result = doctor.run_checks().unwrap();

    let check = result
        .checks
        .get("fake.doctor")
        .expect("missing fake.doctor");
    assert_eq!(check.status, "error");
    assert!(
        check.message.starts_with("doctor view failed:"),
        "unexpected message: {}",
        check.message
    );
    assert!(
        !check.message.contains('\n'),
        "error text must be collapsed to one line"
    );

    // Every other check still ran
    assert!(result.checks.contains_key("orphaned_transactions"));
    assert!(result.checks.contains_key("date_sanity"));
    assert!(result.summary.errors >= 1);
}

/// Unknown status values become errors, and details are capped.
#[test]
fn test_unknown_status_and_details_cap() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    install_fake_plugin(&temp_dir, "fake", "plugin_fake");

    exec(&repo, "CREATE SCHEMA plugin_fake");
    exec(
        &repo,
        "CREATE VIEW plugin_fake.doctor AS
         SELECT 'weird' AS check_id,
                'Weird status' AS name,
                'catastrophe' AS status,
                'should not be trusted' AS message,
                NULL AS details
         UNION ALL
         SELECT 'many_details',
                'Many details',
                'warning',
                '60 rows',
                (SELECT list({'i': i}) FROM range(60) t(i))",
    );

    let doctor = DoctorService::new(Arc::clone(&repo), temp_dir.path().to_path_buf());
    let result = doctor.run_checks().unwrap();

    let weird = result.checks.get("fake.weird").expect("missing fake.weird");
    assert_eq!(weird.status, "error");
    assert!(
        weird.message.contains("catastrophe"),
        "unexpected message: {}",
        weird.message
    );

    let many = result
        .checks
        .get("fake.many_details")
        .expect("missing fake.many_details");
    assert_eq!(many.details.as_ref().unwrap().len(), 50);
}

/// The `details` column is optional.
#[test]
fn test_doctor_view_without_details_column() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);
    install_fake_plugin(&temp_dir, "fake", "plugin_fake");

    exec(&repo, "CREATE SCHEMA plugin_fake");
    exec(
        &repo,
        "CREATE VIEW plugin_fake.doctor AS
         SELECT 'minimal' AS check_id, 'Minimal' AS name, 'pass' AS status, 'fine' AS message",
    );

    let doctor = DoctorService::new(Arc::clone(&repo), temp_dir.path().to_path_buf());
    let result = doctor.run_checks().unwrap();

    let check = result
        .checks
        .get("fake.minimal")
        .expect("missing fake.minimal");
    assert_eq!(check.status, "pass");
    assert!(check.details.is_none());
}

/// Discovery works the same on an encrypted database, which reaches the
/// schema through an in-memory connection plus ATTACH.
#[test]
fn test_plugin_doctor_view_on_encrypted_db() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("treeline.duckdb");
    install_fake_plugin(&temp_dir, "fake", "plugin_fake");

    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
        exec(&repo, "CREATE SCHEMA plugin_fake");
        exec(
            &repo,
            "CREATE VIEW plugin_fake.doctor AS
             SELECT 'encrypted_ok' AS check_id, 'Encrypted' AS name, 'pass' AS status,
                    'fine' AS message, [{'k': 'v'}] AS details",
        );
    }

    let encryption_service = EncryptionService::new(temp_dir.path().to_path_buf(), db_path.clone());
    let backup_service =
        BackupService::new(temp_dir.path().to_path_buf(), "treeline.duckdb".to_string());
    encryption_service
        .encrypt("doctor-test-pwd", &backup_service)
        .unwrap();
    let key_hex = encryption_service
        .derive_key_for_connection("doctor-test-pwd")
        .unwrap();

    let repo = Arc::new(DuckDbRepository::new(&db_path, Some(&key_hex)).unwrap());
    let doctor = DoctorService::new(Arc::clone(&repo), temp_dir.path().to_path_buf());
    let result = doctor.run_checks().unwrap();

    let check = result
        .checks
        .get("fake.encrypted_ok")
        .expect("missing fake.encrypted_ok");
    assert_eq!(check.status, "pass");
    assert_eq!(check.details.as_ref().unwrap()[0]["k"], "v");
}

/// Schema name falls back to `plugin_<id>` when the manifest omits it, with
/// hyphens converted to underscores (same rule the desktop uses).
#[test]
fn test_schema_name_derived_from_plugin_id() {
    let temp_dir = TempDir::new().unwrap();
    let repo = create_test_repo(&temp_dir);

    let plugin_dir = temp_dir.path().join("plugins").join("my-plugin");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join("manifest.json"),
        r#"{"id": "my-plugin", "name": "My Plugin"}"#,
    )
    .unwrap();

    exec(&repo, "CREATE SCHEMA plugin_my_plugin");
    exec(
        &repo,
        "CREATE VIEW plugin_my_plugin.doctor AS
         SELECT 'ok' AS check_id, 'OK' AS name, 'pass' AS status, 'fine' AS message",
    );

    let doctor = DoctorService::new(Arc::clone(&repo), temp_dir.path().to_path_buf());
    let result = doctor.run_checks().unwrap();

    assert!(result.checks.contains_key("my-plugin.ok"));
}
