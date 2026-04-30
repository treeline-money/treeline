//! HTTP integration tests for the hub server's OAuth + MCP paths.
//!
//! Spins up `build_app` on 127.0.0.1:0 against a temp TREELINE_DIR
//! and exercises the endpoints with reqwest. Run with:
//!   cargo test --test serve_oauth_tests -- --nocapture

use std::sync::Arc;

use chrono::Duration;
use reqwest::StatusCode;
use tempfile::TempDir;
use tokio::net::TcpListener;

use treeline_cli::commands::serve::{build_app, AppState};
use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::services::hub::HubService;
use treeline_core::services::oauth::OAuthStore;

// ============================================================================
// Harness
// ============================================================================

struct TestHub {
    base_url: String,
    master_token: String,
    oauth_store: Arc<OAuthStore>,
    _temp_dir: TempDir,
    _handle: tokio::task::JoinHandle<()>,
}

async fn spawn_hub() -> TestHub {
    spawn_hub_with_ttls(Duration::days(30), Duration::days(365)).await
}

async fn spawn_hub_with_ttls(access_ttl: Duration, refresh_ttl: Duration) -> TestHub {
    spawn_hub_inner(access_ttl, refresh_ttl, true).await
}

/// Spawn a hub with no `treeline.duckdb` provisioned. Used to verify MCP
/// `initialize` / `tools/list` succeed before any database has been pushed.
async fn spawn_hub_without_database() -> TestHub {
    spawn_hub_inner(Duration::days(30), Duration::days(365), false).await
}

async fn spawn_hub_inner(
    access_ttl: Duration,
    refresh_ttl: Duration,
    with_database: bool,
) -> TestHub {
    let temp_dir = TempDir::new().unwrap();
    let treeline_dir = temp_dir.path().to_path_buf();

    if with_database {
        // Provision a DuckDB so HubService::has_database() returns true.
        let db_path = treeline_dir.join("treeline.duckdb");
        let repo = DuckDbRepository::new(&db_path, None).expect("create repo");
        repo.ensure_schema().expect("schema");
        repo.checkpoint().expect("checkpoint");
    }

    // Master hub token (the "I own this hub" credential).
    let master_token = HubService::load_or_create_token(&treeline_dir).unwrap();

    let hub_service = HubService::new(treeline_dir.clone(), "treeline.duckdb".to_string());
    let oauth_store = Arc::new(OAuthStore::with_ttls(
        treeline_dir.clone(),
        access_ttl,
        refresh_ttl,
    ));

    let state = Arc::new(AppState::new(
        treeline_dir.clone(),
        hub_service,
        oauth_store.clone(),
    ));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestHub {
        base_url: format!("http://{}", addr),
        master_token,
        oauth_store,
        _temp_dir: temp_dir,
        _handle: handle,
    }
}

fn no_redirect_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

fn pkce_pair() -> (String, String) {
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let verifier =
        "abc123_this_is_a_pkce_code_verifier_that_is_at_least_43_characters_long_yes";
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);
    (verifier.to_string(), challenge)
}

