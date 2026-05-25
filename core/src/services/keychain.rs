//! Keychain service - OS keychain integration for encryption key storage
//!
//! Stores the derived encryption key in the OS keychain so that the desktop app,
//! CLI, and MCP server can all access encrypted databases without env vars or
//! repeated password prompts.
//!
//! Uses the `keyring` crate for cross-platform support:
//! - macOS: Keychain Services
//! - Windows: Credential Manager
//! - Linux: kernel keyutils (persistent) / Secret Service

use anyhow::{Context, Result};

const SERVICE: &str = "treeline";
const USER: &str = "encryption-key";

/// Provides access to the OS keychain for storing/retrieving the database encryption key.
///
/// All methods are static — no instance state is needed. The keychain entry is
/// identified by service="treeline", user="encryption-key".
pub struct KeychainService;

impl KeychainService {
    /// Store the derived encryption key (hex-encoded) in the OS keychain.
    ///
    /// Overwrites any previously stored key.
    pub fn store_key(key_hex: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, USER).context("Failed to access OS keychain")?;
        entry
            .set_password(key_hex)
            .context("Failed to store key in OS keychain")?;
        Ok(())
    }

    /// Retrieve the stored encryption key from the OS keychain.
    ///
    /// Returns `Ok(None)` if no key is stored or if the keychain is unavailable.
    /// This method never errors — keychain failures degrade gracefully to `None`.
    pub fn get_key() -> Result<Option<String>> {
        let entry = match keyring::Entry::new(SERVICE, USER) {
            Ok(e) => e,
            Err(_) => return Ok(None),
        };
        match entry.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            // Graceful degradation: platform failures → treat as "no key"
            Err(_) => Ok(None),
        }
    }

    /// Delete the stored key from the OS keychain.
    ///
    /// Silently succeeds if no key was stored (NoEntry is not an error).
    pub fn delete_key() -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, USER).context("Failed to access OS keychain")?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already gone
            Err(e) => Err(anyhow::anyhow!(
                "Failed to delete key from OS keychain: {}",
                e
            )),
        }
    }

    /// Check if the OS keychain is functional on this system.
    ///
    /// Probes the keychain with a get operation. Returns true if the keychain
    /// responds (even with NoEntry), false if it's unavailable.
    pub fn is_available() -> bool {
        let entry = match keyring::Entry::new(SERVICE, USER) {
            Ok(e) => e,
            Err(_) => return false,
        };
        match entry.get_password() {
            Ok(_) => true,
            Err(keyring::Error::NoEntry) => true,
            Err(_) => false,
        }
    }
}
