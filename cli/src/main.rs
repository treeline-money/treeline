//! Treeline CLI - Personal finance in your terminal

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod output;

use commands::{
    backup, compact, demo, doctor, encrypt, import, logs, plugin, query, setup, status, sync, tag,
    update,
};

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

    /// Import transactions from a CSV file
    Import {
        /// Path to CSV file (use "-" for stdin)
        file: String,
        /// Account ID (UUID) or name to import into
        #[arg(short, long)]
        account: String,
        /// CSV column name for dates
        #[arg(long)]
        date_column: Option<String>,
        /// CSV column name for amounts
        #[arg(long)]
        amount_column: Option<String>,
        /// CSV column name for descriptions
        #[arg(long)]
        description_column: Option<String>,
        /// CSV column name for debits (alternative to amount)
        #[arg(long)]
        debit_column: Option<String>,
        /// CSV column name for credits (alternative to amount)
        #[arg(long)]
        credit_column: Option<String>,
        /// CSV column name for running balance (creates balance snapshots)
        #[arg(long)]
        balance_column: Option<String>,
        /// Negate all amounts (for credit card statements)
        #[arg(long)]
        flip_signs: bool,
        /// Negate positive debit values (for unsigned debit/credit CSVs)
        #[arg(long)]
        debit_negative: bool,
        /// Skip N rows before the header row
        #[arg(long, default_value = "0")]
        skip_rows: u32,
        /// Number format: us (1,234.56), eu (1.234,56), eu_space (1 234,56)
        #[arg(long, default_value = "us")]
        number_format: String,
        /// Known balance for historical balance calculation (preview only)
        #[arg(long)]
        anchor_balance: Option<f64>,
        /// Date of anchor balance (YYYY-MM-DD, required with --anchor-balance)
        #[arg(long)]
        anchor_date: Option<String>,
        /// Use a saved import profile
        #[arg(long)]
        profile: Option<String>,
        /// Save settings as a named profile after import
        #[arg(long)]
        save_profile: Option<String>,
        /// Preview without importing
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Execute SQL query against the database
    #[command(alias = "sql")]
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
        /// Allow write operations (INSERT, UPDATE, DELETE, etc). Without this flag, the database is opened read-only.
        #[arg(long)]
        allow_writes: bool,
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

    /// Set up integrations (SimpleFIN, Lunchflow)
    Setup {
        #[command(subcommand)]
        command: Option<setup::SetupCommands>,
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

    /// Update to the latest version
    Update {
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
        /// Only check for updates, don't install
        #[arg(long)]
        check: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Check if this is the update command (skip update notification for it)
    let is_update_command = matches!(cli.command, Commands::Update { .. });

    let result = run(cli);

    match result {
        Ok(()) => {
            // Check for updates after successful commands (except update itself)
            if !is_update_command {
                update::maybe_notify_update();
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Status { json } => status::run(json),
        Commands::Sync {
            integration,
            dry_run,
            json,
        } => sync::run(integration, dry_run, json),
        Commands::Import {
            file,
            account,
            date_column,
            amount_column,
            description_column,
            debit_column,
            credit_column,
            balance_column,
            flip_signs,
            debit_negative,
            skip_rows,
            number_format,
            anchor_balance,
            anchor_date,
            profile,
            save_profile,
            dry_run,
            json,
        } => import::run(
            &file,
            &account,
            date_column.as_deref(),
            amount_column.as_deref(),
            description_column.as_deref(),
            debit_column.as_deref(),
            credit_column.as_deref(),
            balance_column.as_deref(),
            flip_signs,
            debit_negative,
            skip_rows,
            &number_format,
            anchor_balance,
            anchor_date.as_deref(),
            profile.as_deref(),
            save_profile.as_deref(),
            dry_run,
            json,
        ),
        Commands::Query {
            sql,
            file,
            format,
            json,
            allow_writes,
        } => {
            let fmt = if json { "json".to_string() } else { format };
            query::run(sql.as_deref(), file.as_deref(), &fmt, allow_writes)
        }
        Commands::Tag {
            tags,
            ids,
            replace,
            json,
        } => tag::run(&tags, ids, replace, json),
        Commands::Backup { command } => backup::run(command),
        Commands::Compact { skip_backup, json } => compact::run(skip_backup, json),
        Commands::Doctor { verbose, json } => doctor::run(verbose, json),
        Commands::Encrypt {
            command,
            password,
            json,
        } => encrypt::run(command, password, json),
        Commands::Decrypt { password, json } => encrypt::run_decrypt(password, json),
        Commands::Demo { command } => demo::run(command),
        Commands::Setup { command } => setup::run(command),
        Commands::Plugin { command } => plugin::run(command),
        Commands::Logs { command } => logs::run(command),
        Commands::Update { yes, check } => update::run(yes, check),
    }
}
