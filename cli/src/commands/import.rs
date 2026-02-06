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

    // Resolve account by UUID or name
    let account_id = resolve_account(&ctx, account)?;

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
        description: date_column_or(
            description_column,
            profile_mappings.and_then(|m| m.description.as_deref()),
            detected.description.as_deref(),
        ),
        debit: date_column_or(
            debit_column,
            profile_mappings.and_then(|m| m.debit.as_deref()),
            detected.debit.as_deref(),
        ),
        credit: date_column_or(
            credit_column,
            profile_mappings.and_then(|m| m.credit.as_deref()),
            detected.credit.as_deref(),
        ),
        balance: balance_column.map(String::from).or_else(|| {
            profile_mappings.and_then(|m| m.balance.clone())
        }),
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

    let effective_flip_signs = flip_signs
        || profile_opts.map(|o| o.flip_signs).unwrap_or(false);
    let effective_debit_negative = debit_negative
        || profile_opts.map(|o| o.debit_negative).unwrap_or(false);

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

    // Resolve account name for display
    let account_display = resolve_account_display(&ctx, &account_id);

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
    if file == "-" || (!atty::is(atty::Stream::Stdin) && file == "-") {
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

/// Resolve account by UUID or name match.
fn resolve_account(ctx: &treeline_core::TreelineContext, account: &str) -> Result<String> {
    // Try as UUID first
    if uuid::Uuid::parse_str(account).is_ok() {
        // Verify it exists
        if ctx.repository.get_account_by_id(account)?.is_some() {
            return Ok(account.to_string());
        }
        anyhow::bail!("Account not found with ID: {}", account);
    }

    // Search by name/nickname (case-insensitive)
    let accounts = ctx.repository.get_accounts()?;
    let query = account.to_lowercase();

    let matches: Vec<_> = accounts
        .iter()
        .filter(|a| {
            a.name.to_lowercase() == query
                || a.nickname
                    .as_ref()
                    .map(|n| n.to_lowercase() == query)
                    .unwrap_or(false)
        })
        .collect();

    match matches.len() {
        0 => {
            // Try substring match as fallback
            let partial: Vec<_> = accounts
                .iter()
                .filter(|a| {
                    a.name.to_lowercase().contains(&query)
                        || a.nickname
                            .as_ref()
                            .map(|n| n.to_lowercase().contains(&query))
                            .unwrap_or(false)
                })
                .collect();

            if partial.len() == 1 {
                return Ok(partial[0].id.to_string());
            }

            if partial.is_empty() {
                let account_list: Vec<String> = accounts
                    .iter()
                    .map(|a| {
                        if let Some(nick) = &a.nickname {
                            format!("  {} ({}) — {}", a.name, nick, a.id)
                        } else {
                            format!("  {} — {}", a.name, a.id)
                        }
                    })
                    .collect();

                if account_list.is_empty() {
                    anyhow::bail!(
                        "No accounts found. Create an account first with 'tl sync' or import to a new account."
                    );
                }

                anyhow::bail!(
                    "No account matching '{}'. Available accounts:\n{}",
                    account,
                    account_list.join("\n")
                );
            }

            let match_list: Vec<String> = partial
                .iter()
                .map(|a| {
                    if let Some(nick) = &a.nickname {
                        format!("  {} ({}) — {}", a.name, nick, a.id)
                    } else {
                        format!("  {} — {}", a.name, a.id)
                    }
                })
                .collect();

            anyhow::bail!(
                "Multiple accounts match '{}'. Be more specific or use the UUID:\n{}",
                account,
                match_list.join("\n")
            );
        }
        1 => Ok(matches[0].id.to_string()),
        _ => {
            let match_list: Vec<String> = matches
                .iter()
                .map(|a| {
                    if let Some(nick) = &a.nickname {
                        format!("  {} ({}) — {}", a.name, nick, a.id)
                    } else {
                        format!("  {} — {}", a.name, a.id)
                    }
                })
                .collect();

            anyhow::bail!(
                "Multiple accounts match '{}'. Use the UUID:\n{}",
                account,
                match_list.join("\n")
            );
        }
    }
}

/// Get display name for an account ID.
fn resolve_account_display(ctx: &treeline_core::TreelineContext, account_id: &str) -> String {
    ctx.repository
        .get_account_by_id(account_id)
        .ok()
        .flatten()
        .map(|a| {
            if let Some(nick) = &a.nickname {
                format!("{} ({})", a.name, nick)
            } else {
                a.name.clone()
            }
        })
        .unwrap_or_else(|| account_id.to_string())
}

/// Helper: resolve optional column with flag > profile > detected priority.
fn date_column_or(
    flag: Option<&str>,
    profile: Option<&str>,
    detected: Option<&str>,
) -> Option<String> {
    flag.map(String::from)
        .or_else(|| profile.map(String::from))
        .or_else(|| detected.map(String::from))
}
