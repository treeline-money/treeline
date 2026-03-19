//! MCP (Model Context Protocol) server for Treeline
//!
//! Implements the MCP STDIO transport, exposing Treeline's service layer
//! as tools that AI agents can invoke. This is another adapter in the
//! hexagonal architecture — same as the CLI and Tauri desktop app.
//!
//! Protocol: JSON-RPC 2.0 over STDIO (newline-delimited)
//! Spec: https://modelcontextprotocol.io/specification

use std::io::{self, BufRead, Write};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{get_context, get_treeline_dir, skills};
use treeline_core::services::{DemoService, EncryptionService, KeychainService};

// =============================================================================
// JSON-RPC 2.0 types
// =============================================================================

#[derive(Deserialize)]
#[allow(dead_code)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }
}

// =============================================================================
// MCP server instructions (sent during initialize)
// =============================================================================

/// Build instructions that include encryption status so the client knows
/// immediately if the database is locked.
fn build_instructions() -> String {
    let base = "You are connected to Treeline, a local-first personal finance app backed by DuckDB. Use the query tool for financial analysis and skills_list to discover user-created financial skills.";

    let treeline_dir = get_treeline_dir();
    let db_path = treeline_dir.join("treeline.duckdb");
    let encryption_service = EncryptionService::new(treeline_dir, db_path);

    let is_encrypted = encryption_service.is_encrypted().unwrap_or(false);
    if !is_encrypted {
        return base.to_string();
    }

    // Check if we have a usable key
    let has_env_key =
        std::env::var("TL_DB_KEY").is_ok() || std::env::var("TL_DB_PASSWORD").is_ok();
    let has_keychain_key = KeychainService::get_key().unwrap_or(None).is_some();

    if has_env_key || has_keychain_key {
        return base.to_string();
    }

    format!(
        "{}\n\nIMPORTANT: The database is encrypted and locked. All data tools will fail until the user unlocks it. \
         Ask the user to either open the Treeline app and enter their password, or run 'tl encrypt unlock' in their terminal.",
        base
    )
}

// =============================================================================
// MCP tool definitions
// =============================================================================

