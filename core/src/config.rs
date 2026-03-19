//! Configuration management
//!
//! Compatible with the Python CLI / Desktop App settings.json format:
//! ```json
//! {
//!   "app": { "demoMode": false, ... },
//!   "plugins": { ... },
//!   "importProfiles": { "profiles": { ... }, "accountMappings": { ... } }
//! }
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Raw settings.json structure (matching Python/App format)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsFile {
    #[serde(default)]
    app: AppSettings,
    #[serde(default)]
    plugins: serde_json::Value,
    #[serde(default)]
    import_profiles: ImportProfilesContainer,
    #[serde(default)]
    disabled_plugins: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    #[serde(default)]
    demo_mode: bool,
    #[serde(flatten)]
    other: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportProfilesContainer {
    #[serde(default)]
    profiles: HashMap<String, ImportProfile>,
    #[serde(default)]
    account_mappings: HashMap<String, String>,
}

/// Hub configuration for remote sync
///
/// Stored in hub.json (device-specific, never synced).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubConfig {
    pub url: String,
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_push: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pull: Option<chrono::DateTime<chrono::Utc>>,
}

impl HubConfig {
    /// Load hub config from treeline directory
    pub fn load(treeline_dir: &Path) -> Result<Option<Self>> {
        let path = treeline_dir.join("hub.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Self = serde_json::from_str(&content)?;
        Ok(Some(config))
    }

    /// Save hub config to treeline directory
    pub fn save(&self, treeline_dir: &Path) -> Result<()> {
        let path = treeline_dir.join("hub.json");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Remove hub config from treeline directory
    pub fn remove(treeline_dir: &Path) -> Result<()> {
        let path = treeline_dir.join("hub.json");
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

/// Treeline configuration (simplified view of settings)
#[derive(Debug, Clone)]
pub struct Config {
    pub demo_mode: bool,
    pub import_profiles: HashMap<String, ImportProfile>,
    // Keep the raw settings for preservation when saving
    _raw_settings: SettingsFile,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            demo_mode: false,
            import_profiles: HashMap::new(),
            _raw_settings: SettingsFile::default(),
        }
    }
}

impl Config {
    /// Load config from treeline directory
    ///
    /// Demo mode can be enabled via:
    /// 1. Settings file (tl demo on)
    /// 2. Environment variable TREELINE_DEMO_MODE (for CI/testing)
    pub fn load(treeline_dir: &Path) -> Result<Self> {
        let settings_path = treeline_dir.join("settings.json");

        let raw: SettingsFile = if settings_path.exists() {
            let content = std::fs::read_to_string(&settings_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            SettingsFile::default()
        };

        // Check env var for demo mode override (for CI/testing)
        let demo_mode = match std::env::var("TREELINE_DEMO_MODE").ok().as_deref() {
            Some("true" | "1" | "yes" | "TRUE" | "YES") => true,
            Some("false" | "0" | "no" | "FALSE" | "NO") => false,
            _ => raw.app.demo_mode,
        };

        Ok(Self {
            demo_mode,
            import_profiles: raw.import_profiles.profiles.clone(),
            _raw_settings: raw,
        })
    }

    /// Save config to treeline directory
    /// Preserves other settings that the CLI doesn't manage
    pub fn save(&self, treeline_dir: &Path) -> Result<()> {
        let settings_path = treeline_dir.join("settings.json");

        // Load existing settings to preserve fields we don't manage
        let mut settings = if settings_path.exists() {
            let content = std::fs::read_to_string(&settings_path)?;
            serde_json::from_str::<SettingsFile>(&content).unwrap_or_default()
        } else {
            SettingsFile::default()
        };

        // Update only the fields we manage
        settings.app.demo_mode = self.demo_mode;
        settings.import_profiles.profiles = self.import_profiles.clone();

        let content = serde_json::to_string_pretty(&settings)?;
        std::fs::write(&settings_path, content)?;
        Ok(())
    }

    /// Enable demo mode
    pub fn enable_demo_mode(&mut self) {
        self.demo_mode = true;
    }

    /// Disable demo mode
    pub fn disable_demo_mode(&mut self) {
        self.demo_mode = false;
    }
}

/// Import profile for CSV imports
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProfile {
    pub column_mappings: ColumnMappings,
    #[serde(default)]
    pub date_format: Option<String>,
    #[serde(default)]
    pub skip_rows: usize,
    #[serde(default)]
    pub options: ImportOptions,
}

/// Import options for profile storage
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    #[serde(default)]
    pub flip_signs: bool,
    #[serde(default)]
    pub debit_negative: bool,
    /// Number format string: "us", "eu", or "eu_space". None defaults to US.
    #[serde(default)]
    pub number_format: Option<String>,
}

/// Column mappings for CSV import
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMappings {
    pub date: String,
    pub amount: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub credit: Option<String>,
    #[serde(default)]
    pub debit: Option<String>,
    /// Optional running balance column for balance snapshots
    #[serde(default)]
    pub balance: Option<String>,
}

impl Default for ColumnMappings {
    fn default() -> Self {
        Self {
            date: "Date".to_string(),
            amount: "Amount".to_string(),
            description: Some("Description".to_string()),
            credit: None,
            debit: None,
            balance: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_hub_config_none_by_default() {
        let temp_dir = TempDir::new().unwrap();
        let hub = HubConfig::load(temp_dir.path()).unwrap();
        assert!(hub.is_none());
    }

    #[test]
    fn test_hub_config_save_and_load() {
        let temp_dir = TempDir::new().unwrap();

        let hub = HubConfig {
            url: "http://localhost:4242".to_string(),
            token: "abc123".to_string(),
            last_push: None,
            last_pull: None,
        };
        hub.save(temp_dir.path()).unwrap();

        let loaded = HubConfig::load(temp_dir.path()).unwrap().unwrap();
        assert_eq!(loaded.url, "http://localhost:4242");
        assert_eq!(loaded.token, "abc123");
        assert!(loaded.last_push.is_none());
        assert!(loaded.last_pull.is_none());
    }

    #[test]
    fn test_hub_config_save_with_timestamps() {
        let temp_dir = TempDir::new().unwrap();

        let now = chrono::Utc::now();
        let hub = HubConfig {
            url: "http://localhost:4242".to_string(),
            token: "abc123".to_string(),
            last_push: Some(now),
            last_pull: Some(now),
        };
        hub.save(temp_dir.path()).unwrap();

        let loaded = HubConfig::load(temp_dir.path()).unwrap().unwrap();
        assert!(loaded.last_push.is_some());
        assert!(loaded.last_pull.is_some());
    }

    #[test]
    fn test_hub_config_remove() {
        let temp_dir = TempDir::new().unwrap();

        let hub = HubConfig {
            url: "http://localhost:4242".to_string(),
            token: "abc123".to_string(),
            last_push: None,
            last_pull: None,
        };
        hub.save(temp_dir.path()).unwrap();
        assert!(HubConfig::load(temp_dir.path()).unwrap().is_some());

        HubConfig::remove(temp_dir.path()).unwrap();
        assert!(HubConfig::load(temp_dir.path()).unwrap().is_none());
    }

    #[test]
    fn test_hub_config_independent_of_settings() {
        let temp_dir = TempDir::new().unwrap();

        // Write settings.json
        let settings = r#"{"app":{"demoMode":true},"plugins":{"foo":"bar"}}"#;
        std::fs::write(temp_dir.path().join("settings.json"), settings).unwrap();

        // Save hub config separately
        let hub = HubConfig {
            url: "http://localhost:4242".to_string(),
            token: "abc123".to_string(),
            last_push: None,
            last_pull: None,
        };
        hub.save(temp_dir.path()).unwrap();

        // settings.json should be untouched
        let content =
            std::fs::read_to_string(temp_dir.path().join("settings.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["app"]["demoMode"], true);
        assert!(v.get("hub").is_none());

        // hub.json should exist separately
        assert!(temp_dir.path().join("hub.json").exists());
    }
}
