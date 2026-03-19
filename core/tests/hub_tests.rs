//! Hub service tests
//!
//! Tests for database push/pull/token management and sync bundles.
//! Run with: cargo test --test hub_tests -- --nocapture

use std::sync::Arc;
use tempfile::TempDir;

use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::config::HubConfig;
use treeline_core::services::hub::{HubService, SyncBundle};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test repository with schema initialized
fn create_test_repo(temp_dir: &TempDir) -> Arc<DuckDbRepository> {
    let db_path = temp_dir.path().join("treeline.duckdb");
    let repo = DuckDbRepository::new(&db_path, None).expect("Failed to create repository");
    repo.ensure_schema().expect("Failed to initialize schema");
    Arc::new(repo)
}

/// Create a hub service for testing
fn create_hub_service(temp_dir: &TempDir) -> HubService {
    HubService::new(
        temp_dir.path().to_path_buf(),
        "treeline.duckdb".to_string(),
    )
}

// ============================================================================
// Token Tests
// ============================================================================

#[test]
fn test_token_generated_on_first_call() {
    let temp_dir = TempDir::new().unwrap();
    let token = HubService::load_or_create_token(temp_dir.path()).unwrap();

    assert!(!token.is_empty());
    assert_eq!(token.len(), 64); // 32 bytes = 64 hex chars
}

#[test]
fn test_token_persisted_across_calls() {
    let temp_dir = TempDir::new().unwrap();
    let token1 = HubService::load_or_create_token(temp_dir.path()).unwrap();
    let token2 = HubService::load_or_create_token(temp_dir.path()).unwrap();

    assert_eq!(token1, token2);
}

#[test]
fn test_token_validation_correct() {
    let temp_dir = TempDir::new().unwrap();
    let token = HubService::load_or_create_token(temp_dir.path()).unwrap();

    assert!(HubService::validate_token(temp_dir.path(), &token).unwrap());
}

#[test]
fn test_token_validation_incorrect() {
    let temp_dir = TempDir::new().unwrap();
    let _token = HubService::load_or_create_token(temp_dir.path()).unwrap();

    assert!(!HubService::validate_token(temp_dir.path(), "wrong-token").unwrap());
}

#[test]
fn test_token_validation_no_token_file() {
    let temp_dir = TempDir::new().unwrap();
    assert!(!HubService::validate_token(temp_dir.path(), "any-token").unwrap());
}

// ============================================================================
// HubConfig Tests
// ============================================================================

#[test]
fn test_hub_config_separate_from_settings() {
    let temp_dir = TempDir::new().unwrap();

    // hub.json doesn't exist by default
    assert!(HubConfig::load(temp_dir.path()).unwrap().is_none());

    // Save hub config
    let hub = HubConfig {
        url: "http://localhost:4242".to_string(),
        token: "abc123".to_string(),
        last_push: None,
        last_pull: None,
    };
    hub.save(temp_dir.path()).unwrap();

    // hub.json exists, settings.json does not
    assert!(temp_dir.path().join("hub.json").exists());
    assert!(!temp_dir.path().join("settings.json").exists());

    // Remove
    HubConfig::remove(temp_dir.path()).unwrap();
    assert!(HubConfig::load(temp_dir.path()).unwrap().is_none());
}

// ============================================================================
// SyncBundle Tests
// ============================================================================

#[test]
fn test_bundle_includes_database() {
    let temp_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&temp_dir);

    let bundle = SyncBundle::create(temp_dir.path()).unwrap();
    assert!(!bundle.is_empty());

    // Extract to a new dir and verify database exists
    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&bundle, dest.path()).unwrap();
    assert!(dest.path().join("treeline.duckdb").exists());
}

#[test]
fn test_bundle_includes_encryption_json() {
    let temp_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&temp_dir);

    // Create encryption.json
    std::fs::write(
        temp_dir.path().join("encryption.json"),
        r#"{"encrypted":true,"salt":"abc"}"#,
    )
    .unwrap();

    let bundle = SyncBundle::create(temp_dir.path()).unwrap();

    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&bundle, dest.path()).unwrap();
    assert!(dest.path().join("encryption.json").exists());
}

#[test]
fn test_bundle_includes_settings() {
    let temp_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&temp_dir);

    std::fs::write(
        temp_dir.path().join("settings.json"),
        r#"{"app":{"demoMode":false}}"#,
    )
    .unwrap();

    let bundle = SyncBundle::create(temp_dir.path()).unwrap();

    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&bundle, dest.path()).unwrap();
    assert!(dest.path().join("settings.json").exists());
}

#[test]
fn test_bundle_includes_skills_dir() {
    let temp_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&temp_dir);

    // Create skills directory with a file
    let skills_dir = temp_dir.path().join("skills").join("my-skill");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("SKILL.md"), "# My Skill").unwrap();

    let bundle = SyncBundle::create(temp_dir.path()).unwrap();

    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&bundle, dest.path()).unwrap();
    assert!(dest.path().join("skills/my-skill/SKILL.md").exists());
}