pub fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "status",
                "description": "Get account summary including balances, transaction counts, and connected integrations. Returns a high-level overview of the user's financial data.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Account Status",
                    "readOnlyHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "query",
                "description": "Execute a read-only SQL query against the DuckDB database. Use this for any financial analysis — balances, spending, trends, etc. Use the 'schema' tool first to discover available tables and columns.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sql": {
                            "type": "string",
                            "description": "SQL SELECT query to execute"
                        }
                    },
                    "required": ["sql"],
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "SQL Query (Read-Only)",
                    "readOnlyHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "query_write",
                "description": "Execute a SQL query with write access (INSERT, UPDATE, DELETE, CREATE TABLE). Use for modifying data like creating plugin tables or updating records. Always confirm with the user before running write queries.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sql": {
                            "type": "string",
                            "description": "SQL query to execute (may include writes)"
                        }
                    },
                    "required": ["sql"],
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "SQL Query (Read/Write)",
                    "readOnlyHint": false,
                    "destructiveHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "sync",
                "description": "Sync accounts and transactions from connected bank integrations (SimpleFIN, Lunchflow). Pulls latest data from all configured integrations.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "integration": {
                            "type": "string",
                            "description": "Specific integration to sync (optional, syncs all if omitted)"
                        },
                        "dry_run": {
                            "type": "boolean",
                            "description": "Preview changes without applying them",
                            "default": false
                        }
                    },
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Sync Bank Data",
                    "readOnlyHint": false,
                    "destructiveHint": false,
                    "openWorldHint": true
                }
            },
            {
                "name": "tag",
                "description": "Apply tags to transactions. Tags are the primary categorization system in Treeline — transactions can have multiple tags.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tags": {
                            "type": "string",
                            "description": "Comma-separated tags to apply (e.g. \"groceries,food\")"
                        },
                        "transaction_ids": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Transaction UUIDs to tag"
                        },
                        "replace": {
                            "type": "boolean",
                            "description": "Replace existing tags instead of appending",
                            "default": false
                        }
                    },
                    "required": ["tags", "transaction_ids"],
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Tag Transactions",
                    "readOnlyHint": false,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "doctor",
                "description": "Run database health checks. Detects orphaned transactions, missing data, and other issues.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Database Health Check",
                    "readOnlyHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "demo",
                "description": "Manage demo mode. Demo mode loads sample financial data for testing without connecting a real bank.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["on", "off", "status"],
                            "description": "Enable, disable, or check demo mode status"
                        }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Demo Mode",
                    "readOnlyHint": false,
                    "destructiveHint": false,
                    "openWorldHint": false
                }
            },
            {
                "name": "schema",
                "description": "Get database schema (tables, views, and their columns). Use to discover what's queryable before writing SQL. Optionally filter to a specific table or view. Use 'plugins: true' to include plugin schemas, or 'table: \"plugin_budget.categories\"' for a specific plugin table.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "table": {
                            "type": "string",
                            "description": "Optional table or view name. Use schema.table for plugin tables (e.g. 'plugin_budget.categories')."
                        },
                        "plugins": {
                            "type": "boolean",
                            "description": "Include plugin schemas (default: false)",
                            "default": false
                        }
                    },
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Database Schema",
                    "readOnlyHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "skills_list",
                "description": "List available agent skills. Returns skill names, descriptions, and file listings. Skills contain user-created financial knowledge like tax tracking rules, budget targets, and tagging conventions.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "List Skills",
                    "readOnlyHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "skills_read",
                "description": "Read a file from a skill directory. Use with paths from skills_list output, e.g. 'tax-tracking/SKILL.md' or 'budget-mgmt/references/targets.md'.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path relative to the skills directory (e.g. 'tax-tracking/SKILL.md')"
                        }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Read Skill File",
                    "readOnlyHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "encryption_status",
                "description": "Check database encryption and lock status. If the database is encrypted and locked, all other tools will fail until the user unlocks it via the Treeline app or 'tl encrypt unlock' in their terminal.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Encryption Status",
                    "readOnlyHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "version",
                "description": "Get the current Treeline CLI version and check if an update is available. If an update is available, guide the user to run 'tl update' in their terminal. If using the Claude Desktop extension (.mcpb), they'll need to download the latest from https://treeline.money/download",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Version & Update Check",
                    "readOnlyHint": true,
                    "openWorldHint": true
                }
            },
            {
                "name": "skills_write",
                "description": "Write a file to a skill directory. Use to create or update skills on behalf of the user. Path must include skill name and filename, e.g. 'budget-targets/SKILL.md'. Directories are created automatically. SKILL.md files should have YAML frontmatter with name and description fields.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path relative to the skills directory (e.g. 'tax-categories/SKILL.md')"
                        },
                        "content": {
                            "type": "string",
                            "description": "File content to write"
                        }
                    },
                    "required": ["path", "content"],
                    "additionalProperties": false
                },
                "annotations": {
                    "title": "Write Skill File",
                    "readOnlyHint": false,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false
                }
            }
        ]
    })
}

// =============================================================================
// Tool execution
// =============================================================================

/// Execute a tool call and return the result as MCP content
pub fn execute_tool(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "status" => tool_status(),
        "query" => {
            let sql = args
                .get("sql")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: sql")?;
            tool_query(sql, false)
        }
        "query_write" => {
            let sql = args
                .get("sql")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: sql")?;
            tool_query(sql, true)
        }
        "sync" => {
            let integration = args.get("integration").and_then(|v| v.as_str());
            let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
            tool_sync(integration, dry_run)
        }
        "tag" => {
            let tags = args
                .get("tags")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: tags")?;
            let ids = args
                .get("transaction_ids")
                .and_then(|v| v.as_array())
                .ok_or("Missing required parameter: transaction_ids")?
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            let replace = args.get("replace").and_then(|v| v.as_bool()).unwrap_or(false);
            tool_tag(tags, ids, replace)
        }
        "doctor" => tool_doctor(),
        "demo" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: action")?;
            tool_demo(action)
        }
        "schema" => {
            let table = args.get("table").and_then(|v| v.as_str());
            let plugins = args.get("plugins").and_then(|v| v.as_bool()).unwrap_or(false);
            tool_schema(table, plugins)
        }
        "encryption_status" => tool_encryption_status(),
        "version" => tool_version(),
        "skills_list" => skills::mcp_list(),
        "skills_read" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: path")?;
            let content = skills::mcp_read(path)?;
            Ok(json!({ "content": content }))
        }
        "skills_write" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: path")?;
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: content")?;
            let message = skills::mcp_write(path, content)?;
            Ok(json!({ "message": message }))
        }
        _ => Err(format!("Unknown tool: {}", name)),
    }
}

