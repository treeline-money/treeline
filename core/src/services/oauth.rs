//! OAuth store — per-client access and refresh tokens.
//!
//! The hub's OAuth layer issues scoped tokens to individual thin clients
//! (Claude Desktop, claude.ai, etc.) so that each client's access can be
//! revoked, scoped, and audited independently of the master hub token.
//!
//! State lives in `oauth.json` alongside `hub-sync.json`. It is NOT
//! included in the sync bundle — tokens are hub-local.
//!
//! Token TTLs are configurable via env vars:
//!   TL_HUB_ACCESS_TOKEN_TTL_DAYS   (default: 30)
//!   TL_HUB_REFRESH_TOKEN_TTL_DAYS  (default: 365)

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const STATE_FILE: &str = "oauth.json";

const DEFAULT_ACCESS_TTL_DAYS: i64 = 30;
const DEFAULT_REFRESH_TTL_DAYS: i64 = 365;

// ============================================================================
// Public API types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_name: Option<String>,
    pub redirect_uris: Vec<String>,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ValidatedToken {
    pub client_id: String,
    pub client_name: Option<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenSummary {
    pub access_token_prefix: String,
    pub client_id: String,
    pub client_name: Option<String>,
    pub scopes: Vec<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("access token is not recognized")]
    Unknown,
    #[error("access token has expired")]
    Expired,
}

#[derive(Debug, Error)]
pub enum ExchangeError {
    #[error("unknown or already-consumed authorization code")]
    UnknownCode,
    #[error("PKCE verification failed")]
    PkceFailed,
    #[error("code_verifier is required when the authorization code was issued with a code_challenge")]
    MissingVerifier,
    #[error("authorization code has expired")]
    Expired,
    #[error("i/o error: {0}")]
    Io(String),
}

#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("refresh token is not recognized")]
    Unknown,
    #[error("refresh token has expired")]
    Expired,
    #[error("i/o error: {0}")]
    Io(String),
}

