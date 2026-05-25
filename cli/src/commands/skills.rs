//! Skills command - discover and read agent skills
//!
//! Skills are Agent Skills standard (agentskills.io) directories containing
//! a SKILL.md file with optional references/, scripts/, and assets/ folders.
//! User skills live in ~/.treeline/skills/. The `readme` skill is bundled.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use serde::Serialize;

use super::get_treeline_dir;

#[derive(Subcommand)]
pub enum SkillsCommands {
    /// List available skills
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read a file from a skill directory
    Read {
        /// Path to read (e.g. "readme/SKILL.md" or "tax-tracking/references/schema.md")
        path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the skills directory path
    Path,
}

#[derive(Serialize)]
struct SkillInfo {
    name: String,
    description: String,
    path: String,
}

pub fn run(command: SkillsCommands) -> Result<()> {
    let treeline_dir = get_treeline_dir();
    let skills_dir = treeline_dir.join("skills");

    match command {
        SkillsCommands::List { json } => run_list(&skills_dir, json),
        SkillsCommands::Read { path, json } => run_read(&skills_dir, &path, json),
        SkillsCommands::Path => {
            println!("{}", skills_dir.display());
            Ok(())
        }
    }
}

fn run_list(skills_dir: &Path, json: bool) -> Result<()> {
    // Ensure skills directory exists
    fs::create_dir_all(skills_dir)
        .with_context(|| format!("Failed to create skills directory: {:?}", skills_dir))?;

    let mut skills: Vec<SkillInfo> = Vec::new();

    // Scan for skill directories (each must contain a SKILL.md)
    let entries = fs::read_dir(skills_dir)
        .with_context(|| format!("Failed to read skills directory: {:?}", skills_dir))?;

    for entry in entries {
        let entry = entry?;
        let entry_path = entry.path();

        if !entry_path.is_dir() {
            continue;
        }

        let skill_md_path = entry_path.join("SKILL.md");
        if !skill_md_path.exists() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        let description = parse_description(&skill_md_path).unwrap_or_default();

        skills.push(SkillInfo {
            name,
            description,
            path: entry_path.to_string_lossy().to_string(),
        });
    }

    // Sort by name for consistent output
    skills.sort_by(|a, b| a.name.cmp(&b.name));

    if json {
        println!("{}", serde_json::to_string_pretty(&skills)?);
        return Ok(());
    }

    if skills.is_empty() {
        println!("\n{}", "No skills found".dimmed());
        println!(
            "{}\n",
            format!("Create skills in: {}", skills_dir.display()).dimmed()
        );
        return Ok(());
    }

    println!("\n{}\n", "Skills".bold());
    for skill in &skills {
        println!("{}", skill.name.bold());
        if !skill.description.is_empty() {
            println!("  {}", skill.description.dimmed());
        }
        println!("  {}", skill.path.dimmed());
        println!();
    }

    Ok(())
}

fn run_read(skills_dir: &Path, path: &str, json: bool) -> Result<()> {
    // Validate path doesn't escape skills directory
    let requested = skills_dir.join(path);
    let canonical_skills = skills_dir
        .canonicalize()
        .unwrap_or_else(|_| skills_dir.to_path_buf());
    let canonical_requested = requested
        .canonicalize()
        .with_context(|| format!("File not found: {}", path))?;

    if !canonical_requested.starts_with(&canonical_skills) {
        anyhow::bail!("Path must be within the skills directory");
    }

    let content = fs::read_to_string(&canonical_requested)
        .with_context(|| format!("Failed to read: {}", path))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": path,
                "content": content
            }))?
        );
    } else {
        print!("{}", content);
    }

    Ok(())
}

