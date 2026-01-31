//! Plugin management service

use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

// Embed plugin template files at compile time
// These point to the actual template directory, so there's no duplication
mod embedded_template {
    pub const MANIFEST_JSON: &str = include_str!("../../../template/manifest.json");
    pub const PACKAGE_JSON: &str = include_str!("../../../template/package.json");
    pub const TSCONFIG_JSON: &str = include_str!("../../../template/tsconfig.json");
    pub const VITE_CONFIG_TS: &str = include_str!("../../../template/vite.config.ts");
    pub const SVELTE_CONFIG_JS: &str = include_str!("../../../template/svelte.config.js");
    pub const GITIGNORE: &str = include_str!("../../../template/.gitignore");
    pub const SRC_INDEX_TS: &str = include_str!("../../../template/src/index.ts");
    pub const SRC_VIEW_SVELTE: &str = include_str!("../../../template/src/HelloWorldView.svelte");
    pub const SCRIPTS_RELEASE_SH: &str = include_str!("../../../template/scripts/release.sh");
    pub const GITHUB_WORKFLOW: &str =
        include_str!("../../../template/.github/workflows/release.yml");
}

/// Plugin service for managing external plugins
pub struct PluginService {
    plugins_dir: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct PluginResult {
    pub success: bool,
    pub plugin_id: Option<String>,
    pub plugin_name: Option<String>,
    pub version: Option<String>,
    pub install_dir: Option<String>,
    pub source: Option<String>,
    pub built: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub permissions: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    pub plugin_id: String,
    pub has_update: bool,
    pub installed_version: String,
    pub latest_version: Option<String>,
    pub source: Option<String>,
    pub reason: Option<String>,
}

impl PluginService {
    pub fn new(treeline_dir: &Path) -> Self {
        let plugins_dir = treeline_dir.join("plugins");
        Self { plugins_dir }
    }

    /// Create a new plugin from embedded template
    pub fn create_plugin(&self, name: &str, target_dir: Option<&Path>) -> Result<PluginResult> {
        // Validate name
        let valid_name = name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
        if name.is_empty() || !valid_name {
            return Ok(PluginResult {
                success: false,
                error: Some(
                    "Plugin name must contain only letters, numbers, hyphens, and underscores"
                        .to_string(),
                ),
                ..Default::default()
            });
        }

        let target = target_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let plugin_dir = target.join(name);

        if plugin_dir.exists() {
            return Ok(PluginResult {
                success: false,
                error: Some(format!(
                    "Directory already exists: {}",
                    plugin_dir.display()
                )),
                ..Default::default()
            });
        }

        // Create directory structure
        fs::create_dir_all(&plugin_dir)?;
        fs::create_dir_all(plugin_dir.join("src"))?;
        fs::create_dir_all(plugin_dir.join("scripts"))?;
        fs::create_dir_all(plugin_dir.join(".github/workflows"))?;

        // Compute plugin names
        let table_safe_name = name.replace('-', "_");
        let display_name = name.replace('-', " ").replace('_', " ");
        let display_name: String = display_name
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        // Write template files with customized manifest
        let mut manifest: serde_json::Value =
            serde_json::from_str(embedded_template::MANIFEST_JSON)?;
        manifest["id"] = serde_json::Value::String(name.to_string());
        manifest["name"] = serde_json::Value::String(display_name);
        manifest["permissions"] = serde_json::json!({
            "read": ["transactions", "accounts"],
            "schemaName": format!("plugin_{}", table_safe_name)
        });
        fs::write(
            plugin_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;

        // Write other template files as-is
        fs::write(
            plugin_dir.join("package.json"),
            embedded_template::PACKAGE_JSON,
        )?;
        fs::write(
            plugin_dir.join("tsconfig.json"),
            embedded_template::TSCONFIG_JSON,
        )?;
        fs::write(
            plugin_dir.join("vite.config.ts"),
            embedded_template::VITE_CONFIG_TS,
        )?;
        fs::write(
            plugin_dir.join("svelte.config.js"),
            embedded_template::SVELTE_CONFIG_JS,
        )?;
        fs::write(plugin_dir.join(".gitignore"), embedded_template::GITIGNORE)?;
        fs::write(
            plugin_dir.join("src/index.ts"),
            embedded_template::SRC_INDEX_TS,
        )?;
        fs::write(
            plugin_dir.join("src/HelloWorldView.svelte"),
            embedded_template::SRC_VIEW_SVELTE,
        )?;
        fs::write(
            plugin_dir.join(".github/workflows/release.yml"),
            embedded_template::GITHUB_WORKFLOW,
        )?;

        // Write release script and make executable
        let release_script_path = plugin_dir.join("scripts/release.sh");
        fs::write(&release_script_path, embedded_template::SCRIPTS_RELEASE_SH)?;
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&release_script_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&release_script_path, perms)?;
        }

