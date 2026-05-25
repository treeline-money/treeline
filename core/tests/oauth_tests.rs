//! OAuth store tests
//!
//! Tests for per-client OAuth token issuance, persistence, scopes,
//! expiration, refresh, and revocation. Run with:
//!   cargo test --test oauth_tests -- --nocapture

use chrono::Duration;
use tempfile::TempDir;

use treeline_core::services::oauth::{OAuthStore, ValidateError};

// ============================================================================
// Test Helpers
// ============================================================================

fn make_store(temp_dir: &TempDir) -> OAuthStore {
    OAuthStore::with_ttls(
        temp_dir.path().to_path_buf(),
        Duration::days(30),
        Duration::days(365),
    )
}

fn make_store_with_ttls(temp_dir: &TempDir, access: Duration, refresh: Duration) -> OAuthStore {
    OAuthStore::with_ttls(temp_dir.path().to_path_buf(), access, refresh)
}

fn pkce_pair() -> (String, String) {
    // S256: BASE64URL(SHA256(verifier)) == challenge
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let verifier = "this_is_a_test_pkce_verifier_string_must_be_long_enough";
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);
    (verifier.to_string(), challenge)
}

fn default_scopes() -> Vec<String> {
    vec!["read".to_string(), "write".to_string()]
}

// ============================================================================
// Client Registration
// ============================================================================

#[test]
fn test_register_client_generates_unique_ids() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let c1 = store
        .register_client(vec!["http://a/cb".into()], Some("Client A".into()))
        .unwrap();
    let c2 = store
        .register_client(vec!["http://b/cb".into()], Some("Client B".into()))
        .unwrap();

    assert_ne!(c1.client_id, c2.client_id);
    assert_eq!(c1.client_id.len(), 64);
}

#[test]
fn test_register_client_persists_across_store_instances() {
    let temp_dir = TempDir::new().unwrap();

    let client_id = {
        let store = make_store(&temp_dir);
        store
            .register_client(vec!["http://a/cb".into()], None)
            .unwrap()
            .client_id
    };

    // New store instance against same dir — should see the client
    let store2 = make_store(&temp_dir);
    let clients = store2.list_clients().unwrap();
    assert!(clients.iter().any(|c| c.client_id == client_id));
}

// ============================================================================
// Authorization Code → Token Exchange
// ============================================================================

#[test]
fn test_exchange_code_returns_distinct_access_and_refresh() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (verifier, challenge) = pkce_pair();

    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();

    let pair = store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    assert!(!pair.access_token.is_empty());
    assert!(!pair.refresh_token.is_empty());
    assert_ne!(pair.access_token, pair.refresh_token);
    assert!(pair.expires_in > 0);
    assert_eq!(pair.scopes, default_scopes());
}

#[test]
fn test_exchange_code_is_single_use() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();

    store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    // Second exchange must fail
    let result = store.exchange_authorization_code(&code, Some(&verifier));
    assert!(result.is_err());
}

#[test]
fn test_exchange_code_with_wrong_pkce_verifier_fails() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (_verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();

    let result = store.exchange_authorization_code(&code, Some("wrong_verifier"));
    assert!(result.is_err());
}

#[test]
fn test_exchange_code_without_pkce_when_challenge_set_fails() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (_verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();

    let result = store.exchange_authorization_code(&code, None);
    assert!(result.is_err());
}

#[test]
fn test_exchange_unknown_code_fails() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let result = store.exchange_authorization_code("not_a_real_code", None);
    assert!(result.is_err());
}

// ============================================================================
// Access Token Validation
// ============================================================================

#[test]
fn test_validate_access_token_returns_scopes_and_client_id() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], Some("My App".into()))
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            vec!["read".into()],
        )
        .unwrap();
    let pair = store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    let validated = store.validate_access_token(&pair.access_token).unwrap();
    assert_eq!(validated.client_id, client.client_id);
    assert_eq!(validated.scopes, vec!["read".to_string()]);
    assert_eq!(validated.client_name.as_deref(), Some("My App"));
}

#[test]
fn test_validate_unknown_token_fails() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let result = store.validate_access_token("bogus_token");
    assert!(matches!(result, Err(ValidateError::Unknown)));
}

#[test]
fn test_validate_expired_access_token_fails() {
    let temp_dir = TempDir::new().unwrap();
    // Very short access TTL so it expires immediately
    let store = make_store_with_ttls(&temp_dir, Duration::seconds(-1), Duration::days(365));

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();
    let pair = store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    let result = store.validate_access_token(&pair.access_token);
    assert!(matches!(result, Err(ValidateError::Expired)));
}

// ============================================================================
// Refresh Token Flow
// ============================================================================

#[test]
fn test_refresh_token_issues_new_access_token() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();
    let initial_pair = store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    let refreshed = store
        .refresh_access_token(&initial_pair.refresh_token)
        .unwrap();

    assert_ne!(refreshed.access_token, initial_pair.access_token);
    assert_eq!(refreshed.scopes, default_scopes());
    // Either same refresh token or a newly rotated one; both are spec-compliant.
    // At minimum it must still validate.
    assert!(!refreshed.refresh_token.is_empty());
}

