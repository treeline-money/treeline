//! Hub service tests
//!
//! Tests for database push/pull/token management and sync bundles.
//! Run with: cargo test --test hub_tests -- --nocapture

use std::io::Write;
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
        access_token: "test-access-token".to_string(),
        refresh_token: "test-refresh-token".to_string(),
        device_name: "test-device".to_string(),
        last_push: None,
        last_pull: None,
        base_hash: None,
        link_origin: None,
        extra: Default::default(),
    };
    hub.save(temp_dir.path()).unwrap();

    // hub.json exists, settings.json does not
    assert!(temp_dir.path().join("hub.json").exists());
    assert!(!temp_dir.path().join("settings.json").exists());

    // Remove
    HubConfig::remove(temp_dir.path()).unwrap();
    assert!(HubConfig::load(temp_dir.path()).unwrap().is_none());
}

#[test]
fn test_hub_config_roundtrips_unknown_fields() {
    // Write a hub.json with fields the current `HubConfig` struct doesn't
    // declare (simulating a future build adding new fields). Load + save
    // through the typed struct must preserve them verbatim — that's what
    // keeps a future-feature flag from being silently stripped if a stale
    // build round-trips the file (e.g. on a 401-driven token refresh).
    let temp_dir = TempDir::new().unwrap();
    let raw = r#"{
  "url": "http://example.com:4242",
  "accessToken": "tok",
  "refreshToken": "ref",
  "deviceName": "dev",
  "futureFlag": "yes",
  "futureNested": { "n": 42, "list": [1, 2, 3] }
}"#;
    std::fs::write(temp_dir.path().join("hub.json"), raw).unwrap();

    let loaded = HubConfig::load(temp_dir.path()).unwrap().unwrap();
    loaded.save(temp_dir.path()).unwrap();

    let after = std::fs::read_to_string(temp_dir.path().join("hub.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&after).unwrap();
    assert_eq!(v["futureFlag"], "yes", "unknown scalar must round-trip");
    assert_eq!(v["futureNested"]["n"], 42, "unknown nested object must round-trip");
    assert_eq!(v["futureNested"]["list"][2], 3, "unknown nested array must round-trip");
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

// ============================================================================
// Receive semantics — top-level pass-through, mirror dirs, settings merge
// ============================================================================

/// Build a bundle from a temp dir and return the bytes.
fn bundle_from(dir: &std::path::Path) -> Vec<u8> {
    SyncBundle::create(dir).unwrap()
}

/// Build a bundle that contains only a treeline.duckdb file.
fn bundle_with_only_db() -> Vec<u8> {
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    bundle_from(src.path())
}

#[test]
fn test_extract_preserves_top_level_files_not_in_bundle() {
    // Bundle has only the DB; local already has settings.json and encryption.json.
    // Those locally-present top-level files must survive.
    let bundle = bundle_with_only_db();

    let dest = TempDir::new().unwrap();
    std::fs::write(dest.path().join("settings.json"), r#"{"app":{"theme":"dark"}}"#).unwrap();
    std::fs::write(
        dest.path().join("encryption.json"),
        r#"{"encrypted":false}"#,
    )
    .unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    assert!(dest.path().join("settings.json").exists());
    assert!(dest.path().join("encryption.json").exists());
    assert_eq!(
        std::fs::read_to_string(dest.path().join("settings.json")).unwrap(),
        r#"{"app":{"theme":"dark"}}"#,
        "settings.json should be untouched when the bundle didn't include it"
    );
}

#[test]
fn test_extract_preserves_local_skills_when_bundle_has_no_skills() {
    // Bundle lacks any skills/ entries → local skills must survive.
    let bundle = bundle_with_only_db();

    let dest = TempDir::new().unwrap();
    let local_skill = dest.path().join("skills").join("my-skill");
    std::fs::create_dir_all(&local_skill).unwrap();
    std::fs::write(local_skill.join("SKILL.md"), "# Local").unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    assert!(dest.path().join("skills/my-skill/SKILL.md").exists());
}

#[test]
fn test_extract_clears_skills_dir_when_bundle_has_skills() {
    // Bundle has skills/new-skill; local has skills/old-skill.
    // After extract: only new-skill exists (mirror semantics for skills).
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    let new_skill = src.path().join("skills").join("new-skill");
    std::fs::create_dir_all(&new_skill).unwrap();
    std::fs::write(new_skill.join("SKILL.md"), "# New").unwrap();
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    let old_skill = dest.path().join("skills").join("old-skill");
    std::fs::create_dir_all(&old_skill).unwrap();
    std::fs::write(old_skill.join("SKILL.md"), "# Old").unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    assert!(
        dest.path().join("skills/new-skill/SKILL.md").exists(),
        "new skill should be present"
    );
    assert!(
        !dest.path().join("skills/old-skill").exists(),
        "old skill should be wiped (mirror semantics)"
    );
}

#[test]
fn test_extract_per_plugin_replace_clears_old_files_in_replaced_plugin() {
    // Bundle has plugins/budget/new.js. Local has plugins/budget/old.js.
    // After extract: budget dir replaced — old.js gone, new.js present.
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    let budget = src.path().join("plugins").join("budget");
    std::fs::create_dir_all(&budget).unwrap();
    std::fs::write(budget.join("new.js"), "// new").unwrap();
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    let local_budget = dest.path().join("plugins").join("budget");
    std::fs::create_dir_all(&local_budget).unwrap();
    std::fs::write(local_budget.join("old.js"), "// old").unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    assert!(dest.path().join("plugins/budget/new.js").exists());
    assert!(
        !dest.path().join("plugins/budget/old.js").exists(),
        "old plugin file should be wiped during per-plugin replace"
    );
}

#[test]
fn test_extract_preserves_plugins_not_in_bundle() {
    // Bundle has plugins/budget. Local has plugins/budget AND plugins/goals.
    // After extract: budget replaced from bundle, goals untouched.
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    let budget = src.path().join("plugins").join("budget");
    std::fs::create_dir_all(&budget).unwrap();
    std::fs::write(budget.join("index.js"), "// budget from bundle").unwrap();
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    let local_budget = dest.path().join("plugins").join("budget");
    std::fs::create_dir_all(&local_budget).unwrap();
    std::fs::write(local_budget.join("stale.js"), "// stale").unwrap();
    let local_goals = dest.path().join("plugins").join("goals");
    std::fs::create_dir_all(&local_goals).unwrap();
    std::fs::write(local_goals.join("index.js"), "// local-only goals").unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    assert!(dest.path().join("plugins/budget/index.js").exists());
    assert!(
        !dest.path().join("plugins/budget/stale.js").exists(),
        "old budget file should be wiped"
    );
    assert!(
        dest.path().join("plugins/goals/index.js").exists(),
        "local-only plugin must survive — bundle did not mention goals"
    );
}

#[test]
fn test_extract_settings_shared_path_overrides_local() {
    // Local has currency=USD; bundle has currency=EUR. Currency is shared → bundle wins.
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    std::fs::write(
        src.path().join("settings.json"),
        r#"{"app":{"currency":"EUR"}}"#,
    )
    .unwrap();
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    std::fs::write(
        dest.path().join("settings.json"),
        r#"{"app":{"currency":"USD"}}"#,
    )
    .unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dest.path().join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(merged["app"]["currency"], "EUR");
}

#[test]
fn test_extract_settings_device_path_preserves_local_when_set() {
    // Both sides have a theme; theme is per-device → local wins.
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    std::fs::write(
        src.path().join("settings.json"),
        r#"{"app":{"theme":"light"}}"#,
    )
    .unwrap();
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    std::fs::write(
        dest.path().join("settings.json"),
        r#"{"app":{"theme":"dark"}}"#,
    )
    .unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dest.path().join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        merged["app"]["theme"], "dark",
        "per-device theme must stay local when already set"
    );
}

#[test]
fn test_extract_settings_device_path_bootstraps_when_local_missing() {
    // Local has no theme key. Bundle has theme=light. Bootstrap fills it in.
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    std::fs::write(
        src.path().join("settings.json"),
        r#"{"app":{"theme":"light","currency":"EUR"}}"#,
    )
    .unwrap();
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    std::fs::write(
        dest.path().join("settings.json"),
        r#"{"app":{"currency":"USD"}}"#,
    )
    .unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dest.path().join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(merged["app"]["theme"], "light", "theme bootstrapped from bundle");
    assert_eq!(merged["app"]["currency"], "EUR", "shared currency overridden");
}

#[test]
fn test_extract_settings_bootstraps_entire_file_when_local_missing() {
    // No local settings.json; bundle has one — adopt it wholesale.
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    std::fs::write(
        src.path().join("settings.json"),
        r#"{"app":{"theme":"dark","currency":"USD"}}"#,
    )
    .unwrap();
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&bundle, dest.path()).unwrap();

    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dest.path().join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(written["app"]["theme"], "dark");
    assert_eq!(written["app"]["currency"], "USD");
}

#[test]
fn test_extract_settings_preserves_local_only_plugin_config() {
    // Local has plugins.goals.x=1; bundle has plugins.budget.y=2.
    // Plugins is shared but uses leaf-level merge → both survive.
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    std::fs::write(
        src.path().join("settings.json"),
        r#"{"plugins":{"budget":{"y":2}}}"#,
    )
    .unwrap();
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    std::fs::write(
        dest.path().join("settings.json"),
        r#"{"plugins":{"goals":{"x":1}}}"#,
    )
    .unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dest.path().join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(merged["plugins"]["budget"]["y"], 2, "bundle's plugin config applied");
    assert_eq!(
        merged["plugins"]["goals"]["x"], 1,
        "local-only plugin config must survive — bundle didn't mention goals"
    );
}

#[test]
fn test_extract_rejects_path_traversal() {
    // Manually craft a zip with a `..` entry; verify nothing escapes.
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let mut buf = Vec::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        zip.start_file("../escape.txt", opts).unwrap();
        zip.write_all(b"should not land outside dest").unwrap();
        zip.finish().unwrap();
    }

    let dest_parent = TempDir::new().unwrap();
    let dest = dest_parent.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    SyncBundle::extract(&buf, &dest).unwrap();

    assert!(
        !dest_parent.path().join("escape.txt").exists(),
        "path traversal entry must be rejected"
    );
}

