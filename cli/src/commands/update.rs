//! CLI update command
//!
//! `tl update` - Check for updates and install the latest version

use std::env::consts::{ARCH, OS};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde::{Deserialize, Serialize};

use super::get_treeline_dir;

const GITHUB_REPO: &str = "treeline-money/treeline";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Update state stored in ~/.treeline/update-state.json
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    /// Last time we checked for updates
    pub last_check: Option<DateTime<Utc>>,
    /// Latest version found during last check
    pub latest_version: Option<String>,
    /// Whether user has been notified about this version
    pub notified_version: Option<String>,
}

impl UpdateState {
    fn path() -> PathBuf {
        get_treeline_dir().join("update-state.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }
}

/// GitHub release response (subset of fields we need)
#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Get the artifact name for the current platform
fn get_artifact_name() -> Result<&'static str> {
    match (OS, ARCH) {
        ("linux", "x86_64") => Ok("tl-linux-x64"),
        ("macos", "aarch64") => Ok("tl-macos-arm64"),
        ("macos", "x86_64") => Ok("tl-macos-arm64"), // Use arm64 with Rosetta
        ("windows", "x86_64") => Ok("tl-windows-x64.exe"),
        _ => bail!("Unsupported platform: {} {}", OS, ARCH),
    }
}

/// Get the install path for the CLI binary
fn get_install_path() -> Result<PathBuf> {
    // Use the path of the currently running executable
    std::env::current_exe().context("Failed to determine current executable path")
}

/// Fetch the latest release info from GitHub
fn fetch_latest_release() -> Result<GitHubRelease> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent("treeline-cli")
        .build()?;

    let response = client
        .get(&url)
        .send()
        .context("Failed to fetch release info from GitHub")?;

    if !response.status().is_success() {
        bail!(
            "GitHub API returned error: {} - {}",
            response.status(),
            response.text().unwrap_or_default()
        );
    }

    response
        .json::<GitHubRelease>()
        .context("Failed to parse GitHub release response")
}

/// Compare two CalVer versions (e.g., "26.2.301" vs "26.2.302")
/// Returns true if `latest` is newer than `current`
fn is_newer_version(current: &str, latest: &str) -> bool {
    // Strip 'v' prefix if present
    let current = current.strip_prefix('v').unwrap_or(current);
    let latest = latest.strip_prefix('v').unwrap_or(latest);

    let current_parts = parse_version(current);
    let latest_parts = parse_version(latest);

    // Compare component by component
    for (c, l) in current_parts.iter().zip(latest_parts.iter()) {
        if l > c {
            return true;
        }
        if l < c {
            return false;
        }
    }

    // If all compared parts are equal, longer version is newer
    latest_parts.len() > current_parts.len()
}

/// Parse a version string into numeric components
fn parse_version(v: &str) -> Vec<u32> {
    v.split('.')
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

/// Download and install the update
fn install_update(release: &GitHubRelease) -> Result<()> {
    let artifact_name = get_artifact_name()?;

    // Find the download URL for our platform
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == artifact_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No release artifact found for {} (expected: {})",
                format!("{} {}", OS, ARCH),
                artifact_name
            )
        })?;

    let install_path = get_install_path()?;
    let needs_sudo = !cfg!(windows) && !is_writable(&install_path);

    println!("Downloading {}...", artifact_name);

    // Download the binary
    let client = reqwest::blocking::Client::builder()
        .user_agent("treeline-cli")
        .build()?;

    let response = client
        .get(&asset.browser_download_url)
        .send()
        .context("Failed to download update")?;

    if !response.status().is_success() {
        bail!("Download failed: {}", response.status());
    }

    let bytes = response.bytes()?;

    // Create temp file in the same directory to ensure same filesystem
    let treeline_dir = get_treeline_dir();
    let temp_dir = install_path.parent().unwrap_or(&treeline_dir);
    let temp_path = temp_dir.join(".tl-update-tmp");

    // Ensure parent directory exists
    if let Some(parent) = temp_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&temp_path, &bytes)?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o755))?;
    }

    // Move to final location
    if needs_sudo {
        println!(
            "{}",
            "Installing to system directory requires sudo...".yellow()
        );

        let status = Command::new("sudo")
            .args(["mv", "-f"])
            .arg(&temp_path)
            .arg(&install_path)
            .status()
            .context("Failed to run sudo")?;

        if !status.success() {
            let _ = fs::remove_file(&temp_path);
            bail!("Installation failed (sudo returned non-zero)");
        }

        let _ = Command::new("sudo")
            .args(["chmod", "+x"])
            .arg(&install_path)
            .status();
    } else {
        if install_path.exists() {
            fs::remove_file(&install_path)?;
        }
        fs::rename(&temp_path, &install_path)?;
    }

    // Update state
    let mut state = UpdateState::load();
    state.notified_version = state.latest_version.clone();
    let _ = state.save();

    Ok(())
}

