//! Skill command - manage locally installed skills

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use colored::Colorize;

use super::{get_logger, get_treeline_dir, log_event};
use treeline_core::LogEvent;

#[derive(Subcommand)]
pub enum SkillCommands {
    /// List installed skills
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a skill from a local directory or file
    Add {
        /// Path to a skill directory (containing SKILL.md) or a SKILL.md file
        source: String,
        /// Override the skill name (defaults to directory name or frontmatter name)
        #[arg(long)]
        name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove an installed skill
    Remove {
        /// Skill name to remove
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show details of an installed skill
    Show {
        /// Skill name to show
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Get the skills directory path
fn skills_dir() -> PathBuf {
    get_treeline_dir().join("skills")
}

/// Ensure the skills directory exists
fn ensure_skills_dir() -> Result<PathBuf> {
    let dir = skills_dir();
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create skills directory: {:?}", dir))?;
    Ok(dir)
}

/// Parse YAML frontmatter from a SKILL.md file
fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }

    let after_first = &trimmed[3..];
    let end = after_first.find("\n---")?;
    let frontmatter = after_first[..end].trim().to_string();
    let body = after_first[end + 4..].trim().to_string();
    Some((frontmatter, body))
}

/// Extract the name field from YAML frontmatter (simple parser, no YAML dep needed)
fn extract_name(frontmatter: &str) -> Option<String> {
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name:") {
            let name = rest.trim().trim_matches('"').trim_matches('\'');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Extract the description field from YAML frontmatter
fn extract_description(frontmatter: &str) -> Option<String> {
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("description:") {
            let desc = rest.trim().trim_matches('"').trim_matches('\'');
            if !desc.is_empty() {
                return Some(desc.to_string());
            }
        }
    }
    None
}

/// Extract the version field from YAML frontmatter
fn extract_version(frontmatter: &str) -> Option<String> {
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("version:") {
            let ver = rest.trim().trim_matches('"').trim_matches('\'');
            if !ver.is_empty() {
                return Some(ver.to_string());
            }
        }
    }
    None
}

/// Info about an installed skill
struct SkillInfo {
    name: String,
    description: Option<String>,
    version: Option<String>,
    path: PathBuf,
}

/// Read skill info from a skill directory
fn read_skill_info(skill_path: &Path) -> Result<SkillInfo> {
    let skill_md = skill_path.join("SKILL.md");
    if !skill_md.exists() {
        bail!(
            "No SKILL.md found in {:?}",
            skill_path
        );
    }

    let content = fs::read_to_string(&skill_md)
        .with_context(|| format!("Failed to read {:?}", skill_md))?;

    let dir_name = skill_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let (name, description, version) = if let Some((frontmatter, _)) = parse_frontmatter(&content)
    {
        (
            extract_name(&frontmatter).unwrap_or(dir_name),
            extract_description(&frontmatter),
            extract_version(&frontmatter),
        )
    } else {
        (dir_name, None, None)
    };

    Ok(SkillInfo {
        name,
        description,
        version,
        path: skill_path.to_path_buf(),
    })
}

/// List all installed skills
fn list_skills(dir: &Path) -> Result<Vec<SkillInfo>> {
    let mut skills = Vec::new();

    if !dir.exists() {
        return Ok(skills);
    }

    let entries = fs::read_dir(dir).with_context(|| format!("Failed to read {:?}", dir))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() && path.join("SKILL.md").exists() {
            if let Ok(info) = read_skill_info(&path) {
                skills.push(info);
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

/// Copy a directory recursively
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

pub fn run(command: SkillCommands) -> Result<()> {
    let logger = get_logger();

    match command {
        SkillCommands::List { json } => {
            log_event(
                &logger,
                LogEvent::new("skill_list").with_command("skill list"),
            );

            let dir = skills_dir();
            let skills = list_skills(&dir)?;

            if json {
                let items: Vec<serde_json::Value> = skills
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "name": s.name,
                            "description": s.description,
                            "version": s.version,
                            "path": s.path.to_string_lossy(),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "success": true,
                        "skills": items,
                        "count": items.len(),
                    }))?
                );
                return Ok(());
            }

            if skills.is_empty() {
                println!("\n{}", "No skills installed".dimmed());
                println!(
                    "{}\n",
                    "Use 'tl skill add <path>' to install a skill".dimmed()
                );
                return Ok(());
            }

            println!("\n{}\n", "Installed Skills".bold());

            for skill in &skills {
                let version_str = skill
                    .version
                    .as_deref()
                    .map(|v| format!(" (v{})", v))
                    .unwrap_or_default();

                println!("{}{}", skill.name.bold(), version_str.dimmed());
                if let Some(desc) = &skill.description {
                    println!("  {}", desc.dimmed());
                }
                println!("  {}", skill.path.to_string_lossy().dimmed());
                println!();
            }

            println!(
                "{}\n",
                format!("{} skill(s) installed", skills.len()).dimmed()
            );
        }

        SkillCommands::Add { source, name, json } => {
            log_event(
                &logger,
                LogEvent::new("skill_add").with_command("skill add"),
            );

            let source_path = PathBuf::from(&source);
            if !source_path.exists() {
                bail!("Source path does not exist: {}", source);
            }

            // Determine the source directory containing SKILL.md
            let source_dir = if source_path.is_file() {
                // If a file was given, it should be SKILL.md and we use its parent
                let file_name = source_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if file_name != "SKILL.md" {
                    bail!("Expected a SKILL.md file or a directory containing one");
                }
                source_path
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("Cannot determine parent directory"))?
                    .to_path_buf()
            } else {
                if !source_path.join("SKILL.md").exists() {
                    bail!("No SKILL.md found in {}", source);
                }
                source_path.clone()
            };

            // Read skill info to get the name
            let info = read_skill_info(&source_dir)?;
            let skill_name = name.unwrap_or(info.name);

            // Validate skill name
            if !skill_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                bail!("Skill name must contain only alphanumeric characters, hyphens, and underscores");
            }

