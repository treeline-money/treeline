//! Tag command - apply tags to transactions

use std::io::{self, Read};
use std::process::exit;

use anyhow::Result;
use colored::Colorize;

use super::get_context;

pub fn run(tags: &str, ids: Vec<String>, replace: bool, json: bool) -> Result<()> {
    let ctx = get_context()?;

    let tag_list: Vec<String> = tags.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Get IDs from argument or stdin
    let id_list: Vec<String> = if ids.is_empty() && atty::isnt(atty::Stream::Stdin) {
        // Read from stdin
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        let trimmed = buffer.trim();

        // Parse IDs: Python uses EITHER newline OR comma (not both)
        // If input contains newlines, split by newlines only
        // Otherwise, split by commas
        if trimmed.contains('\n') {
            trimmed.lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            trimmed.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
    } else {
        ids
    };

    if id_list.is_empty() {
        anyhow::bail!("No transaction IDs provided. Use --ids or pipe IDs from stdin.");
    }

    let result = ctx.tag_service.apply_tags(&id_list, &tag_list, replace)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        // Exit with code 1 if any errors (matches Python behavior)
        if result.failed > 0 {
            exit(1);
        }
        return Ok(());
    }

    // Human-readable output matching Python format
    if result.succeeded > 0 {
        println!("{} Successfully tagged {} transaction(s)", "✓".green(), result.succeeded);
        println!("Tags applied: {}", tag_list.join(", "));
    }

    if result.failed > 0 {
        println!();
        println!("{} Failed to tag {} transaction(s)", "✗".red(), result.failed);
        for entry in &result.results {
            if let Some(error) = &entry.error {
                println!("  {}: {}", entry.transaction_id, error);
            }
        }
        // Exit with code 1 if any errors (matches Python behavior)
        exit(1);
    }

    Ok(())
}
