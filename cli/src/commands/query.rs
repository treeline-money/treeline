//! Query command - execute SQL queries against the database

use std::io::{self, Read};
use std::path::Path;

use anyhow::{Context, Result};
use comfy_table::{Table, ContentArrangement};

use super::get_context;

pub fn run(sql: Option<&str>, file: Option<&Path>, format: &str) -> Result<()> {
    // Get SQL from: argument, file, or stdin
    let sql_content = if let Some(sql) = sql {
        sql.to_string()
    } else if let Some(file_path) = file {
        std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read SQL file: {:?}", file_path))?
    } else if atty::isnt(atty::Stream::Stdin) {
        // Read from stdin if not a TTY
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)
            .context("Failed to read SQL from stdin")?;
        buffer
    } else {
        anyhow::bail!("No SQL query provided. Use positional argument, --file, or pipe from stdin.");
    };

    let ctx = get_context()?;
    let result = ctx.query_service.execute(&sql_content)?;

    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "csv" => {
            // CSV output
            println!("{}", result.columns.join(","));
            for row in &result.rows {
                let values: Vec<String> = row.iter().map(value_to_csv).collect();
                println!("{}", values.join(","));
            }
        }
        _ => {
            // Table output
            let mut table = Table::new();
            table.set_content_arrangement(ContentArrangement::Dynamic);
            table.set_header(&result.columns);

            for row in &result.rows {
                let values: Vec<String> = row.iter().map(value_to_string).collect();
                table.add_row(values);
            }

            println!("{}", table);
            println!();
            println!("{} row(s) returned", result.row_count);
        }
    }

    Ok(())
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => v.to_string(),
    }
}

fn value_to_csv(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "".to_string(),
        serde_json::Value::String(s) => {
            if s.contains(',') || s.contains('"') || s.contains('\n') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.clone()
            }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => v.to_string(),
    }
}