// ============================================================================
// On-disk records
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthorizationCodeRecord {
    code: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: Option<String>,
    scopes: Vec<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccessTokenRecord {
    token: String,
    client_id: String,
    refresh_token: String,
    scopes: Vec<String>,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RefreshTokenRecord {
    token: String,
    client_id: String,
    scopes: Vec<String>,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OAuthState {
    #[serde(default)]
    clients: Vec<OAuthClient>,
    #[serde(default)]
    authorization_codes: Vec<AuthorizationCodeRecord>,
    #[serde(default)]
    access_tokens: Vec<AccessTokenRecord>,
    #[serde(default)]
    refresh_tokens: Vec<RefreshTokenRecord>,
}

// Authorization codes are short-lived — drop after this long.
const AUTH_CODE_LIFETIME_SECS: i64 = 600;

// ============================================================================
// Store
// ============================================================================

pub struct OAuthStore {
    treeline_dir: PathBuf,
    access_ttl: Duration,
    refresh_ttl: Duration,
    // Serializes file read-modify-write for the state file.
    lock: Mutex<()>,
}

impl OAuthStore {
    /// Construct with TTLs read from environment variables (or defaults).
    pub fn new(treeline_dir: PathBuf) -> Self {
        Self::with_ttls(treeline_dir, access_ttl_from_env(), refresh_ttl_from_env())
    }

    /// Construct with explicit TTLs — primarily for tests.
    pub fn with_ttls(
        treeline_dir: PathBuf,
        access_ttl: Duration,
        refresh_ttl: Duration,
    ) -> Self {
        Self {
            treeline_dir,
            access_ttl,
            refresh_ttl,
            lock: Mutex::new(()),
        }
    }

    // ------------------------------------------------------------------------
    // Clients
    // ------------------------------------------------------------------------

    pub fn register_client(
        &self,
        redirect_uris: Vec<String>,
        client_name: Option<String>,
    ) -> Result<OAuthClient> {
        let client = OAuthClient {
            client_id: generate_token(),
            client_name,
            redirect_uris,
            registered_at: Utc::now(),
        };

        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        state.clients.push(client.clone());
        self.save_state(&state)?;

        Ok(client)
    }

    pub fn list_clients(&self) -> Result<Vec<OAuthClient>> {
        let _guard = self.lock.lock().unwrap();
        Ok(self.load_state()?.clients)
    }

    pub fn get_client(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        let _guard = self.lock.lock().unwrap();
        Ok(self
            .load_state()?
            .clients
            .into_iter()
            .find(|c| c.client_id == client_id))
    }

    // ------------------------------------------------------------------------
    // Authorization codes
    // ------------------------------------------------------------------------

    pub fn issue_authorization_code(
        &self,
        client_id: &str,
        redirect_uri: &str,
        code_challenge: Option<String>,
        scopes: Vec<String>,
    ) -> Result<String> {
        let record = AuthorizationCodeRecord {
            code: generate_token(),
            client_id: client_id.to_string(),
            redirect_uri: redirect_uri.to_string(),
            code_challenge,
            scopes,
            created_at: Utc::now(),
        };

        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        prune_expired_auth_codes(&mut state);
        state.authorization_codes.push(record.clone());
        self.save_state(&state)?;
        Ok(record.code)
    }

    pub fn exchange_authorization_code(
        &self,
        code: &str,
        code_verifier: Option<&str>,
    ) -> std::result::Result<TokenPair, ExchangeError> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self
            .load_state()
            .map_err(|e| ExchangeError::Io(e.to_string()))?;
        prune_expired_auth_codes(&mut state);

        // Find and remove the code (single-use).
        let pos = state
            .authorization_codes
            .iter()
            .position(|c| c.code == code)
            .ok_or(ExchangeError::UnknownCode)?;
        let record = state.authorization_codes.remove(pos);

        // Expired (belt-and-suspenders — prune already dropped these).
        if Utc::now().signed_duration_since(record.created_at).num_seconds()
            > AUTH_CODE_LIFETIME_SECS
        {
            self.save_state(&state)
                .map_err(|e| ExchangeError::Io(e.to_string()))?;
            return Err(ExchangeError::Expired);
        }

        // PKCE check.
        if let Some(challenge) = &record.code_challenge {
            let verifier = code_verifier.ok_or(ExchangeError::MissingVerifier)?;
            if !verify_pkce(verifier, challenge) {
                self.save_state(&state)
                    .map_err(|e| ExchangeError::Io(e.to_string()))?;
                return Err(ExchangeError::PkceFailed);
            }
        }
        // If no challenge was set and no verifier supplied, that's fine.

        // Mint refresh + access.
        let now = Utc::now();
        let refresh = RefreshTokenRecord {
            token: generate_token(),
            client_id: record.client_id.clone(),
            scopes: record.scopes.clone(),
            issued_at: now,
            expires_at: now + self.refresh_ttl,
        };
        let access = AccessTokenRecord {
            token: generate_token(),
            client_id: record.client_id.clone(),
            refresh_token: refresh.token.clone(),
            scopes: record.scopes.clone(),
            issued_at: now,
            expires_at: now + self.access_ttl,
        };

        let pair = TokenPair {
            access_token: access.token.clone(),
            refresh_token: refresh.token.clone(),
            expires_in: self.access_ttl.num_seconds(),
            scopes: record.scopes.clone(),
        };

        state.refresh_tokens.push(refresh);
        state.access_tokens.push(access);

        self.save_state(&state)
            .map_err(|e| ExchangeError::Io(e.to_string()))?;

        Ok(pair)
    }

    // ------------------------------------------------------------------------
    // Access token validation
    // ------------------------------------------------------------------------

    pub fn validate_access_token(
        &self,
        token: &str,
    ) -> std::result::Result<ValidatedToken, ValidateError> {
        let _guard = self.lock.lock().unwrap();
        let state = self.load_state().map_err(|_| ValidateError::Unknown)?;

        let record = state
            .access_tokens
            .iter()
            .find(|t| t.token == token)
            .ok_or(ValidateError::Unknown)?;

        if Utc::now() >= record.expires_at {
            return Err(ValidateError::Expired);
        }

        let client_name = state
            .clients
            .iter()
            .find(|c| c.client_id == record.client_id)
            .and_then(|c| c.client_name.clone());

        Ok(ValidatedToken {
            client_id: record.client_id.clone(),
            client_name,
            scopes: record.scopes.clone(),
        })
    }

    // ------------------------------------------------------------------------
    // Refresh
    // ------------------------------------------------------------------------

    pub fn refresh_access_token(
        &self,
        refresh_token: &str,
    ) -> std::result::Result<TokenPair, RefreshError> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self
            .load_state()
            .map_err(|e| RefreshError::Io(e.to_string()))?;

        let record = state
            .refresh_tokens
            .iter()
            .find(|t| t.token == refresh_token)
            .cloned()
            .ok_or(RefreshError::Unknown)?;

        if Utc::now() >= record.expires_at {
            return Err(RefreshError::Expired);
        }

        let now = Utc::now();
        let new_access = AccessTokenRecord {
            token: generate_token(),
            client_id: record.client_id.clone(),
            refresh_token: record.token.clone(),
            scopes: record.scopes.clone(),
            issued_at: now,
            expires_at: now + self.access_ttl,
        };

        let pair = TokenPair {
            access_token: new_access.token.clone(),
            refresh_token: record.token.clone(),
            expires_in: self.access_ttl.num_seconds(),
            scopes: record.scopes.clone(),
        };

        state.access_tokens.push(new_access);
        self.save_state(&state)
            .map_err(|e| RefreshError::Io(e.to_string()))?;

        Ok(pair)
    }

    // ------------------------------------------------------------------------
    // Revocation
    // ------------------------------------------------------------------------

    /// Revoke an access token. Returns true if a token was actually found & removed.
    pub fn revoke_access_token(&self, token: &str) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        let before = state.access_tokens.len();
        state.access_tokens.retain(|t| t.token != token);
        let removed = state.access_tokens.len() < before;
        if removed {
            self.save_state(&state)?;
        }
        Ok(removed)
    }

    /// Revoke a refresh token AND all access tokens minted from it.
    pub fn revoke_refresh_token(&self, token: &str) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        let before = state.refresh_tokens.len();
        state.refresh_tokens.retain(|t| t.token != token);
        let removed = state.refresh_tokens.len() < before;
        if removed {
            state.access_tokens.retain(|t| t.refresh_token != token);
            self.save_state(&state)?;
        }
        Ok(removed)
    }

    /// Revoke every active access token whose value starts with `prefix`.
    /// Returns the number of tokens removed. Owner CLI affordance:
    /// `tl hub tokens revoke <prefix>`.
    pub fn revoke_access_token_by_prefix(&self, prefix: &str) -> Result<usize> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        let before = state.access_tokens.len();
        state.access_tokens.retain(|t| !t.token.starts_with(prefix));
        let removed = before - state.access_tokens.len();
        if removed > 0 {
            self.save_state(&state)?;
        }
        Ok(removed)
    }

    /// Revoke every access + refresh token for a client.
    pub fn revoke_client(&self, client_id: &str) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        state.access_tokens.retain(|t| t.client_id != client_id);
        state.refresh_tokens.retain(|t| t.client_id != client_id);
        self.save_state(&state)?;
        Ok(())
    }

    // ------------------------------------------------------------------------
    // Listing
    // ------------------------------------------------------------------------

    pub fn list_tokens(&self) -> Result<Vec<TokenSummary>> {
        let _guard = self.lock.lock().unwrap();
        let state = self.load_state()?;
        let now = Utc::now();
        Ok(state
            .access_tokens
            .iter()
            .filter(|t| t.expires_at > now)
            .map(|t| {
                let client_name = state
                    .clients
                    .iter()
                    .find(|c| c.client_id == t.client_id)
                    .and_then(|c| c.client_name.clone());
                TokenSummary {
                    access_token_prefix: t.token.chars().take(8).collect(),
                    client_id: t.client_id.clone(),
                    client_name,
                    scopes: t.scopes.clone(),
                    issued_at: t.issued_at,
                    expires_at: t.expires_at,
                }
            })
            .collect())
    }

    // ------------------------------------------------------------------------
    // File I/O
    // ------------------------------------------------------------------------

    fn state_path(&self) -> PathBuf {
        self.treeline_dir.join(STATE_FILE)
    }

    fn load_state(&self) -> Result<OAuthState> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(OAuthState::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&content).context("Failed to parse oauth.json")
    }

    fn save_state(&self, state: &OAuthState) -> Result<()> {
        fs::create_dir_all(&self.treeline_dir)?;
        let path = self.state_path();
        let tmp = path.with_extension("json.tmp");

        let content = serde_json::to_string_pretty(state)?;
        {
            let mut f = fs::File::create(&tmp)
                .with_context(|| format!("Failed to create {}", tmp.display()))?;
            f.write_all(content.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)
            .with_context(|| format!("Failed to rename {} -> {}", tmp.display(), path.display()))?;
        Ok(())
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    hex::encode(bytes)
}

fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);
    computed == challenge
}

fn prune_expired_auth_codes(state: &mut OAuthState) {
    let cutoff = Utc::now() - Duration::seconds(AUTH_CODE_LIFETIME_SECS);
    state.authorization_codes.retain(|c| c.created_at >= cutoff);
}

fn access_ttl_from_env() -> Duration {
    let days = std::env::var("TL_HUB_ACCESS_TOKEN_TTL_DAYS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(DEFAULT_ACCESS_TTL_DAYS);
    Duration::days(days)
}

fn refresh_ttl_from_env() -> Duration {
    let days = std::env::var("TL_HUB_REFRESH_TOKEN_TTL_DAYS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(DEFAULT_REFRESH_TTL_DAYS);
    Duration::days(days)
}