/// Run the full OAuth dance over HTTP (register → authorize → token),
/// returning (access_token, refresh_token, client_id).
async fn full_oauth_flow(hub: &TestHub, scope: &str) -> (String, String, String) {
    let c = reqwest::Client::new();
    let redir = no_redirect_client();

    // /register
    let resp = c
        .post(format!("{}/register", hub.base_url))
        .json(&serde_json::json!({
            "redirect_uris": ["http://localhost/cb"],
            "client_name": "Test Client",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let client_id = body["client_id"].as_str().unwrap().to_string();

    // /authorize POST (form)
    let (verifier, challenge) = pkce_pair();
    let resp = redir
        .post(format!("{}/authorize", hub.base_url))
        .form(&[
            ("hub_token", hub.master_token.as_str()),
            ("redirect_uri", "http://localhost/cb"),
            ("state", "state_abc"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("client_id", client_id.as_str()),
            ("scope", scope),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();

    // Extract `code` from the redirect URL query string.
    let parsed = url::Url::parse(location).unwrap();
    let code = parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .expect("redirect includes code");

    // /token exchange.
    let resp = c
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", "http://localhost/cb"),
            ("client_id", &client_id),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();

    (
        body["access_token"].as_str().unwrap().to_string(),
        body["refresh_token"].as_str().unwrap().to_string(),
        client_id,
    )
}

async fn mcp_tools_call(
    hub: &TestHub,
    token: &str,
    tool_name: &str,
    args: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/mcp", hub.base_url))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": tool_name, "arguments": args },
        }))
        .send()
        .await
        .unwrap()
}

// ============================================================================
// Full OAuth flow
// ============================================================================

#[tokio::test]
async fn full_flow_issues_access_token_distinct_from_master() {
    let hub = spawn_hub().await;
    let (access, refresh, _) = full_oauth_flow(&hub, "read write").await;

    assert_ne!(access, hub.master_token);
    assert_ne!(refresh, hub.master_token);
    assert_ne!(access, refresh);
    assert_eq!(access.len(), 64); // 32 bytes hex
}

#[tokio::test]
async fn register_endpoint_persists_client_across_server_views() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();
    let resp = c
        .post(format!("{}/register", hub.base_url))
        .json(&serde_json::json!({
            "redirect_uris": ["http://localhost/cb"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let client_id = body["client_id"].as_str().unwrap();

    // OAuthStore sees the same client_id.
    let clients = hub.oauth_store.list_clients().unwrap();
    assert!(clients.iter().any(|c| c.client_id == client_id));
}

#[tokio::test]
async fn authorize_with_wrong_hub_token_does_not_issue_code() {
    let hub = spawn_hub().await;
    let redir = no_redirect_client();
    let (_verifier, challenge) = pkce_pair();

    let resp = redir
        .post(format!("{}/authorize", hub.base_url))
        .form(&[
            ("hub_token", "not_the_hub_token"),
            ("redirect_uri", "http://localhost/cb"),
            ("state", "s"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("scope", "read"),
        ])
        .send()
        .await
        .unwrap();

    // Should NOT redirect with a code. Current impl returns 200 HTML error page;
    // the important thing is that no redirect to the client happens.
    assert_ne!(resp.status(), StatusCode::FOUND);
}

// ============================================================================
// /mcp authentication + scope enforcement
// ============================================================================

#[tokio::test]
async fn mcp_with_per_client_token_can_call_read_tool() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("result").is_some(), "body was {}", body);
}

#[tokio::test]
async fn mcp_with_read_scope_rejects_write_tool() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_tools_call(
        &hub,
        &access,
        "tag",
        serde_json::json!({"tags": "foo", "transaction_ids": []}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let www_auth = resp
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        www_auth.contains("insufficient_scope"),
        "expected WWW-Authenticate: insufficient_scope, got {:?}",
        www_auth
    );
}

#[tokio::test]
async fn mcp_with_write_scope_allows_write_tool() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read write").await;

    // `tag` on a made-up uuid — expect the tool to run (may return app-level
    // error inside the JSON-RPC result, but the HTTP status should be 200).
    let resp = mcp_tools_call(
        &hub,
        &access,
        "tag",
        serde_json::json!({
            "tags": "test",
            "transaction_ids": ["00000000-0000-0000-0000-000000000000"],
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn mcp_with_master_hub_token_is_rejected() {
    // No backward compat: /mcp must use OAuth-issued tokens.
    let hub = spawn_hub().await;
    let resp = mcp_tools_call(&hub, &hub.master_token, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mcp_with_no_token_is_rejected() {
    let hub = spawn_hub().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/mcp", hub.base_url))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mcp_with_revoked_token_is_rejected() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    // Directly revoke in the store (revocation endpoint covered in its own test).
    hub.oauth_store.revoke_access_token(&access).unwrap();

    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mcp_with_expired_access_token_is_rejected() {
    let hub = spawn_hub_with_ttls(Duration::seconds(-1), Duration::days(30)).await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// /revoke
// ============================================================================

#[tokio::test]
async fn revoke_endpoint_invalidates_access_token() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    // Call /revoke per RFC 7009: form-encoded with `token`.
    let resp = reqwest::Client::new()
        .post(format!("{}/revoke", hub.base_url))
        .form(&[("token", access.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Subsequent /mcp call fails.
    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn revoke_refresh_cascades_to_access_token() {
    let hub = spawn_hub().await;
    let (access, refresh, _) = full_oauth_flow(&hub, "read").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/revoke", hub.base_url))
        .form(&[
            ("token", refresh.as_str()),
            ("token_type_hint", "refresh_token"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Access token also dead.
    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// Refresh grant
// ============================================================================

#[tokio::test]
async fn refresh_grant_issues_new_access_token() {
    let hub = spawn_hub().await;
    let (_initial_access, refresh, _) = full_oauth_flow(&hub, "read").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let new_access = body["access_token"].as_str().unwrap();
    assert!(!new_access.is_empty());

    // New access token works.
    let resp = mcp_tools_call(&hub, new_access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ============================================================================
// /api/push — still master-token-only
// ============================================================================

#[tokio::test]
async fn api_push_with_no_auth_is_rejected() {
    let hub = spawn_hub().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/api/push", hub.base_url))
        .body(b"garbage".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_push_with_per_client_token_is_rejected() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read write").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/push", hub.base_url))
        .bearer_auth(&access)
        .body(b"garbage".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_push_with_master_token_accepts_auth() {
    let hub = spawn_hub().await;

    // Auth passes; bundle is garbage, so expect 400 (not 401).
    let resp = reqwest::Client::new()
        .post(format!("{}/api/push", hub.base_url))
        .bearer_auth(&hub.master_token)
        .body(b"garbage".to_vec())
        .send()
        .await
        .unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::OK
            || resp.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "expected non-401 for master auth, got {}",
        resp.status()
    );
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// OAuth metadata — unchanged, but smoke test
// ============================================================================

#[tokio::test]
async fn oauth_metadata_endpoints_are_reachable() {
    let hub = spawn_hub().await;
    let resp = reqwest::Client::new()
        .get(format!("{}/.well-known/oauth-authorization-server", hub.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["authorization_endpoint"].is_string());
    assert!(body["token_endpoint"].is_string());
    assert!(body["registration_endpoint"].is_string());
}

// ============================================================================
// /mcp behavior on a freshly-provisioned hub with no database yet
//
// Thin clients (Claude.ai, ChatGPT, Claude Desktop) need to complete OAuth and
// `initialize` / `tools/list` *before* the user has pushed any data — the hub
// is effectively a Fly app with just OAuth state on disk. These tests pin
// down: protocol-level handshakes succeed, data-requiring tool calls fail
// gracefully via JSON-RPC error semantics (HTTP 200 + isError result), and
// non-DB tools (e.g. `version`) keep working.
// ============================================================================

async fn mcp_post(
    hub: &TestHub,
    token: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/mcp", hub.base_url))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn mcp_initialize_succeeds_without_database() {
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_post(
        &hub,
        &access,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("result").is_some(), "body was {}", body);
    assert_eq!(body["result"]["serverInfo"]["name"], "treeline");
    assert!(body["result"]["protocolVersion"].is_string());
}

#[tokio::test]
async fn mcp_tools_list_succeeds_without_database() {
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_post(
        &hub,
        &access,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let tools = body["result"]["tools"].as_array().expect("tools array");
    assert!(!tools.is_empty(), "tools list should not be empty");
    // Sanity: the schema-discovery tools the user will call first are present.
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"query"));
    assert!(names.contains(&"schema"));
    assert!(names.contains(&"version"));
}

#[tokio::test]
async fn mcp_data_tool_call_without_database_returns_jsonrpc_error_at_http_200() {
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    // `query` is in TOOLS_REQUIRING_DB. Without a DB on disk, the hub must
    // return HTTP 200 + a JSON-RPC success envelope carrying isError=true,
    // *not* HTTP 400 (Claude treats non-200 as a transport failure and never
    // surfaces the error message to the user).
    let resp = mcp_tools_call(
        &hub,
        &access,
        "query",
        serde_json::json!({"sql": "SELECT 1"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.to_lowercase().contains("no database"),
        "expected helpful empty-state message, got: {}",
        text
    );
}

#[tokio::test]
async fn mcp_version_tool_works_without_database() {
    // Tools that don't read DuckDB (`version`, `encryption_status`, `skills_*`,
    // `demo`) must keep working before any push. Otherwise the dashboard's
    // "Connect AI assistants" link is dead until the user does a CLI push.
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_tools_call(&hub, &access, "version", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["result"].get("isError").is_none() || body["result"]["isError"] == false);
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains("current_version"),
        "expected version payload, got: {}",
        text
    );
}

#[tokio::test]
async fn mcp_malformed_json_returns_parse_error_at_http_200() {
    // JSON-RPC parse errors must also travel at HTTP 200 — same reasoning as
    // the data-tool case above.
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/mcp", hub.base_url))
        .bearer_auth(&access)
        .header("content-type", "application/json")
        .body("{ this is not json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32700);
}