/// Parse the description field from SKILL.md YAML frontmatter
fn parse_description(skill_md: &Path) -> Option<String> {
    let content = fs::read_to_string(skill_md).ok()?;
    let content = content.trim_start();

    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let after_open = &content[3..];
    let close_pos = after_open.find("\n---")?;
    let frontmatter = &after_open[..close_pos];

    // Simple YAML parsing — just find the description field
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(desc) = line.strip_prefix("description:") {
            let desc = desc.trim();
            // Strip surrounding quotes if present
            let desc = desc.strip_prefix('"').unwrap_or(desc);
            let desc = desc.strip_suffix('"').unwrap_or(desc);
            let desc = desc.strip_prefix('\'').unwrap_or(desc);
            let desc = desc.strip_suffix('\'').unwrap_or(desc);
            return Some(desc.to_string());
        }
    }

    None
}

// =============================================================================
// MCP tool support
// =============================================================================

/// List skills for MCP tool call
pub fn mcp_list() -> Result<serde_json::Value, String> {
    let treeline_dir = get_treeline_dir();
    let skills_dir = treeline_dir.join("skills");

    fs::create_dir_all(&skills_dir).map_err(|e| e.to_string())?;

    let mut skills: Vec<serde_json::Value> = Vec::new();

    let entries = fs::read_dir(&skills_dir).map_err(|e| e.to_string())?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path();

        if !entry_path.is_dir() {
            continue;
        }

        let skill_md_path = entry_path.join("SKILL.md");
        if !skill_md_path.exists() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let description = parse_description(&skill_md_path).unwrap_or_default();

        // List files in the skill directory for discoverability
        let files = list_skill_files(&entry_path);

        skills.push(serde_json::json!({
            "name": name,
            "description": description,
            "files": files
        }));
    }

    skills.sort_by(|a, b| {
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        a_name.cmp(b_name)
    });

    Ok(serde_json::json!(skills))
}

/// Read a file from a skill directory for MCP tool call
pub fn mcp_read(path: &str) -> Result<String, String> {
    let treeline_dir = get_treeline_dir();
    let skills_dir = treeline_dir.join("skills");

    let requested = skills_dir.join(path);
    let canonical_skills = skills_dir
        .canonicalize()
        .unwrap_or_else(|_| skills_dir.to_path_buf());
    let canonical_requested = requested
        .canonicalize()
        .map_err(|_| format!("File not found: {}", path))?;

    if !canonical_requested.starts_with(&canonical_skills) {
        return Err("Path must be within the skills directory".to_string());
    }

    fs::read_to_string(&canonical_requested).map_err(|e| format!("Failed to read {}: {}", path, e))
}

/// Write a file to a skill directory for MCP tool call
pub fn mcp_write(path: &str, content: &str) -> Result<String, String> {
    let treeline_dir = get_treeline_dir();
    let skills_dir = treeline_dir.join("skills");

    fs::create_dir_all(&skills_dir).map_err(|e| e.to_string())?;

    // Validate path: must have at least skill_name/filename
    let components: Vec<&str> = path.split('/').collect();
    if components.len() < 2 {
        return Err(
            "Path must include skill name and filename (e.g. 'my-skill/SKILL.md')".to_string(),
        );
    }

    // Block path traversal
    if components.iter().any(|c| *c == ".." || c.is_empty()) {
        return Err("Invalid path: must not contain '..' or empty segments".to_string());
    }

    let target = skills_dir.join(path);

    // Double-check resolved path is within skills dir
    // (can't canonicalize yet since file may not exist, so check parent)
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
        let canonical_skills = skills_dir
            .canonicalize()
            .unwrap_or_else(|_| skills_dir.to_path_buf());
        let canonical_parent = parent
            .canonicalize()
            .map_err(|e| format!("Invalid path: {}", e))?;
        if !canonical_parent.starts_with(&canonical_skills) {
            return Err("Path must be within the skills directory".to_string());
        }
    }

    fs::write(&target, content).map_err(|e| format!("Failed to write {}: {}", path, e))?;

    Ok(format!("Wrote {}", path))
}

/// List all files in a skill directory (relative paths)
fn list_skill_files(skill_dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_files(skill_dir, skill_dir, &mut files);
    files.sort();
    files
}

fn collect_files(base: &Path, dir: &Path, files: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files(base, &path, files);
            } else if let Ok(relative) = path.strip_prefix(base) {
                files.push(relative.to_string_lossy().to_string());
            }
        }
    }
}
