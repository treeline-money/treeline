//! Compact command - compact the database

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use super::get_context;

/// JSON output structure matching Python CLI
#[derive(Serialize)]
struct CompactOutput {
    original_size: u64,
    compacted_size: u64,
    backup_name: Option<String>,
}

pub fn run(skip_backup: bool, json: bool) -> Result<()> {
    let ctx = get_context()?;

    // Create safety backup first (unless skipped)
    let backup_name = if !skip_backup {
        let backup = ctx.backup_service.create(None)?;
        Some(backup.name)
    } else {
        None
    };

    let result = ctx.compact_service.compact()?;

    if json {
        let output = CompactOutput {
            original_size: result.original_size,
            compacted_size: result.compacted_size,
            backup_name,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Print safety backup info
    if let Some(name) = backup_name {
        println!("Safety backup: {}", name);
    }

    let saved = result.original_size.saturating_sub(result.compacted_size);
    let saved_pct = if result.original_size > 0 {
        (saved as f64 / result.original_size as f64) * 100.0
    } else {
        0.0
    };

    println!("{}", "Database compacted".green());
    println!("Before: {} bytes", result.original_size);
    println!("After: {} bytes", result.compacted_size);
    println!("Saved: {} bytes ({:.1}%)", saved, saved_pct);

    Ok(())
}
