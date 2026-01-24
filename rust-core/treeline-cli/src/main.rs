//! Treeline CLI - Personal finance in your terminal

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod output;

use commands::{backup, compact, demo, doctor, encrypt, logs, plugin, query, status, sync, tag};

/// Treeline - personal finance in your terminal
#[derive(Parser)]
#[command(name = "tl", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show account status and summary
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Sync accounts and transactions from integrations
    Sync {
        /// Integration name (optional, syncs all if not specified)
        integration: Option<String>,
        /// Preview changes without applying
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Execute SQL query against the database
    Query {
        /// SQL query to execute
        sql: Option<String>,
        /// Read SQL from file
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Output format
        #[arg(long, default_value = "table")]
        format: String,
        /// Output as JSON (shorthand for --format json)
        #[arg(long)]
        json: bool,
    },

    /// Apply tags to transactions
    Tag {
        /// Comma-separated tags to apply
        tags: String,
        /// Transaction IDs to tag
        #[arg(long, value_delimiter = ',')]
        ids: Vec<String>,
        /// Replace existing tags instead of appending
        #[arg(long)]
        replace: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Manage backups
    Backup {
        #[command(subcommand)]
        command: backup::BackupCommands,
    },

    /// Compact the database
    Compact {
        /// Skip creating safety backup
        #[arg(long)]
        skip_backup: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Run database health checks
    Doctor {
        /// Show verbose output
        #[arg(long, short)]
        verbose: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Encrypt the database
    Encrypt {
        /// Subcommand (status) or encrypt the database
        #[command(subcommand)]
        command: Option<encrypt::EncryptCommands>,
        /// Password for encryption
        #[arg(short, long)]
        password: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Decrypt the database
    Decrypt {
        /// Password for decryption
        #[arg(short, long)]
        password: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Manage demo mode
    Demo {
        #[command(subcommand)]
        command: Option<demo::DemoCommands>,
    },

    /// Manage plugins
    Plugin {
        #[command(subcommand)]
        command: plugin::PluginCommands,
    },

    /// View and manage application logs
    Logs {
        #[command(subcommand)]
        command: logs::LogsCommands,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = run(cli);

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Status { json } => status::run(json),
        Commands::Sync { integration, dry_run, json } => sync::run(integration, dry_run, json),
        Commands::Query { sql, file, format, json } => {
            let fmt = if json { "json".to_string() } else { format };
            query::run(sql.as_deref(), file.as_deref(), &fmt)
        }
        Commands::Tag { tags, ids, replace, json } => tag::run(&tags, ids, replace, json),
        Commands::Backup { command } => backup::run(command),
        Commands::Compact { skip_backup, json } => compact::run(skip_backup, json),
        Commands::Doctor { verbose, json } => doctor::run(verbose, json),
        Commands::Encrypt { command, password, json } => encrypt::run(command, password, json),
        Commands::Decrypt { password, json } => encrypt::run_decrypt(password, json),
        Commands::Demo { command } => demo::run(command),
        Commands::Plugin { command } => plugin::run(command),
        Commands::Logs { command } => logs::run(command),
    }
}
