//! Output formatting utilities

#![allow(dead_code)]

use colored::Colorize;
use comfy_table::{presets::UTF8_FULL_CONDENSED, Table, ContentArrangement};

/// Print a success message
pub fn success(msg: &str) {
    println!("{}", msg.green());
}

/// Print an error message
pub fn error(msg: &str) {
    eprintln!("{}", msg.red());
}

/// Print a warning message
pub fn warning(msg: &str) {
    println!("{}", msg.yellow());
}

/// Print an info message
pub fn info(msg: &str) {
    println!("{}", msg.cyan());
}

/// Create a styled table
pub fn create_table() -> Table {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table
}

/// Format bytes as human-readable size
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}
