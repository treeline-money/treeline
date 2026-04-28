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

// Env-var tests mutate process-global state. Hold this mutex so they
// can't race each other when cargo runs tests in parallel.
static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_token_loaded_from_env_var() {
    let _g = ENV_GUARD.lock().unwrap();
    let temp_dir = TempDir::new().unwrap();
    std::env::remove_var("TL_HUB_TOKEN");

    std::env::set_var("TL_HUB_TOKEN", "deadbeef0123");
    let token = HubService::load_or_create_token(temp_dir.path()).unwrap();
    std::env::remove_var("TL_HUB_TOKEN");

    assert_eq!(token, "deadbeef0123");

    // Persisted to disk: subsequent calls without the env var return the same.
    let token_again = HubService::load_or_create_token(temp_dir.path()).unwrap();
    assert_eq!(token_again, "deadbeef0123");
}

#[test]
fn test_env_var_overrides_existing_token_file() {
    let _g = ENV_GUARD.lock().unwrap();
    let temp_dir = TempDir::new().unwrap();
    std::env::remove_var("TL_HUB_TOKEN");

    let original = HubService::load_or_create_token(temp_dir.path()).unwrap();

    std::env::set_var("TL_HUB_TOKEN", "abcd1234");
    let with_env = HubService::load_or_create_token(temp_dir.path()).unwrap();
    std::env::remove_var("TL_HUB_TOKEN");

    assert_ne!(original, with_env);
    assert_eq!(with_env, "abcd1234");
}

#[test]
fn test_empty_env_var_is_ignored() {
    let _g = ENV_GUARD.lock().unwrap();
    let temp_dir = TempDir::new().unwrap();

    std::env::set_var("TL_HUB_TOKEN", "");
    let token = HubService::load_or_create_token(temp_dir.path()).unwrap();
    std::env::remove_var("TL_HUB_TOKEN");

    // Empty env var falls through to the generate-fresh path.
    assert_eq!(token.len(), 64);
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
        base_hash: None,
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

    let source_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&source_dir);
    let bundle = SyncBundle::create(source_dir.path()).unwrap();

    // First push — no base_hash
    let result = hub_service.accept_push(&bundle, None).unwrap();
    match result {
        treeline_core::services::PushOutcome::Accepted { backup_name, bytes_received, new_hash } => {
            assert!(bytes_received > 0);
            assert!(backup_name.is_none()); // No backup on first push
            assert!(!new_hash.is_empty());
        }
        _ => panic!("Expected Accepted, got conflict"),
    }
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

    let result = hub_service.accept_push(&bundle, None).unwrap();
    match result {
        treeline_core::services::PushOutcome::Accepted { backup_name, .. } => {
            assert!(backup_name.is_some());
        }
        _ => panic!("Expected Accepted"),
    }

    let backups_dir = hub_dir.path().join("backups");
    assert!(backups_dir.exists());
}

#[test]
fn test_push_accepted_when_hash_matches() {
    let hub_dir = TempDir::new().unwrap();
    let hub_service = create_hub_service(&hub_dir);

    // First push to establish a hash
    let source_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&source_dir);
    let bundle = SyncBundle::create(source_dir.path()).unwrap();
    let first_hash = match hub_service.accept_push(&bundle, None).unwrap() {
        treeline_core::services::PushOutcome::Accepted { new_hash, .. } => new_hash,
        _ => panic!("Expected Accepted"),
    };

    // Second push with matching base_hash
    let bundle2 = SyncBundle::create(source_dir.path()).unwrap();
    let result = hub_service.accept_push(&bundle2, Some(&first_hash)).unwrap();
    match result {
        treeline_core::services::PushOutcome::Accepted { .. } => {}
        _ => panic!("Expected Accepted when hash matches"),
    }
}

#[test]
fn test_push_conflict_when_hash_mismatches() {
    let hub_dir = TempDir::new().unwrap();
    let hub_service = create_hub_service(&hub_dir);

    // First push
    let source_dir = TempDir::new().unwrap();
    let _repo = create_test_repo(&source_dir);
    let bundle = SyncBundle::create(source_dir.path()).unwrap();
    hub_service.accept_push(&bundle, None).unwrap();

    // Try to push with a stale hash
    let result = hub_service.accept_push(&bundle, Some("stale_hash_abc")).unwrap();
    match result {
        treeline_core::services::PushOutcome::Conflict { hub_hash } => {
            assert!(!hub_hash.is_empty());
        }
        _ => panic!("Expected Conflict when hash mismatches"),
    }
}

#[test]
fn test_hash_changes_after_push() {
    let hub_dir = TempDir::new().unwrap();
    let hub_service = create_hub_service(&hub_dir);

    // Push database A
    let source_a = TempDir::new().unwrap();
    let repo_a = create_test_repo(&source_a);
    let account = treeline_core::Account::new(uuid::Uuid::new_v4(), "Account A".to_string());
    repo_a.upsert_account(&account).unwrap();
    repo_a.checkpoint().unwrap();
    drop(repo_a);
    let bundle_a = SyncBundle::create(source_a.path()).unwrap();
    let hash_a = match hub_service.accept_push(&bundle_a, None).unwrap() {
        treeline_core::services::PushOutcome::Accepted { new_hash, .. } => new_hash,
        _ => panic!("Expected Accepted"),
    };

    // Push database B (different content)
    let source_b = TempDir::new().unwrap();
    let repo_b = create_test_repo(&source_b);
    let account2 = treeline_core::Account::new(uuid::Uuid::new_v4(), "Account B".to_string());
    repo_b.upsert_account(&account2).unwrap();
    repo_b.checkpoint().unwrap();
    drop(repo_b);
    let bundle_b = SyncBundle::create(source_b.path()).unwrap();
    let hash_b = match hub_service.accept_push(&bundle_b, Some(&hash_a)).unwrap() {
        treeline_core::services::PushOutcome::Accepted { new_hash, .. } => new_hash,
        _ => panic!("Expected Accepted"),
    };

    assert_ne!(hash_a, hash_b);
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
    hub_service.accept_push(&bundle, None).unwrap();

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