/// Check if a path is writable (or its parent directory if it doesn't exist)
fn is_writable(path: &PathBuf) -> bool {
    if path.exists() {
        fs::OpenOptions::new().write(true).open(path).is_ok()
    } else if let Some(parent) = path.parent() {
        parent.exists()
            && fs::metadata(parent)
                .map(|m| !m.permissions().readonly())
                .unwrap_or(false)
    } else {
        false
    }
}

/// Run the update command
/// Checks for updates and installs the latest version if available.
pub fn run(yes: bool, check_only: bool) -> Result<()> {
    println!("Checking for updates...");
    println!();

    let release = fetch_latest_release()?;

    let latest_version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);

    // Update state
    let mut state = UpdateState::load();
    state.last_check = Some(Utc::now());
    state.latest_version = Some(latest_version.to_string());
    let _ = state.save();

    let update_available = is_newer_version(CURRENT_VERSION, latest_version);

    println!("Current version: {}", CURRENT_VERSION.cyan());
    println!(
        "Latest version:  {}",
        if update_available {
            latest_version.green().to_string()
        } else {
            latest_version.to_string()
        }
    );
    println!();

    if !update_available {
        println!("{}", "You're on the latest version.".green());
        return Ok(());
    }

    if check_only {
        println!("{}", "Update available!".green().bold());
        println!("Run {} to install.", "tl update".cyan());
        if let Some(url) = Some(&release.html_url) {
            println!();
            println!("Release notes: {}", url);
        }
        return Ok(());
    }

    // Confirmation prompt
    if !yes {
        print!("Install version {}? [Y/n] ", latest_version);
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        let input = input.trim().to_lowercase();
        if !input.is_empty() && !matches!(input.as_str(), "y" | "yes") {
            println!("Update cancelled.");
            return Ok(());
        }
    }

    println!();
    install_update(&release)?;

    println!();
    println!(
        "{} Updated to version {}",
        "Success!".green().bold(),
        latest_version.green()
    );

    Ok(())
}

/// Check for updates in the background (called from other commands).
/// Shows a notification if an update is available.
pub fn maybe_notify_update() {
    let state = UpdateState::load();

    // Check if we should do a new check (every 2 hours)
    let should_check = state
        .last_check
        .map(|last| {
            let elapsed = Utc::now().signed_duration_since(last);
            elapsed.num_hours() >= 2
        })
        .unwrap_or(true);

    if should_check {
        // Do a fresh check (this makes a network request)
        if let Ok(release) = fetch_latest_release() {
            let latest = release
                .tag_name
                .strip_prefix('v')
                .unwrap_or(&release.tag_name);

            // Update state
            let mut state = UpdateState::load();
            state.last_check = Some(Utc::now());
            state.latest_version = Some(latest.to_string());
            let _ = state.save();

            if is_newer_version(CURRENT_VERSION, latest) {
                let already_notified = state
                    .notified_version
                    .as_ref()
                    .map(|v| v == latest)
                    .unwrap_or(false);

                if !already_notified {
                    print_update_notification(latest);
                    let mut state = UpdateState::load();
                    state.notified_version = Some(latest.to_string());
                    let _ = state.save();
                }
            }
        }
    } else if let Some(latest) = &state.latest_version {
        // Use cached version info
        if is_newer_version(CURRENT_VERSION, latest) {
            let already_notified = state
                .notified_version
                .as_ref()
                .map(|v| v == latest)
                .unwrap_or(false);

            if !already_notified {
                print_update_notification(latest);
                let mut state = UpdateState::load();
                state.notified_version = Some(latest.clone());
                let _ = state.save();
            }
        }
    }
}