            let skills_dir = ensure_skills_dir()?;
            let dest = skills_dir.join(&skill_name);

            // Check if already installed
            if dest.exists() {
                // Remove old version
                fs::remove_dir_all(&dest)
                    .with_context(|| format!("Failed to remove existing skill: {:?}", dest))?;
            }

            // Copy skill directory
            copy_dir_recursive(&source_dir, &dest)
                .with_context(|| format!("Failed to copy skill to {:?}", dest))?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "success": true,
                        "name": skill_name,
                        "path": dest.to_string_lossy(),
                    }))?
                );
            } else {
                println!(
                    "\n{}",
                    format!("✓ Added skill: {}", skill_name).green()
                );
                println!("  Location: {}\n", dest.to_string_lossy());
            }
        }

        SkillCommands::Remove { name, json } => {
            log_event(
                &logger,
                LogEvent::new("skill_remove").with_command("skill remove"),
            );

            let dir = skills_dir();
            let skill_path = dir.join(&name);

            if !skill_path.exists() {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "success": false,
                            "error": format!("Skill '{}' not found", name),
                        })
                    );
                } else {
                    eprintln!("{}", format!("Skill '{}' not found", name).red());
                }
                std::process::exit(1);
            }

            fs::remove_dir_all(&skill_path)
                .with_context(|| format!("Failed to remove skill: {:?}", skill_path))?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "success": true,
                        "name": name,
                    }))?
                );
            } else {
                println!("\n{}\n", format!("✓ Removed skill: {}", name).green());
            }
        }

        SkillCommands::Show { name, json } => {
            log_event(
                &logger,
                LogEvent::new("skill_show").with_command("skill show"),
            );

            let dir = skills_dir();
            let skill_path = dir.join(&name);

            if !skill_path.exists() {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "success": false,
                            "error": format!("Skill '{}' not found", name),
                        })
                    );
                } else {
                    eprintln!("{}", format!("Skill '{}' not found", name).red());
                }
                std::process::exit(1);
            }

            let info = read_skill_info(&skill_path)?;

            // List all files in the skill directory
            let mut files = Vec::new();
            fn collect_files(dir: &Path, base: &Path, files: &mut Vec<String>) {
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            collect_files(&path, base, files);
                        } else if let Ok(rel) = path.strip_prefix(base) {
                            files.push(rel.to_string_lossy().to_string());
                        }
                    }
                }
            }
            collect_files(&skill_path, &skill_path, &mut files);
            files.sort();

            if json {
                let skill_md = fs::read_to_string(skill_path.join("SKILL.md"))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "success": true,
                        "name": info.name,
                        "description": info.description,
                        "version": info.version,
                        "path": info.path.to_string_lossy(),
                        "files": files,
                        "skill_md": skill_md,
                    }))?
                );
            } else {
                let version_str = info
                    .version
                    .as_deref()
                    .map(|v| format!(" (v{})", v))
                    .unwrap_or_default();

                println!("\n{}{}", info.name.bold(), version_str.dimmed());

                if let Some(desc) = &info.description {
                    println!("{}", desc.dimmed());
                }

                println!("\n{}:", "Location".cyan());
                println!("  {}", info.path.to_string_lossy());

                println!("\n{}:", "Files".cyan());
                for file in &files {
                    println!("  {}", file);
                }

                // Show SKILL.md content
                let skill_md = fs::read_to_string(skill_path.join("SKILL.md"))?;
                println!("\n{}:", "SKILL.md".cyan());
                println!("{}\n", skill_md);
            }
        }
    }

    Ok(())
}