        Ok(PluginResult {
            success: true,
            plugin_id: Some(name.to_string()),
            install_dir: Some(plugin_dir.to_string_lossy().to_string()),
            ..Default::default()
        })
    }

    /// Install a plugin from local directory or GitHub URL
    pub fn install_plugin(
        &self,
        source: &str,
        version: Option<&str>,
        force_build: bool,
    ) -> Result<PluginResult> {
        // Ensure plugins directory exists
        fs::create_dir_all(&self.plugins_dir)?;

        if source.starts_with("http://")
            || source.starts_with("https://")
            || source.starts_with("git@")
        {
            self.install_from_github(source, version)
        } else {
            self.install_from_directory(Path::new(source), force_build)
        }
    }

    fn install_from_directory(&self, source_dir: &Path, force_build: bool) -> Result<PluginResult> {
        let source_dir = source_dir
            .canonicalize()
            .unwrap_or_else(|_| source_dir.to_path_buf());

        if !source_dir.exists() {
            return Ok(PluginResult {
                success: false,
                error: Some(format!("Directory not found: {}", source_dir.display())),
                ..Default::default()
            });
        }

        // Read manifest
        let manifest_path = source_dir.join("manifest.json");
        if !manifest_path.exists() {
            return Ok(PluginResult {
                success: false,
                error: Some(format!(
                    "No manifest.json found in {}",
                    source_dir.display()
                )),
                ..Default::default()
            });
        }

        let manifest: PluginManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;

        // Check if plugin needs to be built
        let dist_file = source_dir.join("dist").join("index.js");
        let needs_build = force_build || !dist_file.exists();

        if needs_build {
            if source_dir.join("package.json").exists() {
                self.build_plugin(&source_dir)?;
            } else {
                return Ok(PluginResult {
                    success: false,
                    error: Some(format!(
                        "Plugin not built and no package.json found. Expected dist/index.js at {}",
                        dist_file.display()
                    )),
                    ..Default::default()
                });
            }
        }

        // Verify dist file exists after build
        if !dist_file.exists() {
            return Ok(PluginResult {
                success: false,
                error: Some(format!(
                    "Build succeeded but dist/index.js not found at {}",
                    dist_file.display()
                )),
                ..Default::default()
            });
        }

        // Install to plugins directory
        let install_dir = self.plugins_dir.join(&manifest.id);
        fs::create_dir_all(&install_dir)?;

        fs::copy(&manifest_path, install_dir.join("manifest.json"))?;
        fs::copy(&dist_file, install_dir.join("index.js"))?;

        Ok(PluginResult {
            success: true,
            plugin_id: Some(manifest.id.clone()),
            plugin_name: Some(manifest.name),
            version: Some(manifest.version),
            install_dir: Some(install_dir.to_string_lossy().to_string()),
            built: Some(needs_build),
            ..Default::default()
        })
    }

    fn install_from_github(&self, url: &str, version: Option<&str>) -> Result<PluginResult> {
        let (owner, repo) = self.parse_github_url(url)?;

        // Get release info from GitHub API
        let api_url = if let Some(v) = version {
            format!(
                "https://api.github.com/repos/{}/{}/releases/tags/{}",
                owner, repo, v
            )
        } else {
            format!(
                "https://api.github.com/repos/{}/{}/releases/latest",
                owner, repo
            )
        };

        let client = reqwest::blocking::Client::new();
        let response = client
            .get(&api_url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "Treeline-CLI")
            .send()
            .context("Failed to fetch release info")?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            let msg = if version.is_some() {
                format!(
                    "Release {} not found for {}/{}",
                    version.unwrap(),
                    owner,
                    repo
                )
            } else {
                format!(
                    "No releases found for {}/{}. The plugin author needs to create a release.",
                    owner, repo
                )
            };
            return Ok(PluginResult {
                success: false,
                error: Some(msg),
                ..Default::default()
            });
        }

        let release_data: serde_json::Value = response.json()?;

