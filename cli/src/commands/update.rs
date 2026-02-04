//! CLI update management
//!
//! Provides commands for checking and installing CLI updates.
//! - `tl update` or `tl update check` - Check if a new version is available
//! - `tl update install` - Download and install the latest version

use std::env::consts::{ARCH, OS};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use clap::Subcommand;
use colored::Colorize;
use serde::{Deserialize, Serialize};

use super::get_treeline_dir;

const GITHUB_REPO: &str = "treeline-money/treeline";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Update state stored in ~/.treeline/update-state.json
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    /// Last time we checked for updates
    pub last_check: Option<DateTime<Utc>>,
    /// Latest version found during last check
    pub latest_version: Option<String>,
    /// Whether user has been notified about this version
    pub notified_version: Option<String>,
    /// Disable update checks entirely
    #[serde(default)]
    pub disabled: bool,
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
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    published_at: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Subcommand)]
pub enum UpdateCommands {
    /// Check for available updates
    Check {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Install the latest version
    Install {
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Enable update notifications
    Enable,

    /// Disable update notifications
    Disable,
}

/// Result of checking for updates
#[derive(Debug, Serialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: Option<String>,
    pub release_notes: Option<String>,
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
    if cfg!(windows) {
        // Windows: ~/.treeline/bin/tl.exe
        Ok(get_treeline_dir().join("bin").join("tl.exe"))
    } else {
        // Unix: try to detect where tl is currently installed
        if let Ok(output) = Command::new("which").arg("tl").output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout);
                return Ok(PathBuf::from(path.trim()));
            }
        }
        // Default to /usr/local/bin/tl
        Ok(PathBuf::from("/usr/local/bin/tl"))
    }
}

/// Fetch the latest release info from GitHub
fn fetch_latest_release() -> Result<GitHubRelease> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);

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

    // Parse as dot-separated numbers
    let parse_version = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|part| part.parse::<u32>().ok())
            .collect()
    };

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

/// Check for updates
pub fn check_for_update() -> Result<UpdateCheckResult> {
    let release = fetch_latest_release()?;

    // Strip 'v' prefix from tag if present
    let latest_version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_string();

    let update_available = is_newer_version(CURRENT_VERSION, &latest_version);

    // Update state
    let mut state = UpdateState::load();
    state.last_check = Some(Utc::now());
    state.latest_version = Some(latest_version.clone());
    let _ = state.save();

    Ok(UpdateCheckResult {
        current_version: CURRENT_VERSION.to_string(),
        latest_version,
        update_available,
        release_url: Some(release.html_url),
        release_notes: release.body,
    })
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

    // Download to temp file
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

        // Use sudo mv to install
        let status = Command::new("sudo")
            .args(["mv", "-f"])
            .arg(&temp_path)
            .arg(&install_path)
            .status()
            .context("Failed to run sudo")?;

        if !status.success() {
            // Clean up temp file
            let _ = fs::remove_file(&temp_path);
            bail!("Installation failed (sudo returned non-zero)");
        }

        // Ensure correct permissions
        let _ = Command::new("sudo")
            .args(["chmod", "+x"])
            .arg(&install_path)
            .status();
    } else {
        // Direct move
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
        // Try to open for writing
        fs::OpenOptions::new().write(true).open(path).is_ok()
    } else if let Some(parent) = path.parent() {
        // Check if parent directory is writable
        parent.exists() && fs::metadata(parent).map(|m| !m.permissions().readonly()).unwrap_or(false)
    } else {
        false
    }
}

/// Run the update check command
pub fn run_check(json: bool) -> Result<()> {
    let result = check_for_update()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!(
        "Current version: {}",
        result.current_version.cyan()
    );
    println!(
        "Latest version:  {}",
        if result.update_available {
            result.latest_version.green().to_string()
        } else {
            result.latest_version.to_string()
        }
    );
    println!();

    if result.update_available {
        println!(
            "{}",
            "A new version is available!".green().bold()
        );
        println!();
        println!("Run {} to install the update.", "tl update install".cyan());

        if let Some(url) = &result.release_url {
            println!();
            println!("Release notes: {}", url);
        }
    } else {
        println!("{}", "You're on the latest version.".green());
    }

    Ok(())
}

