//! Import command - import transactions from CSV files

use std::io::{self, Read as IoRead};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;
use comfy_table::{ContentArrangement, Table};
use rust_decimal::Decimal;
use treeline_core::config::ColumnMappings;
use treeline_core::services::import::{ImportOptions, NumberFormat};
use treeline_core::LogEvent;

use super::{get_context, get_logger, log_event};

pub fn run(
    file: &str,
    account: &str,
    date_column: Option<&str>,
    amount_column: Option<&str>,
    description_column: Option<&str>,
    debit_column: Option<&str>,
    credit_column: Option<&str>,
    balance_column: Option<&str>,
    flip_signs: bool,
    debit_negative: bool,
    skip_rows: u32,
    number_format: &str,
    anchor_balance: Option<f64>,
    anchor_date: Option<&str>,
    profile: Option<&str>,
    save_profile: Option<&str>,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let logger = get_logger();
    log_event(
        &logger,
        LogEvent::new("import_started").with_command("import"),
    );

    let ctx = get_context()?;

    // Resolve file path — support stdin via "-"
    let file_path = resolve_file(file)?;

    // Resolve account by UUID or name (via service layer)
    let account_id = ctx.import_service.resolve_account(account)?;

    // Load profile if specified
    let loaded_profile = if let Some(profile_name) = profile {
        let p = ctx
            .import_service
            .get_profile(profile_name)?
            .with_context(|| format!("Import profile '{}' not found", profile_name))?;
        Some(p)
    } else {
        None
    };

    // Build column mappings with resolution order:
    // 1. Explicit flags (highest priority)
    // 2. Profile settings
    // 3. Auto-detection (fallback)
    let detected = ctx.import_service.detect_columns(&file_path)?;

    let profile_mappings = loaded_profile.as_ref().map(|p| &p.column_mappings);

    let mappings = ColumnMappings {
        date: date_column
            .map(String::from)
            .or_else(|| profile_mappings.map(|m| m.date.clone()))
            .or(detected.date)
            .unwrap_or_else(|| "Date".to_string()),
        amount: amount_column
            .map(String::from)
            .or_else(|| profile_mappings.map(|m| m.amount.clone()))
            .or(detected.amount)
            .unwrap_or_else(|| "Amount".to_string()),
        description: resolve_optional_column(
            description_column,
            profile_mappings.and_then(|m| m.description.as_deref()),
            detected.description.as_deref(),
        ),
        debit: resolve_optional_column(
            debit_column,
            profile_mappings.and_then(|m| m.debit.as_deref()),
            detected.debit.as_deref(),
        ),
        credit: resolve_optional_column(
            credit_column,
            profile_mappings.and_then(|m| m.credit.as_deref()),
            detected.credit.as_deref(),
        ),
        balance: balance_column
            .map(String::from)
            .or_else(|| profile_mappings.and_then(|m| m.balance.clone())),
    };

    // Build import options with same resolution order
    let profile_opts = loaded_profile.as_ref().map(|p| &p.options);
    let effective_skip_rows = if skip_rows > 0 {
        skip_rows
    } else if let Some(p) = &loaded_profile {
        p.skip_rows as u32
    } else {
        0
    };

    let effective_flip_signs =
        flip_signs || profile_opts.map(|o| o.flip_signs).unwrap_or(false);
    let effective_debit_negative =
        debit_negative || profile_opts.map(|o| o.debit_negative).unwrap_or(false);

    // Parse anchor balance/date for preview balance calculation
    let parsed_anchor_balance =
        anchor_balance.map(|b| Decimal::from_f64_retain(b).unwrap_or_default());
    let parsed_anchor_date = anchor_date
        .map(|d| {
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .with_context(|| format!("Invalid anchor date '{}', expected YYYY-MM-DD", d))
        })
        .transpose()?;

    let options = ImportOptions {
        flip_signs: effective_flip_signs,
        debit_negative: effective_debit_negative,
        skip_rows: effective_skip_rows,
        number_format: NumberFormat::from_str(number_format),
        anchor_balance: parsed_anchor_balance,
        anchor_date: parsed_anchor_date,
    };

    // Run import (preview or execute)
    let result = ctx
        .import_service
        .import(&file_path, &account_id, &mappings, &options, dry_run)
        .map_err(|e| {
            log_event(
                &logger,
                LogEvent::new("import_failed").with_error(&e.to_string()),
            );
            e
        })?;

    // Save profile if requested (only on successful non-preview import)
    if let Some(profile_name) = save_profile {
        if !dry_run {
            ctx.import_service
                .save_profile(profile_name, &mappings, &options)?;
            if !json {
                println!(
                    "{}",
                    format!("Saved import profile '{}'", profile_name).green()
                );
            }
        }
    }

    log_event(
        &logger,
        LogEvent::new("import_completed").with_command("import"),
    );

    // Output
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    // Resolve account name for display (via service layer)
    let account_display = ctx.import_service.get_account_display_name(&account_id);

    if dry_run {
        println!(
            "{} {} → {}",
            "Preview".yellow(),
            file_path.display(),
            account_display.bold()
        );
        println!();

        // Show preview table
        if let Some(transactions) = &result.transactions {
            if transactions.is_empty() {
                println!("  No transactions found in CSV.");
            } else {
                let mut table = Table::new();
                table.set_content_arrangement(ContentArrangement::Dynamic);

                let has_balance = transactions.iter().any(|t| t.balance.is_some());
                if has_balance {
                    table.set_header(vec!["Date", "Amount", "Description", "Balance"]);
                } else {
                    table.set_header(vec!["Date", "Amount", "Description"]);
                }

                for tx in transactions {
                    let desc = tx.description.as_deref().unwrap_or("");
                    if has_balance {
                        table.add_row(vec![
                            &tx.date,
                            &tx.amount,
                            desc,
                            tx.balance.as_deref().unwrap_or(""),
                        ]);
                    } else {
                        table.add_row(vec![&tx.date, &tx.amount, desc]);
                    }
                }

                println!("{}", table);
            }
        }

        println!();
        println!(
            "  Discovered: {} transactions | Skipped: {} (invalid rows)",
            result.discovered, result.skipped
        );
        println!();
        println!("{}", "  Dry run — no changes applied.".yellow());
    } else {
        println!(
            "{} {} → {}",
            "Imported".green(),
            file_path.display(),
            account_display.bold()
        );
        println!();
        println!("  Discovered:  {} transactions", result.discovered);
        println!(
            "  Skipped:     {} (duplicates/invalid)",
            result.skipped
        );
        println!("  Imported:    {} transactions", result.imported);
        if result.balance_snapshots_created > 0 {
            println!(
                "  Snapshots:   {} balance snapshots",
                result.balance_snapshots_created
            );
        }
        println!();
        println!("  Batch: {}", result.batch_id);
    }

    Ok(())
}

/// Resolve file path, handling stdin ("-") by writing to a temp file.
fn resolve_file(file: &str) -> Result<PathBuf> {
    if file == "-" {
        // Read from stdin to temp file (ImportService needs a file path)
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("Failed to read CSV from stdin")?;

        if buffer.trim().is_empty() {
            anyhow::bail!("No CSV data received from stdin");
        }

        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join("treeline_import_stdin.csv");
        std::fs::write(&tmp_path, &buffer)
            .context("Failed to write stdin to temp file")?;
        Ok(tmp_path)
    } else {
        let path = Path::new(file);
        if !path.exists() {
            anyhow::bail!("File not found: {}", file);
        }
        Ok(path.to_path_buf())
    }
}

/// Resolve optional column with flag > profile > detected priority.
fn resolve_optional_column(
    flag: Option<&str>,
    profile: Option<&str>,
    detected: Option<&str>,
) -> Option<String> {
    flag.map(String::from)
        .or_else(|| profile.map(String::from))
        .or_else(|| detected.map(String::from))
}