#[test]
fn test_bundle_includes_plugins_dir() {
    let temp_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&temp_dir);

    // Create plugins directory
    let plugin_dir = temp_dir.path().join("plugins").join("budget");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("index.js"), "// plugin").unwrap();

    let bundle = SyncBundle::create(temp_dir.path()).unwrap();

    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&bundle, dest.path()).unwrap();
    assert!(dest.path().join("plugins/budget/index.js").exists());
}

#[test]
fn test_bundle_excludes_hub_json() {
    let temp_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&temp_dir);

    // Create hub.json (should NOT be in bundle)
    std::fs::write(
        temp_dir.path().join("hub.json"),
        r#"{"url":"http://localhost"}"#,
    )
    .unwrap();

    let bundle = SyncBundle::create(temp_dir.path()).unwrap();

    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&bundle, dest.path()).unwrap();
    assert!(!dest.path().join("hub.json").exists());
}

#[test]
fn test_bundle_excludes_logs() {
    let temp_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&temp_dir);

    std::fs::write(temp_dir.path().join("logs.duckdb"), "fake logs").unwrap();

    let bundle = SyncBundle::create(temp_dir.path()).unwrap();

    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&bundle, dest.path()).unwrap();
    assert!(!dest.path().join("logs.duckdb").exists());
}

// ============================================================================
// Push Tests
// ============================================================================

#[test]
fn test_first_push_to_empty_hub() {
    let hub_dir = TempDir::new().unwrap();
    let hub_service = create_hub_service(&hub_dir);

    // Create a source with a database
    let source_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&source_dir);
    let bundle = SyncBundle::create(source_dir.path()).unwrap();

    let result = hub_service.accept_push(&bundle).unwrap();
    assert!(result.bytes_received > 0);
    assert!(result.backup_name.is_none()); // No backup on first push
    assert!(hub_service.has_database());
}

#[test]
fn test_push_creates_backup_on_existing_hub() {
    let hub_dir = TempDir::new().unwrap();
    let _hub_repo = create_test_repo(&hub_dir);
    let hub_service = create_hub_service(&hub_dir);

    let source_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&source_dir);
    let bundle = SyncBundle::create(source_dir.path()).unwrap();

    let result = hub_service.accept_push(&bundle).unwrap();
    assert!(result.backup_name.is_some());

    let backups_dir = hub_dir.path().join("backups");
    assert!(backups_dir.exists());
}

// ============================================================================
// Pull Tests
// ============================================================================

#[test]
fn test_pull_fails_on_empty_hub() {
    let hub_dir = TempDir::new().unwrap();
    let hub_service = create_hub_service(&hub_dir);

    let result = hub_service.get_bundle_for_pull();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Push a database first"));
}

#[test]
fn test_push_then_pull_roundtrip() {
    let hub_dir = TempDir::new().unwrap();
    let hub_service = create_hub_service(&hub_dir);

    // Create source with database, settings, and skills
    let source_dir = TempDir::new().unwrap();
    let source_path = source_dir.path().join("treeline.duckdb");
    let source_repo = DuckDbRepository::new(&source_path, None).unwrap();
    source_repo.ensure_schema().unwrap();

    let account = treeline_core::Account::new(uuid::Uuid::new_v4(), "Test Account".to_string());
    source_repo.upsert_account(&account).unwrap();
    source_repo.checkpoint().unwrap();
    drop(source_repo);

    std::fs::write(
        source_dir.path().join("settings.json"),
        r#"{"app":{"demoMode":false}}"#,
    )
    .unwrap();
    let skills_dir = source_dir.path().join("skills").join("budgeting");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("SKILL.md"), "# Budget skill").unwrap();

    // Push to hub
    let bundle = SyncBundle::create(source_dir.path()).unwrap();
    hub_service.accept_push(&bundle).unwrap();

    // Pull from hub
    let pulled_bundle = hub_service.get_bundle_for_pull().unwrap();

    // Extract to new device
    let dest_dir = TempDir::new().unwrap();
    SyncBundle::extract(&pulled_bundle, dest_dir.path()).unwrap();

    // Verify everything arrived
    let dest_repo = DuckDbRepository::new(&dest_dir.path().join("treeline.duckdb"), None).unwrap();
    let accounts = dest_repo.get_accounts().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].name, "Test Account");

    assert!(dest_dir.path().join("settings.json").exists());
    assert!(dest_dir.path().join("skills/budgeting/SKILL.md").exists());
}

#[test]
fn test_has_database() {
    let hub_dir = TempDir::new().unwrap();
    let hub_service = create_hub_service(&hub_dir);

    assert!(!hub_service.has_database());

    create_test_repo(&hub_dir);
    assert!(hub_service.has_database());
}