#[test]
fn test_extract_no_staging_files_left_after_success() {
    // After a successful extract, no `.incoming` files should remain.
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&bundle, dest.path()).unwrap();

    for entry in std::fs::read_dir(dest.path()).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(
            !name.ends_with(".incoming"),
            "found leftover staging file after extract: {name}"
        );
    }
}

#[test]
fn test_extract_cleans_up_orphan_staging_from_prior_crash() {
    // Simulate a previous extract that crashed: an orphan
    // `treeline.duckdb.incoming` with junk bytes is sitting around.
    // The next extract must remove it and write the bundle's DB cleanly.
    let src = TempDir::new().unwrap();
    let _repo = create_test_repo(&src);
    let bundle = bundle_from(src.path());

    let dest = TempDir::new().unwrap();
    std::fs::write(dest.path().join("treeline.duckdb.incoming"), b"junk-from-prior-crash").unwrap();

    SyncBundle::extract(&bundle, dest.path()).unwrap();

    assert!(
        !dest.path().join("treeline.duckdb.incoming").exists(),
        "orphan staging file should be cleaned up"
    );
    assert!(dest.path().join("treeline.duckdb").exists());
    let written = std::fs::read(dest.path().join("treeline.duckdb")).unwrap();
    assert_ne!(
        written, b"junk-from-prior-crash",
        "treeline.duckdb must reflect the bundle, not the orphan staging file"
    );
}