#[test]
fn test_refresh_with_expired_refresh_token_fails() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store_with_ttls(&temp_dir, Duration::days(30), Duration::seconds(-1));

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();
    let pair = store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    let result = store.refresh_access_token(&pair.refresh_token);
    assert!(result.is_err());
}

#[test]
fn test_refresh_with_unknown_token_fails() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let result = store.refresh_access_token("not_a_real_refresh");
    assert!(result.is_err());
}

// ============================================================================
// Revocation
// ============================================================================

#[test]
fn test_revoke_access_token_invalidates_it() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();
    let pair = store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    // Before revoke: validates.
    assert!(store.validate_access_token(&pair.access_token).is_ok());

    store.revoke_access_token(&pair.access_token).unwrap();

    // After revoke: unknown.
    let result = store.validate_access_token(&pair.access_token);
    assert!(matches!(result, Err(ValidateError::Unknown)));
}

#[test]
fn test_revoke_refresh_cascades_to_access_tokens() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();
    let pair = store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    // Mint a second access token from this refresh (should also die on revoke).
    let refreshed = store.refresh_access_token(&pair.refresh_token).unwrap();

    store.revoke_refresh_token(&pair.refresh_token).unwrap();

    assert!(matches!(
        store.validate_access_token(&pair.access_token),
        Err(ValidateError::Unknown)
    ));
    assert!(matches!(
        store.validate_access_token(&refreshed.access_token),
        Err(ValidateError::Unknown)
    ));
    // And refresh itself is dead.
    assert!(store.refresh_access_token(&pair.refresh_token).is_err());
}

// ============================================================================
// Persistence
// ============================================================================

#[test]
fn test_store_survives_process_restart() {
    let temp_dir = TempDir::new().unwrap();

    let (client_id, access_token, refresh_token, verifier, challenge) = {
        let store = make_store(&temp_dir);
        let client = store
            .register_client(vec!["http://a/cb".into()], Some("Persistent".into()))
            .unwrap();
        let (verifier, challenge) = pkce_pair();
        let code = store
            .issue_authorization_code(
                &client.client_id,
                "http://a/cb",
                Some(challenge.clone()),
                default_scopes(),
            )
            .unwrap();
        let pair = store
            .exchange_authorization_code(&code, Some(&verifier))
            .unwrap();
        (
            client.client_id,
            pair.access_token,
            pair.refresh_token,
            verifier,
            challenge,
        )
    };

    // Drop and recreate — simulates server restart.
    let _ = (verifier, challenge); // consumed
    let store2 = make_store(&temp_dir);

    let validated = store2.validate_access_token(&access_token).unwrap();
    assert_eq!(validated.client_id, client_id);

    // Refresh still works after restart.
    let refreshed = store2.refresh_access_token(&refresh_token).unwrap();
    assert!(!refreshed.access_token.is_empty());
}

// ============================================================================
// Listing + Owner-Side Views
// ============================================================================

#[test]
fn test_list_tokens_shows_issued_access_tokens() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], Some("Named".into()))
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();
    store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    let listed = store.list_tokens().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].client_id, client.client_id);
    assert_eq!(listed[0].client_name.as_deref(), Some("Named"));
    assert_eq!(listed[0].scopes, default_scopes());
}

#[test]
fn test_list_tokens_excludes_revoked() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();
    let pair = store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    assert_eq!(store.list_tokens().unwrap().len(), 1);

    store.revoke_access_token(&pair.access_token).unwrap();
    assert_eq!(store.list_tokens().unwrap().len(), 0);
}

#[test]
fn test_issue_code_without_pkce_is_allowed_and_no_verifier_required() {
    // PKCE is recommended but not strictly required by the store API —
    // the server handler may enforce it separately. The store itself
    // should allow None→None as a matching pair.
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);

    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let code = store
        .issue_authorization_code(&client.client_id, "http://a/cb", None, default_scopes())
        .unwrap();

    let pair = store.exchange_authorization_code(&code, None).unwrap();
    assert!(!pair.access_token.is_empty());
}

// ============================================================================
// Revoke by prefix — owner CLI affordance
// ============================================================================

#[test]
fn test_revoke_access_token_by_prefix_unique_match() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);
    let client = store
        .register_client(vec!["http://a/cb".into()], None)
        .unwrap();
    let (verifier, challenge) = pkce_pair();
    let code = store
        .issue_authorization_code(
            &client.client_id,
            "http://a/cb",
            Some(challenge),
            default_scopes(),
        )
        .unwrap();
    let pair = store
        .exchange_authorization_code(&code, Some(&verifier))
        .unwrap();

    let prefix: String = pair.access_token.chars().take(8).collect();
    let n = store.revoke_access_token_by_prefix(&prefix).unwrap();
    assert_eq!(n, 1);

    assert!(matches!(
        store.validate_access_token(&pair.access_token),
        Err(ValidateError::Unknown)
    ));
}

#[test]
fn test_revoke_access_token_by_prefix_no_match_returns_zero() {
    let temp_dir = TempDir::new().unwrap();
    let store = make_store(&temp_dir);
    let n = store.revoke_access_token_by_prefix("zzzzzzzz").unwrap();
    assert_eq!(n, 0);
}
