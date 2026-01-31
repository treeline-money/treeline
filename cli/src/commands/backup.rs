//! Backup command - manage database backups

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use comfy_table::{ContentArrangement, Table};
use treeline_core::LogEvent;

use super::{get_context, get_logger, get_treeline_dir, log_event};
use treeline_core::services::BackupService;

#[derive(Subcommand)]
pub enum BackupCommands {
    /// Create a new backup
    Create {
        /// Maximum number of backups to keep
        #[arg(long, short = 'm')]
        max_backups: Option<usize>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List available backups
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Restore from a backup
    Restore {
        /// Backup name to restore
        name: String,
        /// Skip confirmation prompt
        #[arg(long, short = 'f')]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Clear all backups
    Clear {
        /// Skip confirmation prompt
        #[arg(long, short = 'f')]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Get a BackupService without requiring database access
/// Used for operations that don't need the database (list, restore, clear)
fn get_backup_service() -> BackupService {
    let treeline_dir = get_treeline_dir();
    // Determine db filename based on demo mode
    let config = treeline_core::config::Config::load(&treeline_dir).unwrap_or_default();
    let db_filename = if config.demo_mode {
        "demo.duckdb".to_string()
    } else {
        "treeline.duckdb".to_string()
    };
    BackupService::new(treeline_dir, db_filename)
}

pub fn run(command: BackupCommands) -> Result<()> {
    let logger = get_logger();

    match command {
        BackupCommands::Create { max_backups, json } => {
            log_event(&logger, LogEvent::new("backup_started").with_command("backup create"));
            // Create needs full context to access the database
            let ctx = get_context()?;
            match ctx.backup_service.create(max_backups) {
                Ok(result) => {
                    log_event(&logger, LogEvent::new("backup_completed").with_command("backup create"));
                    if json {
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    } else {
                        println!("{}", "Backup created".green());
                        println!("  Name: {}", result.name);
                        println!("  Size: {} bytes", result.size_bytes);
                    }
                }
                Err(e) => {
                    log_event(
                        &logger,
                        LogEvent::new("backup_failed")
                            .with_command("backup create")
                            .with_error(&e.to_string()),
                    );
                    return Err(e);
                }
            }
        }
        BackupCommands::List { json } => {
            // List doesn't need database access
            let backup_service = get_backup_service();
            let backups = backup_service.list()?;

            if json {
                println!("{}", serde_json::to_string_pretty(&backups)?);
                return Ok(());
            }

            if backups.is_empty() {
                println!("No backups found.");
                return Ok(());
            }

            let mut table = Table::new();
            table.set_content_arrangement(ContentArrangement::Dynamic);
            table.set_header(vec!["Name", "Created", "Size"]);

            for backup in backups {
                table.add_row(vec![
                    backup.name,
                    backup.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                    format!("{} bytes", backup.size_bytes),
                ]);
            }

            println!("{}", table);
        }
        BackupCommands::Restore { name, force, json } => {
            log_event(&logger, LogEvent::new("restore_started").with_command("backup restore"));
            // Restore doesn't need database access - it replaces the database
            let backup_service = get_backup_service();
            if !force && !json {
                use dialoguer::Confirm;
                if !Confirm::new()
                    .with_prompt(format!("Restore from backup '{}'?", name))
                    .default(false)
                    .interact()?
                {
                    println!("Cancelled.");
                    return Ok(());
                }
            }
            match backup_service.restore(&name) {
                Ok(()) => {
                    log_event(&logger, LogEvent::new("restore_completed").with_command("backup restore"));
                    if json {
                        println!("{}", serde_json::json!({"restored": name}));
                    } else {
                        println!("Database restored from backup: {}", name);
                    }
                }
                Err(e) => {
                    log_event(
                        &logger,
                        LogEvent::new("restore_failed")
                            .with_command("backup restore")
                            .with_error(&e.to_string()),
                    );
                    return Err(e);
                }
            }
        }
        BackupCommands::Clear { force, json } => {
            // Clear doesn't need database access
            let backup_service = get_backup_service();
            if !force && !json {
                use dialoguer::Confirm;
                if !Confirm::new()
                    .with_prompt("Delete all backups?")
                    .default(false)
                    .interact()?
                {
                    println!("Cancelled.");
                    return Ok(());
                }
            }
            let result = backup_service.clear()?;
            if json {
                println!("{}", serde_json::json!({"deleted": result.deleted}));
            } else {
                println!("Deleted {} backup(s)", result.deleted);
            }
        }
    }

    Ok(())
}
