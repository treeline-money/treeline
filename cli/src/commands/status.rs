//! Status command - show account status and summary

use anyhow::Result;
use colored::Colorize;
use comfy_table::{Table, ContentArrangement};

use super::get_context;

pub fn run(json: bool) -> Result<()> {
    let ctx = get_context()?;
    let status = ctx.status_service.get_status()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    // Print summary header matching Python format
    println!("{}", "Financial Data Status".bold());
    println!();

    // Create summary table (vertical key-value pairs like Python)
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);

    table.add_row(vec!["Accounts", &status.total_accounts.to_string()]);
    table.add_row(vec!["Transactions", &status.total_transactions.to_string()]);
    table.add_row(vec!["Balance Snapshots", &status.total_snapshots.to_string()]);
    table.add_row(vec!["Integrations", &status.total_integrations.to_string()]);

    println!("{}", table);
    println!();

    // Print date range
    if let (Some(earliest), Some(latest)) = (&status.date_range.earliest, &status.date_range.latest) {
        println!("Date range: {} to {}", earliest, latest);
        println!();
    }

    // Print connected integrations
    if !status.integration_names.is_empty() {
        println!("{}", "Connected Integrations".bold());
        for name in &status.integration_names {
            println!("  • {}", name);
        }
    }

    Ok(())
}