fn tool_status() -> Result<Value, String> {
    let ctx = get_context().map_err(|e| e.to_string())?;
    let status = ctx.status_service.get_status().map_err(|e| e.to_string())?;
    serde_json::to_value(&status).map_err(|e| e.to_string())
}

fn tool_query(sql: &str, allow_writes: bool) -> Result<Value, String> {
    let ctx = get_context().map_err(|e| e.to_string())?;
    let result = if allow_writes {
        ctx.query_service.execute_sql(sql)
    } else {
        ctx.query_service.execute_readonly(sql)
    };
    let result = result.map_err(|e| e.to_string())?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

fn tool_sync(integration: Option<&str>, dry_run: bool) -> Result<Value, String> {
    let ctx = get_context().map_err(|e| e.to_string())?;
    let result = ctx
        .sync_service
        .sync(integration, dry_run, false)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

fn tool_tag(tags: &str, ids: Vec<String>, replace: bool) -> Result<Value, String> {
    let ctx = get_context().map_err(|e| e.to_string())?;
    let tag_list: Vec<String> = tags
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let result = ctx
        .tag_service
        .apply_tags(&ids, &tag_list, replace)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

fn tool_schema(table: Option<&str>, plugins: bool) -> Result<Value, String> {
    let ctx = get_context().map_err(|e| e.to_string())?;

    // Parse dot notation for schema.table
    let (schema_filter, table_filter) = if let Some(filter) = table {
        if let Some((schema, tbl)) = filter.split_once('.') {
            (Some(schema), Some(tbl))
        } else {
            (None, Some(filter))
        }
    } else {
        (None, None)
    };

    let schema_sql = if let Some(sf) = schema_filter {
        format!(
            "SELECT table_name, table_type, table_schema FROM information_schema.tables \
             WHERE table_schema = '{}' ORDER BY table_type, table_name",
            sf
        )
    } else if plugins || table_filter.is_none() {
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
        "SELECT table_name, table_type, table_schema FROM information_schema.tables \
         WHERE table_schema NOT IN ('information_schema', 'pg_catalog') \
         ORDER BY CASE WHEN table_schema = 'main' THEN 0 ELSE 1 END, table_schema, table_type, table_name"
            .to_string()
    };

    let tables_result = ctx.query_service.execute_readonly(&schema_sql).map_err(|e| e.to_string())?;
    let show_schema = plugins || schema_filter.is_some();

    let mut tables = Vec::new();
    for row in &tables_result.rows {
        let name = row[0].as_str().unwrap_or_default();
        let table_type = row[1].as_str().unwrap_or_default();
        let tbl_schema = row[2].as_str().unwrap_or_default();

        if let Some(filter) = table_filter {
            if !name.eq_ignore_ascii_case(filter) {
                continue;
            }
        }

        let describe_sql = format!(
            "SELECT column_name, data_type, CASE WHEN is_nullable = 'YES' THEN true ELSE false END as nullable \
             FROM information_schema.columns WHERE table_schema = '{}' AND table_name = '{}' ORDER BY ordinal_position",
            tbl_schema, name
        );
        let columns_result = ctx.query_service.execute_readonly(&describe_sql).map_err(|e| e.to_string())?;

        let columns: Vec<Value> = columns_result.rows.iter().map(|row| {
            json!({
                "name": row[0].as_str().unwrap_or_default(),
                "type": row[1].as_str().unwrap_or_default(),
                "nullable": row[2].as_bool().unwrap_or(true)
            })
        }).collect();

        let mut entry = json!({
            "name": name,
            "type": table_type,
            "columns": columns
        });
        if show_schema {
            entry.as_object_mut().unwrap().insert("schema".to_string(), json!(tbl_schema));
        }
        tables.push(entry);
    }

    if table_filter.is_some() && tables.is_empty() {
        return Err(format!("Table or view '{}' not found", table.unwrap_or_default()));
    }

    Ok(json!({ "tables": tables }))
}

fn tool_version() -> Result<Value, String> {
    let current_version = env!("CARGO_PKG_VERSION");

    // Check cached update state for latest known version
    let update_state = super::update::UpdateState::load();
    let latest_version = update_state.latest_version.as_deref();

    let update_available = latest_version.map_or(false, |latest| {
        latest != current_version && latest != format!("v{}", current_version)
    });

    let mut result = json!({
        "current_version": current_version,
        "update_available": update_available,
    });

    if let Some(latest) = latest_version {
        result["latest_version"] = json!(latest);
    }

    if update_available {
        result["update_instructions"] = json!({
            "cli": "Run 'tl update' in your terminal",
            "mcpb_extension": "Download the latest from https://treeline.money/download"
        });
    }

    Ok(result)
}

fn tool_encryption_status() -> Result<Value, String> {
    let treeline_dir = get_treeline_dir();
    let db_path = treeline_dir.join("treeline.duckdb");
    let encryption_service = EncryptionService::new(treeline_dir, db_path);

    let mut status = encryption_service.get_status().map_err(|e| e.to_string())?;

    let keychain_available = KeychainService::is_available();
    status.keychain_available = Some(keychain_available);

    if status.encrypted {
        let has_env_key =
            std::env::var("TL_DB_KEY").is_ok() || std::env::var("TL_DB_PASSWORD").is_ok();
        let has_keychain_key = KeychainService::get_key().unwrap_or(None).is_some();
        status.locked = Some(!has_env_key && !has_keychain_key);
    } else {
        status.locked = Some(false);
    }

    serde_json::to_value(&status).map_err(|e| e.to_string())
}

fn tool_doctor() -> Result<Value, String> {
    let ctx = get_context().map_err(|e| e.to_string())?;
    let result = ctx.doctor_service.run_checks().map_err(|e| e.to_string())?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

fn tool_demo(action: &str) -> Result<Value, String> {
    let treeline_dir = get_treeline_dir();
    std::fs::create_dir_all(&treeline_dir).map_err(|e| e.to_string())?;
    let demo_service = DemoService::new(&treeline_dir);

    match action {
        "on" => {
            demo_service.enable().map_err(|e| e.to_string())?;
            Ok(json!({"status": "enabled", "message": "Demo mode enabled. Sample data is ready."}))
        }
        "off" => {
            demo_service.disable(false).map_err(|e| e.to_string())?;
            Ok(json!({"status": "disabled", "message": "Demo mode disabled."}))
        }
        "status" => {
            let enabled = demo_service.is_enabled().map_err(|e| e.to_string())?;
            Ok(json!({"enabled": enabled}))
        }
        _ => Err(format!("Unknown demo action: {}. Use on, off, or status.", action)),
    }
}

// =============================================================================
// MCP protocol handler
// =============================================================================

fn handle_request(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let id = match &req.id {
        Some(id) => id.clone(),
        None => return None, // Notification — no response needed
    };

    let params = req.params.as_ref().cloned().unwrap_or(json!({}));

    match req.method.as_str() {
        "initialize" => {
            let instructions = build_instructions();
            Some(JsonRpcResponse::success(
                id,
                json!({
                    "protocolVersion": "2025-03-26",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "treeline",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": instructions
                }),
            ))
        }

        "tools/list" => {
            Some(JsonRpcResponse::success(id, tool_definitions()))
        }

        "tools/call" => {
            let tool_name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

            match execute_tool(tool_name, &arguments) {
                Ok(result) => Some(JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
                        }]
                    }),
                )),
                Err(err) => Some(JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": err
                        }],
                        "isError": true
                    }),
                )),
            }
        }

        _ => {
            // Unknown method — return method not found per JSON-RPC spec
            Some(JsonRpcResponse::error(id, -32601, format!("Method not found: {}", req.method)))
        }
    }
}

// =============================================================================
// Entry point
// =============================================================================

/// Run the MCP server on STDIO
pub fn run() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Parse JSON-RPC request
        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(req) => req,
            Err(e) => {
                // Parse error
                let resp = JsonRpcResponse::error(
                    Value::Null,
                    -32700,
                    format!("Parse error: {}", e),
                );
                let output = serde_json::to_string(&resp)?;
                writeln!(stdout, "{}", output)?;
                stdout.flush()?;
                continue;
            }
        };

        // Handle request
        if let Some(resp) = handle_request(&req) {
            let output = serde_json::to_string(&resp)?;
            writeln!(stdout, "{}", output)?;
            stdout.flush()?;
        }
    }

    Ok(())
}