#[test]
fn test_extract_preserves_existing_db_on_invalid_bundle() {
    // Extract is called with bytes that aren't a valid zip. The pre-existing
    // local treeline.duckdb must be untouched — that's the whole point of
    // atomic writes (intermediate failures don't corrupt the live file).
    let dest = TempDir::new().unwrap();
    std::fs::write(dest.path().join("treeline.duckdb"), b"existing-good-db").unwrap();

    let _ = SyncBundle::extract(b"not a zip at all", dest.path());

    let after = std::fs::read(dest.path().join("treeline.duckdb")).unwrap();
    assert_eq!(after, b"existing-good-db", "DB must survive a bad bundle");
}

#[test]
fn test_extract_rejects_denylisted_paths() {
    // Manually craft a bundle that includes hub.json — receiver must skip it
    // (defense in depth — a correct producer would never include it).
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let mut buf = Vec::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        zip.start_file("hub.json", opts).unwrap();
        zip.write_all(br#"{"url":"http://malicious"}"#).unwrap();
        zip.finish().unwrap();
    }

    let dest = TempDir::new().unwrap();
    SyncBundle::extract(&buf, dest.path()).unwrap();

    assert!(
        !dest.path().join("hub.json").exists(),
        "denylisted hub.json must not be written"
    );
}
