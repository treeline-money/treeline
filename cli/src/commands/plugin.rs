//! Plugin command - manage UI plugins

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

use super::get_treeline_dir;
use treeline_core::services::PluginService;

#[derive(Subcommand)]
pub enum PluginCommands {
    /// Create a new plugin from template
    New {
        /// Plugin name
        name: String,
        /// Directory to create plugin in (defaults to current directory)
        #[arg(short, long)]
        directory: Option<PathBuf>,
    },
    /// Install a plugin from local directory or GitHub URL
    Install {
        /// Local directory path or GitHub URL
        source: String,
        /// Version to install (e.g., v1.0.0). Defaults to latest release.
        #[arg(short, long)]
        version: Option<String>,
        /// Force rebuild even if dist/index.js exists (local installs only)
        #[arg(long)]
        rebuild: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Uninstall a plugin
    Uninstall {
        /// Plugin ID to uninstall
        plugin_id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List installed plugins
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub fn run(command: PluginCommands) -> Result<()> {
    let treeline_dir = get_treeline_dir();
    let plugin_service = PluginService::new(&treeline_dir);

    match command {
        PluginCommands::New { name, directory } => {
            let result = plugin_service.create_plugin(&name, directory.as_deref())?;

            if !result.success {
                eprintln!("{}", format!("Error: {}", result.error.unwrap_or_default()).red());
                std::process::exit(1);
            }

            let plugin_dir = result.install_dir.unwrap_or_default();
            println!("{}", format!("✓ Created plugin: {}", name).green());
            println!("\nPlugin directory: {}", plugin_dir);
            println!("\n{}:", "Next steps".cyan());
            println!("  1. cd {}", plugin_dir);
            println!("  2. npm install");
            println!("  3. Edit src/index.ts and src/*View.svelte");
            println!("  4. npm run build");
            println!("  5. tl plugin install {}\n", plugin_dir);
        }

        PluginCommands::Install { source, version, rebuild, json } => {
            let result = plugin_service.install_plugin(&source, version.as_deref(), rebuild)?;

            if !result.success {
                if json {
                    println!("{}", serde_json::json!({
                        "success": false,
                        "error": result.error
                    }));
                } else {
                    eprintln!("{}", format!("Error: {}", result.error.unwrap_or_default()).red());
                }
                std::process::exit(1);
            }

            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "success": true,
                    "plugin_id": result.plugin_id,
                    "plugin_name": result.plugin_name,
                    "version": result.version,
                    "install_dir": result.install_dir,
                    "source": result.source,
                    "built": result.built
                }))?);
            } else {
                println!("\n{}", format!("✓ Installed plugin: {}", result.plugin_name.as_deref().unwrap_or("")).green());
                println!("  Plugin ID: {}", result.plugin_id.as_deref().unwrap_or(""));
                println!("  Version: {}", result.version.as_deref().unwrap_or(""));
                println!("  Location: {}", result.install_dir.as_deref().unwrap_or(""));
                if let Some(src) = &result.source {
                    println!("  Source: {}", src);
                }
                if result.built == Some(true) {
                    println!("  {}", "(Built from source)".dimmed());
                }
                println!("\n{}\n", "Restart the Treeline UI to load the plugin".cyan());
            }
        }

        PluginCommands::Uninstall { plugin_id, json } => {
            let result = plugin_service.uninstall_plugin(&plugin_id)?;

            if !result.success {
                if json {
                    println!("{}", serde_json::json!({
                        "success": false,
                        "error": result.error
                    }));
                } else {
                    eprintln!("{}", format!("Error: {}", result.error.unwrap_or_default()).red());
                }
                std::process::exit(1);
            }

            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "success": true,
                    "plugin_id": result.plugin_id,
                    "plugin_name": result.plugin_name
                }))?);
            } else {
                println!("{}\n", format!("✓ Uninstalled plugin: {}", result.plugin_name.as_deref().unwrap_or(&plugin_id)).green());
            }
        }

        PluginCommands::List { json } => {
            let plugins = plugin_service.list_plugins()?;

            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "success": true,
                    "plugins": plugins
                }))?);
                return Ok(());
            }

            if plugins.is_empty() {
                println!("\n{}", "No plugins installed".dimmed());
                println!("{}\n", "Use 'tl plugin new <name>' to create a plugin".dimmed());
                return Ok(());
            }

            println!("\n{}\n", "Installed Plugins".bold());

            for plugin in plugins {
                println!("{} ({})", plugin.name.bold(), plugin.id);
                println!("  Version: {}", plugin.version);
                if !plugin.description.is_empty() {
                    println!("  {}", plugin.description.dimmed());
                }
                if !plugin.author.is_empty() {
                    println!("  {}", format!("by {}", plugin.author).dimmed());
                }
                println!();
            }
        }
    }

    Ok(())
}
