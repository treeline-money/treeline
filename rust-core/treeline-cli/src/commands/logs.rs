//! Logs command - view and manage application logs

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use comfy_table::{ContentArrangement, Table};

use super::get_treeline_dir;
use treeline_core::{EntryPoint, LoggingService};

#[derive(Subcommand)]
pub enum LogsCommands {
    /// Show recent log entries
    List {
        /// Number of entries to show
        #[arg(short, long, default_value = "50")]
        limit: usize,
        /// Show only errors
        #[arg(long)]
        errors: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Clear old log entries
    Clear {
        /// Delete logs older than N days
        #[arg(long, default_value = "30")]
        older_than_days: u64,
        /// Skip confirmation prompt
        #[arg(long, short = 'f')]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show log statistics and database path
    Stats {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

fn get_logging_service() -> Result<LoggingService> {
    let treeline_dir = get_treeline_dir();
    LoggingService::new(&treeline_dir, EntryPoint::Cli, env!("CARGO_PKG_VERSION"))
}

fn format_timestamp(timestamp_ms: i64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_millis_opt(timestamp_ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| timestamp_ms.to_string())
}

pub fn run(command: LogsCommands) -> Result<()> {
    match command {
        LogsCommands::List { limit, errors, json } => {
            let service = get_logging_service()?;
            let entries = if errors {
                service.get_errors(limit)?
            } else {
                service.get_recent(limit)?
            };

            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
                return Ok(());
            }

            if entries.is_empty() {
                println!("No log entries found.");
                return Ok(());
            }

            let mut table = Table::new();
            table.set_content_arrangement(ContentArrangement::Dynamic);
            table.set_header(vec!["Time", "Entry", "Event", "Context", "Error"]);

            for entry in entries {
                let context = [
                    entry.command.as_deref(),
                    entry.page.as_deref(),
                    entry.integration.as_deref(),
                ]
                .iter()
                .filter_map(|&s| s)
                .collect::<Vec<_>>()
                .join(", ");

                let error_indicator = if entry.error_message.is_some() {
                    "!".red().to_string()
                } else {
                    String::new()
                };

                table.add_row(vec![
                    format_timestamp(entry.timestamp),
                    entry.entry_point,
                    entry.event,
                    context,
                    error_indicator,
                ]);
            }

            println!("{}", table);

            // Show error details if any
            let service = get_logging_service()?;
            let errors_list = service.get_errors(5)?;
            if !errors_list.is_empty() && !errors {
                println!();
                println!("{}", "Recent Errors:".red().bold());
                for err in errors_list.iter().take(3) {
                    println!(
                        "  {} [{}]: {}",
                        format_timestamp(err.timestamp).dimmed(),
                        err.event,
                        err.error_message.as_deref().unwrap_or("Unknown error")
                    );
                }
            }
        }
        LogsCommands::Clear {
            older_than_days,
            force,
            json,
        } => {
            let service = get_logging_service()?;
            let cutoff_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
                - (older_than_days as i64 * 24 * 60 * 60 * 1000);

            if !force && !json {
                use dialoguer::Confirm;
                if !Confirm::new()
                    .with_prompt(format!(
                        "Delete logs older than {} days?",
                        older_than_days
                    ))
                    .default(false)
                    .interact()?
                {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            let deleted = service.delete_before(cutoff_ms)?;

            if json {
                println!("{}", serde_json::json!({"deleted": deleted}));
            } else {
                println!("Deleted {} log entries", deleted);
            }
        }
        LogsCommands::Stats { json } => {
            let service = get_logging_service()?;
            let total = service.count()?;
            let errors = service.get_errors(1000)?.len();
            let db_path = service.db_path().to_path_buf();
            let size_bytes = std::fs::metadata(&db_path)
                .map(|m| m.len())
                .unwrap_or(0);

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "total_entries": total,
                        "error_count": errors,
                        "database_path": db_path.to_string_lossy(),
                        "database_size_bytes": size_bytes
                    })
                );
            } else {
                println!("{}", "Log Statistics".bold());
                println!("  Total entries: {}", total);
                println!("  Errors: {}", errors);
                println!("  Database: {}", db_path.display());
                println!("  Size: {} bytes", size_bytes);
            }
        }
    }

    Ok(())
}
