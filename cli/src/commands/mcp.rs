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

use super::{get_context, get_treeline_dir};
use treeline_core::services::DemoService;

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
// MCP tool definitions
// =============================================================================

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "status",
                "description": "Get account summary including balances, transaction counts, and connected integrations. Returns a high-level overview of the user's financial data.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            },
            {
                "name": "query",
                "description": "Execute a read-only SQL query against the DuckDB database. Use this for any financial analysis — balances, spending, trends, etc. The database contains tables: accounts, transactions (with tags array), sys_balance_snapshots. Plugin tables live in plugin_<name> schemas.",
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
                }
            },
            {
                "name": "doctor",
                "description": "Run database health checks. Detects orphaned transactions, missing data, and other issues.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
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
                }
            }
        ]
    })
}

// =============================================================================
// Tool execution
// =============================================================================

/// Execute a tool call and return the result as MCP content
fn execute_tool(name: &str, args: &Value) -> Result<Value, String> {
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
                    }
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
