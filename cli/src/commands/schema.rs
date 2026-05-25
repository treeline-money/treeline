//! Schema command - database schema introspection for agents and humans
//!
//! Shows tables, views, and their columns. Designed for agents to discover
//! what's queryable without reading documentation.

use anyhow::Result;
use comfy_table::{ContentArrangement, Table};
use serde::Serialize;

use super::get_context;

#[derive(Serialize)]
struct SchemaOutput {
    tables: Vec<TableInfo>,
}

#[derive(Serialize)]
struct TableInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<String>,
    name: String,
    table_type: String,
    columns: Vec<ColumnInfo>,
}

#[derive(Serialize)]
struct ColumnInfo {
    name: String,
    data_type: String,
    nullable: bool,
}

pub fn run(table: Option<&str>, plugins: bool, json: bool) -> Result<()> {
    let ctx = get_context()?;

    // Parse table filter — supports "plugin_budget.categories" dot notation
    let (schema_filter, table_filter) = if let Some(filter) = table {
        if let Some((schema, tbl)) = filter.split_once('.') {
            (Some(schema.to_string()), Some(tbl.to_string()))
        } else {
            (None, Some(filter.to_string()))
        }
    } else {
        (None, None)
    };

    // Determine which schemas to query
    let schema_sql = if let Some(ref sf) = schema_filter {
        format!(
            "SELECT table_name, table_type, table_schema FROM information_schema.tables \
             WHERE table_schema = '{}' ORDER BY table_type, table_name",
            sf
        )
    } else if plugins || table_filter.is_none() {
        // With --plugins or when listing all, include plugin schemas
        let where_clause = if plugins {
            "WHERE table_schema NOT IN ('information_schema', 'pg_catalog')"
        } else {
            "WHERE table_schema = 'main'"
        };
        format!(
            "SELECT table_name, table_type, table_schema FROM information_schema.tables \
             {} ORDER BY table_schema, table_type, table_name",
            where_clause
        )
    } else {
        // Specific table name — search main first, then plugin schemas
        "SELECT table_name, table_type, table_schema FROM information_schema.tables \
         WHERE table_schema NOT IN ('information_schema', 'pg_catalog') \
         ORDER BY CASE WHEN table_schema = 'main' THEN 0 ELSE 1 END, table_schema, table_type, table_name"
            .to_string()
    };

    let tables_result = ctx.query_service.execute_readonly(&schema_sql)?;

    let mut output = SchemaOutput { tables: Vec::new() };

    let show_schema = plugins || schema_filter.is_some();

    for row in &tables_result.rows {
        let name = row[0].as_str().unwrap_or_default();
        let table_type = row[1].as_str().unwrap_or_default();
        let table_schema = row[2].as_str().unwrap_or_default();

        // Apply table name filter
        if let Some(ref filter) = table_filter {
            if !name.eq_ignore_ascii_case(filter) {
                continue;
            }
        }

        // Get columns for this table/view
        let describe_sql = format!(
            "SELECT column_name, data_type, CASE WHEN is_nullable = 'YES' THEN true ELSE false END as nullable \
             FROM information_schema.columns WHERE table_schema = '{}' AND table_name = '{}' ORDER BY ordinal_position",
            table_schema, name
        );
        let columns_result = ctx.query_service.execute_readonly(&describe_sql)?;

        let columns: Vec<ColumnInfo> = columns_result
            .rows
            .iter()
            .map(|row| ColumnInfo {
                name: row[0].as_str().unwrap_or_default().to_string(),
                data_type: row[1].as_str().unwrap_or_default().to_string(),
                nullable: row[2].as_bool().unwrap_or(true),
            })
            .collect();

        output.tables.push(TableInfo {
            schema: if show_schema {
                Some(table_schema.to_string())
            } else {
                None
            },
            name: name.to_string(),
            table_type: table_type.to_string(),
            columns,
        });
    }

    if table_filter.is_some() && output.tables.is_empty() {
        let filter_display = table.unwrap_or_default();
        anyhow::bail!("Table or view '{}' not found", filter_display);
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for table_info in &output.tables {
            let type_label = match table_info.table_type.as_str() {
                "VIEW" => "view",
                "BASE TABLE" => "table",
                _ => &table_info.table_type,
            };
            let display_name = if let Some(ref schema) = table_info.schema {
                format!("{}.{}", schema, table_info.name)
            } else {
                table_info.name.clone()
            };
            println!("\n{} ({})", display_name, type_label);
            println!("{}", "-".repeat(display_name.len() + type_label.len() + 3));

            let mut tbl = Table::new();
            tbl.set_content_arrangement(ContentArrangement::Dynamic);
            tbl.set_header(vec!["Column", "Type", "Nullable"]);

            for col in &table_info.columns {
                tbl.add_row(vec![
                    col.name.clone(),
                    col.data_type.clone(),
                    if col.nullable { "yes" } else { "no" }.to_string(),
                ]);
            }

            println!("{}", tbl);
        }
        println!();
    }

    Ok(())
}