/// Run the update install command
pub fn run_install(yes: bool) -> Result<()> {
    let release = fetch_latest_release()?;

    let latest_version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);

    let update_available = is_newer_version(CURRENT_VERSION, latest_version);

    println!("Current version: {}", CURRENT_VERSION.cyan());
    println!("Latest version:  {}", latest_version.green());
    println!();

    if !update_available {
        println!("{}", "You're already on the latest version.".green());
        return Ok(());
    }

    // Confirmation prompt
    if !yes {
        print!("Do you want to install version {}? [y/N] ", latest_version);
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
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
    println!();
    println!("Run {} to see what's new.", "tl --version".cyan());

    Ok(())
}

/// Run the update enable command
pub fn run_enable() -> Result<()> {
    let mut state = UpdateState::load();
    state.disabled = false;
    state.save()?;

    println!("{}", "Update notifications enabled.".green());
    println!(
        "Treeline will check for updates and notify you when a new version is available."
    );

    Ok(())
}

/// Run the update disable command
pub fn run_disable() -> Result<()> {
    let mut state = UpdateState::load();
    state.disabled = true;
    state.save()?;

    println!("{}", "Update notifications disabled.".yellow());
    println!(
        "You can still manually check for updates with {}.",
        "tl update check".cyan()
    );

    Ok(())
}

/// Main entry point for update commands
pub fn run(command: Option<UpdateCommands>) -> Result<()> {
    match command {
        None | Some(UpdateCommands::Check { json: false }) => run_check(false),
        Some(UpdateCommands::Check { json: true }) => run_check(true),
        Some(UpdateCommands::Install { yes }) => run_install(yes),
        Some(UpdateCommands::Enable) => run_enable(),
        Some(UpdateCommands::Disable) => run_disable(),
    }
}

/// Check for updates in the background (called from other commands)
/// Shows a notification if an update is available and hasn't been shown before.
/// This is designed to be non-blocking and silent on errors.
pub fn maybe_notify_update() {
    // Run in a way that doesn't block the main command
    let state = UpdateState::load();

    // Skip if disabled
    if state.disabled {
        return;
    }

    // Check if we should do a new check (once per day)
    let should_check = state
        .last_check
        .map(|last| {
            let elapsed = Utc::now().signed_duration_since(last);
            elapsed.num_hours() >= 24
        })
        .unwrap_or(true);

    if should_check {
        // Do a fresh check (this makes a network request)
        if let Ok(result) = check_for_update() {
            if result.update_available {
                // Check if we already notified about this version
                let already_notified = state
                    .notified_version
                    .as_ref()
                    .map(|v| v == &result.latest_version)
                    .unwrap_or(false);

                if !already_notified {
                    print_update_notification(&result.latest_version);

                    // Mark as notified
                    let mut state = UpdateState::load();
                    state.notified_version = Some(result.latest_version);
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

                // Mark as notified
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
            CURRENT_VERSION,
            version
        )
        .yellow()
    );
    eprintln!(
        "{}",
        format!("  Run '{}' to update.", "tl update install").yellow()
    );
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        // Same version
        assert!(!is_newer_version("26.2.301", "26.2.301"));

        // Newer patch
        assert!(is_newer_version("26.2.301", "26.2.302"));

        // Newer minor
        assert!(is_newer_version("26.2.301", "26.3.100"));

        // Newer major
        assert!(is_newer_version("26.2.301", "27.1.100"));

        // Older version
        assert!(!is_newer_version("26.2.301", "26.2.300"));
        assert!(!is_newer_version("26.2.301", "26.1.999"));
        assert!(!is_newer_version("26.2.301", "25.12.999"));

        // With 'v' prefix
        assert!(is_newer_version("v26.2.301", "v26.2.302"));
        assert!(is_newer_version("26.2.301", "v26.2.302"));
    }
}