        // Find manifest.json and index.js in release assets
        let assets: HashMap<String, String> = release_data["assets"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|asset| {
                        let name = asset["name"].as_str()?;
                        let url = asset["browser_download_url"].as_str()?;
                        Some((name.to_string(), url.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !assets.contains_key("manifest.json") {
            return Ok(PluginResult {
                success: false,
                error: Some(format!("Release is missing manifest.json asset")),
                ..Default::default()
            });
        }

        if !assets.contains_key("index.js") {
            return Ok(PluginResult {
                success: false,
                error: Some(format!("Release is missing index.js asset")),
                ..Default::default()
            });
        }

        // Download files
        let manifest_content = client.get(&assets["manifest.json"]).send()?.bytes()?;
        let index_content = client.get(&assets["index.js"]).send()?.bytes()?;

        let mut manifest: PluginManifest = serde_json::from_slice(&manifest_content)?;
        manifest.source = format!("https://github.com/{}/{}", owner, repo);

        // Install to plugins directory
        let install_dir = self.plugins_dir.join(&manifest.id);
        fs::create_dir_all(&install_dir)?;

        fs::write(
            install_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;
        fs::write(install_dir.join("index.js"), &index_content)?;

        let version = release_data["tag_name"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| manifest.version.clone());

        Ok(PluginResult {
            success: true,
            plugin_id: Some(manifest.id.clone()),
            plugin_name: Some(manifest.name),
            version: Some(version),
            install_dir: Some(install_dir.to_string_lossy().to_string()),
            source: Some(manifest.source),
            ..Default::default()
        })
    }

    fn parse_github_url(&self, url: &str) -> Result<(String, String)> {
        let https_re = Regex::new(r"https?://github\.com/([^/]+)/([^/]+?)(?:\.git)?/?$")?;
        let ssh_re = Regex::new(r"git@github\.com:([^/]+)/([^/]+?)(?:\.git)?$")?;

        if let Some(caps) = https_re.captures(url) {
            return Ok((caps[1].to_string(), caps[2].to_string()));
        }
        if let Some(caps) = ssh_re.captures(url) {
            return Ok((caps[1].to_string(), caps[2].to_string()));
        }

        anyhow::bail!(
            "Invalid GitHub URL: {}. Expected https://github.com/owner/repo",
            url
        )
    }

    fn build_plugin(&self, plugin_dir: &Path) -> Result<()> {
        // Check npm is available
        let npm_check = Command::new("npm").arg("--version").output();

        if npm_check.is_err() {
            anyhow::bail!("npm command not found. Please install Node.js and npm.");
        }

        // Install dependencies
        let install = Command::new("npm")
            .arg("install")
            .current_dir(plugin_dir)
            .output()
            .context("Failed to run npm install")?;

        if !install.status.success() {
            anyhow::bail!(
                "npm install failed: {}",
                String::from_utf8_lossy(&install.stderr)
            );
        }

        // Build plugin
        let build = Command::new("npm")
            .args(["run", "build"])
            .current_dir(plugin_dir)
            .output()
            .context("Failed to run npm run build")?;

        if !build.status.success() {
            anyhow::bail!(
                "npm run build failed: {}",
                String::from_utf8_lossy(&build.stderr)
            );
        }

        Ok(())
    }

    /// Uninstall a plugin
    pub fn uninstall_plugin(&self, plugin_id: &str) -> Result<PluginResult> {
        let plugin_dir = self.plugins_dir.join(plugin_id);

        if !plugin_dir.exists() {
            return Ok(PluginResult {
                success: false,
                error: Some(format!("Plugin not found: {}", plugin_id)),
                ..Default::default()
            });
        }

        // Read manifest for plugin name
        let manifest_path = plugin_dir.join("manifest.json");
        let plugin_name = if manifest_path.exists() {
            serde_json::from_str::<PluginManifest>(&fs::read_to_string(&manifest_path)?)
                .map(|m| m.name)
                .unwrap_or_else(|_| plugin_id.to_string())
        } else {
            plugin_id.to_string()
        };

        fs::remove_dir_all(&plugin_dir)?;

        Ok(PluginResult {
            success: true,
            plugin_id: Some(plugin_id.to_string()),
            plugin_name: Some(plugin_name),
            ..Default::default()
        })
    }

    /// List installed plugins
    pub fn list_plugins(&self) -> Result<Vec<PluginInfo>> {
        let mut plugins = Vec::new();

        if !self.plugins_dir.exists() {
            return Ok(plugins);
        }

        for entry in fs::read_dir(&self.plugins_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }

            if let Ok(content) = fs::read_to_string(&manifest_path) {
                if let Ok(manifest) = serde_json::from_str::<PluginManifest>(&content) {
                    plugins.push(PluginInfo {
                        id: manifest.id,
                        name: manifest.name,
                        version: manifest.version,
                        description: manifest.description,
                        author: manifest.author,
                        source: manifest.source,
                    });
                }
            }
        }

        Ok(plugins)
    }

    /// Fetch manifest from GitHub release
    pub fn fetch_manifest(
        &self,
        url: &str,
        version: Option<&str>,
    ) -> Result<(PluginManifest, String)> {
        let (owner, repo) = self.parse_github_url(url)?;

        let api_url = if let Some(v) = version {
            format!(
                "https://api.github.com/repos/{}/{}/releases/tags/{}",
                owner, repo, v
            )
        } else {
            format!(
                "https://api.github.com/repos/{}/{}/releases/latest",
                owner, repo
            )
        };

        let client = reqwest::blocking::Client::new();
        let response = client
            .get(&api_url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "Treeline-CLI")
            .send()?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            anyhow::bail!("Release not found");
        }

        let release_data: serde_json::Value = response.json()?;

        let assets: HashMap<String, String> = release_data["assets"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|asset| {
                        let name = asset["name"].as_str()?;
                        let url = asset["browser_download_url"].as_str()?;
                        Some((name.to_string(), url.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !assets.contains_key("manifest.json") {
            anyhow::bail!("Release is missing manifest.json asset");
        }

        let manifest_content = client.get(&assets["manifest.json"]).send()?.bytes()?;

        let manifest: PluginManifest = serde_json::from_slice(&manifest_content)?;
        let version = release_data["tag_name"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| manifest.version.clone());

        Ok((manifest, version))
    }

    /// Check for updates
    pub fn check_update(&self, plugin_id: &str) -> Result<UpdateInfo> {
        let plugin_dir = self.plugins_dir.join(plugin_id);
        let manifest_path = plugin_dir.join("manifest.json");

        if !manifest_path.exists() {
            anyhow::bail!("Plugin not found: {}", plugin_id);
        }

        let manifest: PluginManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;

        if manifest.source.is_empty() {
            return Ok(UpdateInfo {
                plugin_id: plugin_id.to_string(),
                has_update: false,
                installed_version: manifest.version,
                latest_version: None,
                source: None,
                reason: Some("no_source".to_string()),
            });
        }

        match self.fetch_manifest(&manifest.source, None) {
            Ok((_, latest_version)) => {
                let has_update = version_compare(&latest_version, &manifest.version) > 0;
                Ok(UpdateInfo {
                    plugin_id: plugin_id.to_string(),
                    has_update,
                    installed_version: manifest.version,
                    latest_version: Some(latest_version),
                    source: Some(manifest.source),
                    reason: None,
                })
            }
            Err(_) => Ok(UpdateInfo {
                plugin_id: plugin_id.to_string(),
                has_update: false,
                installed_version: manifest.version,
                latest_version: None,
                source: Some(manifest.source),
                reason: Some("fetch_failed".to_string()),
            }),
        }
    }

    /// Upgrade plugin to latest version
    pub fn upgrade_plugin(&self, plugin_id: &str) -> Result<PluginResult> {
        let plugin_dir = self.plugins_dir.join(plugin_id);
        let manifest_path = plugin_dir.join("manifest.json");

        if !manifest_path.exists() {
            return Ok(PluginResult {
                success: false,
                error: Some(format!("Plugin not found: {}", plugin_id)),
                ..Default::default()
            });
        }

        let manifest: PluginManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;

        if manifest.source.is_empty() {
            return Ok(PluginResult {
                success: false,
                error: Some(format!("Plugin '{}' has no source URL. Cannot upgrade plugins installed from local directories.", plugin_id)),
                ..Default::default()
            });
        }

        // Reinstall from source
        self.install_plugin(&manifest.source, None, false)
    }
}

impl Default for PluginResult {
    fn default() -> Self {
        Self {
            success: false,
            plugin_id: None,
            plugin_name: None,
            version: None,
            install_dir: None,
            source: None,
            built: None,
            error: None,
        }
    }
}

fn version_compare(v1: &str, v2: &str) -> i32 {
    fn parse_version(v: &str) -> Vec<u32> {
        v.trim_start_matches('v')
            .split('-')
            .next()
            .unwrap_or("")
            .split('.')
            .filter_map(|p| p.parse().ok())
            .collect()
    }

    let p1 = parse_version(v1);
    let p2 = parse_version(v2);

    let max_len = p1.len().max(p2.len());

    for i in 0..max_len {
        let n1 = p1.get(i).copied().unwrap_or(0);
        let n2 = p2.get(i).copied().unwrap_or(0);

        if n1 > n2 {
            return 1;
        } else if n1 < n2 {
            return -1;
        }
    }

    0
}