fn print_update_notification(version: &str) {
    eprintln!();
    eprintln!(
        "{}",
        format!(
            "  A new version of Treeline is available: {} -> {}",
            CURRENT_VERSION, version
        )
        .yellow()
    );
    eprintln!("{}", "  Run 'tl update' to update.".yellow());
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison_same() {
        assert!(!is_newer_version("26.2.301", "26.2.301"));
    }

    #[test]
    fn test_version_comparison_newer_patch() {
        assert!(is_newer_version("26.2.301", "26.2.302"));
        assert!(is_newer_version("26.2.301", "26.2.400"));
    }

    #[test]
    fn test_version_comparison_newer_minor() {
        assert!(is_newer_version("26.2.301", "26.3.100"));
        assert!(is_newer_version("26.2.301", "26.12.1"));
    }

    #[test]
    fn test_version_comparison_newer_major() {
        assert!(is_newer_version("26.2.301", "27.1.100"));
        assert!(is_newer_version("26.2.301", "27.0.0"));
    }

    #[test]
    fn test_version_comparison_older() {
        assert!(!is_newer_version("26.2.301", "26.2.300"));
        assert!(!is_newer_version("26.2.301", "26.1.999"));
        assert!(!is_newer_version("26.2.301", "25.12.999"));
    }

    #[test]
    fn test_version_comparison_with_v_prefix() {
        assert!(is_newer_version("v26.2.301", "v26.2.302"));
        assert!(is_newer_version("26.2.301", "v26.2.302"));
        assert!(is_newer_version("v26.2.301", "26.2.302"));
        assert!(!is_newer_version("v26.2.301", "v26.2.301"));
    }

    #[test]
    fn test_version_parsing() {
        assert_eq!(parse_version("26.2.301"), vec![26, 2, 301]);
        assert_eq!(parse_version("1.0.0"), vec![1, 0, 0]);
        assert_eq!(parse_version("0.1.0"), vec![0, 1, 0]);
    }

    #[test]
    fn test_update_state_default() {
        let state = UpdateState::default();
        assert!(state.last_check.is_none());
        assert!(state.latest_version.is_none());
        assert!(state.notified_version.is_none());
    }

    #[test]
    fn test_update_state_serialization() {
        let state = UpdateState {
            last_check: Some(Utc::now()),
            latest_version: Some("26.2.302".to_string()),
            notified_version: None,
        };

        let json = serde_json::to_string(&state).unwrap();
        let parsed: UpdateState = serde_json::from_str(&json).unwrap();

        assert_eq!(state.latest_version, parsed.latest_version);
        assert_eq!(state.notified_version, parsed.notified_version);
    }

    #[test]
    fn test_artifact_name() {
        // This test will pass on the current platform
        let result = get_artifact_name();
        // Just verify it doesn't error on supported platforms
        if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            assert_eq!(result.unwrap(), "tl-linux-x64");
        } else if cfg!(target_os = "macos") {
            assert_eq!(result.unwrap(), "tl-macos-arm64");
        } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
            assert_eq!(result.unwrap(), "tl-windows-x64.exe");
        }
    }
}
