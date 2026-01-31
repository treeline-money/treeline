//! Doctor command - run database health checks

use anyhow::Result;
use colored::Colorize;
use comfy_table::{Table, ContentArrangement, Cell, Color};
use serde_json::Value;

use super::get_context;

/// Format a detail JSON value for display
fn format_detail(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            // Format as "key: value, key: value"
            let parts: Vec<String> = map.iter()
                .filter(|(k, v)| !v.is_null() && *k != "transactions")
                .map(|(k, v)| {
                    let display_val = match v {
                        Value::String(s) => {
                            // Truncate long strings
                            if s.len() > 40 {
                                format!("{}...", &s[..37])
                            } else {
                                s.clone()
                            }
                        }
                        Value::Number(n) => {
                            // Format amounts nicely
                            if k.contains("amount") {
                                if let Some(f) = n.as_f64() {
                                    format!("${:.2}", f.abs())
                                } else {
                                    n.to_string()
                                }
                            } else {
                                n.to_string()
                            }
                        }
                        Value::Array(arr) => {
                            // Format arrays compactly
                            if arr.len() <= 3 {
                                format!("{:?}", arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                            } else {
                                format!("[{} items]", arr.len())
                            }
                        }
                        _ => v.to_string(),
                    };
                    format!("{}: {}", k, display_val)
                })
                .collect();
            parts.join(", ")
        }
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

pub fn run(verbose: bool, json: bool) -> Result<()> {
    let ctx = get_context()?;
    let result = ctx.doctor_service.run_checks()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!("{}", "Database Health Check".bold());
    println!();

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec!["Check", "Status", "Message"]);

    for (check_name, check_result) in &result.checks {
        let status_cell = match check_result.status.as_str() {
            "pass" => Cell::new("PASS").fg(Color::Green),
            "warning" => Cell::new("WARN").fg(Color::Yellow),
            "error" => Cell::new("ERROR").fg(Color::Red),
            _ => Cell::new(&check_result.status),
        };

        table.add_row(vec![
            Cell::new(check_name),
            status_cell,
            Cell::new(&check_result.message),
        ]);

        if verbose {
            if let Some(details) = &check_result.details {
                for detail in details {
                    // Format the JSON value nicely for display
                    let formatted = format_detail(detail);
                    table.add_row(vec![
                        Cell::new(""),
                        Cell::new(""),
                        Cell::new(format!("  - {}", formatted)),
                    ]);
                }
            }
        }
    }

    println!("{}", table);
    println!();

    // Summary
    println!(
        "Summary: {} passed, {} warnings, {} errors",
        result.summary.passed.to_string().green(),
        result.summary.warnings.to_string().yellow(),
        result.summary.errors.to_string().red(),
    );

    if result.summary.errors > 0 {
        std::process::exit(1);
    }

    Ok(())
}
