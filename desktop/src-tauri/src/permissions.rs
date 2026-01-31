//! Plugin Permission Validation
//!
//! This module provides SQL-level permission validation for plugins using sqlparser-rs.
//! It parses SQL queries and validates that plugins only access tables they're permitted to use.

use serde::Deserialize;
use sqlparser::ast::{
    Expr, FromTable, FunctionArgumentList, FunctionArguments, ObjectName, Query, Select,
    SelectItem, SetExpr, Statement, TableFactor, TableObject, TableWithJoins, UpdateTableFromKind,
    With,
};
use sqlparser::dialect::DuckDbDialect;
use sqlparser::parser::Parser;
use std::collections::HashSet;

/// Context for plugin permission validation.
/// Passed from TypeScript SDK when executing queries on behalf of a plugin.
#[derive(Debug, Clone, Deserialize)]
pub struct PluginContext {
    /// The plugin's unique identifier (e.g., "goals", "budget")
    pub plugin_id: String,
    /// The plugin's schema name (e.g., "plugin_goals", "plugin_budget")
    pub plugin_schema: String,
    /// Tables the plugin is allowed to read from (outside its own schema)
    pub allowed_reads: Vec<String>,
    /// Tables the plugin is allowed to write to (outside its own schema)
    pub allowed_writes: Vec<String>,
}

/// A table reference extracted from a SQL query
#[derive(Debug, Clone)]
struct TableRef {
    /// The full table name (may include schema, e.g., "plugin_goals.goals" or just "transactions")
    name: String,
    /// Whether this is a write operation (INSERT, UPDATE, DELETE, CREATE, DROP, ALTER)
    is_write: bool,
}

/// Validate that a SQL query only accesses tables the plugin is permitted to use.
///
/// # Arguments
/// * `sql` - The SQL query to validate
/// * `ctx` - The plugin context containing permissions
///
/// # Returns
/// * `Ok(())` if the query is permitted
/// * `Err(String)` with a descriptive error message if validation fails
pub fn validate_query_permissions(sql: &str, ctx: &PluginContext) -> Result<(), String> {
    let dialect = DuckDbDialect {};
    let statements =
        Parser::parse_sql(&dialect, sql).map_err(|e| format!("SQL parse error: {}", e))?;

    for stmt in statements {
        let table_refs = extract_table_references(&stmt);

        for table_ref in table_refs {
            validate_table_access(&table_ref.name, table_ref.is_write, ctx)?;
        }
    }

    Ok(())
}

/// Extract all table references from a SQL statement.
/// Returns a list of (table_name, is_write) pairs.
fn extract_table_references(stmt: &Statement) -> Vec<TableRef> {
    let mut refs = Vec::new();
    let mut cte_names: HashSet<String> = HashSet::new();

    match stmt {
        // SELECT queries - all tables are reads
        Statement::Query(query) => {
            extract_from_query(query, &mut refs, &mut cte_names, false);
        }

        // INSERT - target is write, subquery tables are reads
        Statement::Insert(insert) => {
            // Extract table name from TableObject
            let name = match &insert.table {
                TableObject::TableName(obj_name) => object_name_to_string(obj_name),
                TableObject::TableFunction(func) => func.name.to_string(),
            };
            refs.push(TableRef {
                name,
                is_write: true,
            });

            // Source can be a query
            if let Some(src) = &insert.source {
                extract_from_query(src, &mut refs, &mut cte_names, false);
            }
        }

        // UPDATE - target is write, WHERE/FROM subqueries are reads
        Statement::Update(update) => {
            // Extract target table
            let table_name = extract_table_name_from_table_with_joins(&update.table);
            if let Some(name) = table_name {
                refs.push(TableRef {
                    name,
                    is_write: true,
                });
            }

            // FROM clause tables are reads
            if let Some(from_kind) = &update.from {
                let from_tables = match from_kind {
                    UpdateTableFromKind::BeforeSet(tables) => tables,
                    UpdateTableFromKind::AfterSet(tables) => tables,
                };
                for twj in from_tables {
                    extract_from_table_with_joins(twj, &mut refs, &cte_names, false);
                }
            }

            // WHERE clause may have subqueries
            if let Some(expr) = &update.selection {
                extract_from_expr(expr, &mut refs, &cte_names, false);
            }
        }

        // DELETE - target is write, WHERE subqueries are reads
        Statement::Delete(delete) => {
            // Extract target table from FROM clause
            let from_tables = match &delete.from {
                FromTable::WithFromKeyword(tables) => tables,
                FromTable::WithoutKeyword(tables) => tables,
            };
            for twj in from_tables {
                let table_name = extract_table_name_from_table_with_joins(twj);
                if let Some(name) = table_name {
                    refs.push(TableRef {
                        name,
                        is_write: true,
                    });
                }
            }

            // WHERE clause may have subqueries
            if let Some(expr) = &delete.selection {
                extract_from_expr(expr, &mut refs, &cte_names, false);
            }
        }

        // CREATE TABLE - target is write (DDL)
        Statement::CreateTable(create_table) => {
            let table_name = object_name_to_string(&create_table.name);
            refs.push(TableRef {
                name: table_name,
                is_write: true,
            });

            // AS SELECT clause
            if let Some(q) = &create_table.query {
                extract_from_query(q, &mut refs, &mut cte_names, false);
            }
        }

        // DROP TABLE - target is write (DDL)
        Statement::Drop { names, .. } => {
            for name in names {
                refs.push(TableRef {
                    name: object_name_to_string(name),
                    is_write: true,
                });
            }
        }

        // ALTER TABLE - target is write (DDL)
        Statement::AlterTable(alter_table) => {
            refs.push(TableRef {
                name: object_name_to_string(&alter_table.name),
                is_write: true,
            });
        }

        // CREATE INDEX - the table is a write target
        Statement::CreateIndex(create_index) => {
            refs.push(TableRef {
                name: object_name_to_string(&create_index.table_name),
                is_write: true,
            });
        }

        // CREATE SCHEMA - allowed if it matches plugin schema
        Statement::CreateSchema { schema_name, .. } => {
            // Extract schema name from SchemaName
            let schema = match schema_name {
                sqlparser::ast::SchemaName::Simple(name) => object_name_to_string(name),
                sqlparser::ast::SchemaName::UnnamedAuthorization(ident) => ident.value.clone(),
                sqlparser::ast::SchemaName::NamedAuthorization(name, _) => {
                    object_name_to_string(name)
                }
            };
            refs.push(TableRef {
                name: schema,
                is_write: true,
            });
        }

        // Other statements - ignore or handle as needed
        _ => {}
    }

    refs
}

/// Extract table references from a Query (SELECT with potential CTEs)
fn extract_from_query(
    query: &Query,
    refs: &mut Vec<TableRef>,
    cte_names: &mut HashSet<String>,
    is_write: bool,
) {
    // Process CTEs first
    if let Some(with) = &query.with {
        extract_from_with(with, refs, cte_names);
    }

    // Process the main query body
    extract_from_set_expr(&query.body, refs, cte_names, is_write);
}

/// Extract CTE names and their table references
fn extract_from_with(with: &With, refs: &mut Vec<TableRef>, cte_names: &mut HashSet<String>) {
    for cte in &with.cte_tables {
        // Record CTE name so we don't treat it as a table reference
        cte_names.insert(cte.alias.name.value.to_lowercase());

        // Extract tables from CTE definition
        let mut local_ctes = cte_names.clone();
        extract_from_query(&cte.query, refs, &mut local_ctes, false);
    }
}

/// Extract table references from a SetExpr (SELECT, UNION, etc.)
fn extract_from_set_expr(
    set_expr: &SetExpr,
    refs: &mut Vec<TableRef>,
    cte_names: &HashSet<String>,
    is_write: bool,
) {
    match set_expr {
        SetExpr::Select(select) => {
            extract_from_select(select, refs, cte_names, is_write);
        }
        SetExpr::Query(query) => {
            let mut local_ctes = cte_names.clone();
            extract_from_query(query, refs, &mut local_ctes, is_write);
        }
        SetExpr::SetOperation { left, right, .. } => {
            extract_from_set_expr(left, refs, cte_names, is_write);
            extract_from_set_expr(right, refs, cte_names, is_write);
        }
        SetExpr::Values(_) => {
            // VALUES clause doesn't reference tables
        }
        _ => {
            // Handle other variants as they arise
        }
    }
}

/// Extract table references from a SELECT clause
fn extract_from_select(
    select: &Select,
    refs: &mut Vec<TableRef>,
    cte_names: &HashSet<String>,
    is_write: bool,
) {
    // FROM clause
    for twj in &select.from {
        extract_from_table_with_joins(twj, refs, cte_names, is_write);
    }

    // SELECT items may contain subqueries
    for item in &select.projection {
        match item {
            SelectItem::ExprWithAlias { expr, .. } => {
                extract_from_expr(expr, refs, cte_names, is_write);
            }
            SelectItem::UnnamedExpr(expr) => {
                extract_from_expr(expr, refs, cte_names, is_write);
            }
            _ => {}
        }
    }

    // WHERE clause
    if let Some(expr) = &select.selection {
        extract_from_expr(expr, refs, cte_names, is_write);
    }

    // HAVING clause
    if let Some(expr) = &select.having {
        extract_from_expr(expr, refs, cte_names, is_write);
    }
}

/// Extract table references from a TableWithJoins (FROM clause item)
fn extract_from_table_with_joins(
    twj: &TableWithJoins,
    refs: &mut Vec<TableRef>,
    cte_names: &HashSet<String>,
    is_write: bool,
) {
    extract_from_table_factor(&twj.relation, refs, cte_names, is_write);

    for join in &twj.joins {
        extract_from_table_factor(&join.relation, refs, cte_names, is_write);

        // JOIN ON clause may have subqueries - check the join constraint
        match &join.join_operator {
            sqlparser::ast::JoinOperator::Inner(constraint)
            | sqlparser::ast::JoinOperator::LeftOuter(constraint)
            | sqlparser::ast::JoinOperator::RightOuter(constraint)
            | sqlparser::ast::JoinOperator::FullOuter(constraint)
            | sqlparser::ast::JoinOperator::LeftSemi(constraint)
            | sqlparser::ast::JoinOperator::RightSemi(constraint)
            | sqlparser::ast::JoinOperator::LeftAnti(constraint)
            | sqlparser::ast::JoinOperator::RightAnti(constraint) => {
                if let sqlparser::ast::JoinConstraint::On(expr) = constraint {
                    extract_from_expr(expr, refs, cte_names, is_write);
                }
            }
            _ => {}
        }
    }
}

/// Extract table name from a TableWithJoins (for write targets)
fn extract_table_name_from_table_with_joins(twj: &TableWithJoins) -> Option<String> {
    match &twj.relation {
        TableFactor::Table { name, .. } => Some(object_name_to_string(name)),
        _ => None,
    }
}

/// Extract table references from a TableFactor
fn extract_from_table_factor(
    factor: &TableFactor,
    refs: &mut Vec<TableRef>,
    cte_names: &HashSet<String>,
    is_write: bool,
) {
    match factor {
        TableFactor::Table { name, .. } => {
            let table_name = object_name_to_string(name);
            // Skip if this is a CTE reference
            if !cte_names.contains(&table_name.to_lowercase()) {
                refs.push(TableRef {
                    name: table_name,
                    is_write,
                });
            }
        }
        TableFactor::Derived { subquery, .. } => {
            let mut local_ctes = cte_names.clone();
            extract_from_query(subquery, refs, &mut local_ctes, is_write);
        }
        TableFactor::TableFunction { .. } => {
            // Table functions don't reference tables directly
        }
        TableFactor::NestedJoin {
            table_with_joins, ..
        } => {
            extract_from_table_with_joins(table_with_joins, refs, cte_names, is_write);
        }
        _ => {}
    }
}

/// Extract table references from an expression (handles subqueries)
fn extract_from_expr(
    expr: &Expr,
    refs: &mut Vec<TableRef>,
    cte_names: &HashSet<String>,
    is_write: bool,
) {
    match expr {
        Expr::Subquery(query) => {
            let mut local_ctes = cte_names.clone();
            extract_from_query(query, refs, &mut local_ctes, is_write);
        }
        Expr::InSubquery { subquery, .. } => {
            let mut local_ctes = cte_names.clone();
            extract_from_query(subquery, refs, &mut local_ctes, is_write);
        }
        Expr::Exists { subquery, .. } => {
            let mut local_ctes = cte_names.clone();
            extract_from_query(subquery, refs, &mut local_ctes, is_write);
        }
        Expr::BinaryOp { left, right, .. } => {
            extract_from_expr(left, refs, cte_names, is_write);
            extract_from_expr(right, refs, cte_names, is_write);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            extract_from_expr(inner, refs, cte_names, is_write);
        }
        Expr::Nested(inner) => {
            extract_from_expr(inner, refs, cte_names, is_write);
        }
        Expr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => {
            if let Some(op) = operand {
                extract_from_expr(op, refs, cte_names, is_write);
            }
            // In sqlparser 0.60+, conditions is Vec<CaseWhen> with condition and result fields
            for case_when in conditions {
                extract_from_expr(&case_when.condition, refs, cte_names, is_write);
                extract_from_expr(&case_when.result, refs, cte_names, is_write);
            }
            if let Some(else_expr) = else_result {
                extract_from_expr(else_expr, refs, cte_names, is_write);
            }
        }
        Expr::Function(func) => {
            // Process function arguments - FunctionArguments is now an enum
            if let FunctionArguments::List(FunctionArgumentList { args, .. }) = &func.args {
                for arg in args {
                    match arg {
                        sqlparser::ast::FunctionArg::Unnamed(arg_expr) => {
                            if let sqlparser::ast::FunctionArgExpr::Expr(e) = arg_expr {
                                extract_from_expr(e, refs, cte_names, is_write);
                            }
                        }
                        sqlparser::ast::FunctionArg::Named { arg, .. } => {
                            if let sqlparser::ast::FunctionArgExpr::Expr(e) = arg {
                                extract_from_expr(e, refs, cte_names, is_write);
                            }
                        }
                        sqlparser::ast::FunctionArg::ExprNamed { arg, .. } => {
                            if let sqlparser::ast::FunctionArgExpr::Expr(e) = arg {
                                extract_from_expr(e, refs, cte_names, is_write);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Convert an ObjectName to a string (handles schema-qualified names)
fn object_name_to_string(name: &ObjectName) -> String {
    name.0
        .iter()
        .filter_map(|part| part.as_ident().map(|ident| ident.value.clone()))
        .collect::<Vec<_>>()
        .join(".")
}

/// Validate access to a specific table
fn validate_table_access(table: &str, is_write: bool, ctx: &PluginContext) -> Result<(), String> {
    // Parse table name (may be schema-qualified)
    let (schema, table_name) = if table.contains('.') {
        let parts: Vec<&str> = table.split('.').collect();
        (Some(parts[0].to_lowercase()), parts[1].to_lowercase())
    } else {
        (None, table.to_lowercase())
    };

    // Plugin's own schema is always allowed (read and write)
    if let Some(ref s) = schema {
        if s == &ctx.plugin_schema.to_lowercase() {
            return Ok(());
        }
    }

    // Schema creation for own schema is allowed
    if table.to_lowercase() == ctx.plugin_schema.to_lowercase() {
        return Ok(());
    }

    if is_write {
        // Check for wildcard write permission
        if ctx.allowed_writes.iter().any(|w| w == "*") {
            return Ok(());
        }

        // Check explicit write permissions
        let allowed = ctx.allowed_writes.iter().any(|w| {
            let w_lower = w.to_lowercase();
            w_lower == table.to_lowercase()
                || w_lower == format!("{}.{}", schema.as_deref().unwrap_or("main"), table_name)
                || (schema.is_none() && w_lower == table_name)
        });

        if !allowed {
            return Err(format!(
                "Plugin '{}' cannot write to '{}'. Declared writes: {:?}",
                ctx.plugin_id, table, ctx.allowed_writes
            ));
        }
    } else {
        // Check explicit read permissions (or wildcard)
        if ctx.allowed_reads.iter().any(|r| r == "*") {
            return Ok(());
        }

        let allowed = ctx.allowed_reads.iter().any(|r| {
            let r_lower = r.to_lowercase();
            r_lower == table.to_lowercase()
                || r_lower == format!("{}.{}", schema.as_deref().unwrap_or("main"), table_name)
                || (schema.is_none() && r_lower == table_name)
        });

        if !allowed {
            return Err(format!(
                "Plugin '{}' cannot read from '{}'. Declared reads: {:?}",
                ctx.plugin_id, table, ctx.allowed_reads
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> PluginContext {
        PluginContext {
            plugin_id: "goals".to_string(),
            plugin_schema: "plugin_goals".to_string(),
            allowed_reads: vec!["accounts".to_string(), "sys_balance_snapshots".to_string()],
            allowed_writes: vec![],
        }
    }

    fn test_ctx_with_writes() -> PluginContext {
        PluginContext {
            plugin_id: "goals".to_string(),
            plugin_schema: "plugin_goals".to_string(),
            allowed_reads: vec!["accounts".to_string(), "sys_balance_snapshots".to_string()],
            allowed_writes: vec!["sys_transactions".to_string()],
        }
    }

    // ============================================================================
    // Basic SELECT Tests
    // ============================================================================

    #[test]
    fn test_select_allowed_table() {
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECT * FROM accounts", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_denied_table() {
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECT * FROM transactions", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    #[test]
    fn test_select_own_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECT * FROM plugin_goals.goals", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_own_schema_unqualified_table() {
        // Plugin creates tables in own schema but may reference without schema prefix
        // when USE plugin_goals; is active
        let ctx = test_ctx();
        // This should still work because we allow access to plugin_schema tables
        let result = validate_query_permissions("SELECT * FROM plugin_goals.settings", &ctx);
        assert!(result.is_ok());
    }

    // ============================================================================
    // INSERT Tests
    // ============================================================================

    #[test]
    fn test_insert_own_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "INSERT INTO plugin_goals.goals (id, name) VALUES ('1', 'test')",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_insert_denied() {
        let ctx = test_ctx();
        let result =
            validate_query_permissions("INSERT INTO sys_transactions (id) VALUES ('1')", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    #[test]
    fn test_insert_with_explicit_write_permission() {
        let ctx = test_ctx_with_writes();
        let result = validate_query_permissions(
            "INSERT INTO sys_transactions (id, amount) VALUES ('1', 100)",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_insert_select_allowed_source() {
        // INSERT INTO own schema, SELECT FROM allowed table
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "INSERT INTO plugin_goals.goal_accounts SELECT id, name FROM accounts",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_insert_select_denied_source() {
        // INSERT INTO own schema, SELECT FROM denied table
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "INSERT INTO plugin_goals.cached_tx SELECT * FROM sys_transactions",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    #[test]
    fn test_insert_denied_target_allowed_source() {
        // INSERT INTO denied table, SELECT FROM allowed table
        let ctx = test_ctx();
        let result =
            validate_query_permissions("INSERT INTO sys_transactions SELECT * FROM accounts", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    // ============================================================================
    // UPDATE Tests
    // ============================================================================

    #[test]
    fn test_update_own_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "UPDATE plugin_goals.goals SET name = 'new' WHERE id = '1'",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_denied_table() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "UPDATE sys_transactions SET amount = 0 WHERE id = '1'",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    #[test]
    fn test_update_with_subquery_in_where() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "UPDATE plugin_goals.goals SET balance = 100 WHERE account_id IN (SELECT id FROM accounts)",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_with_denied_subquery_in_where() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "UPDATE plugin_goals.goals SET balance = 100 WHERE account_id IN (SELECT account_id FROM sys_transactions)",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    #[test]
    fn test_update_with_from_clause() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "UPDATE plugin_goals.goals g SET balance = a.balance FROM accounts a WHERE g.account_id = a.id",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_with_denied_from_clause() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "UPDATE plugin_goals.goals g SET amount = t.amount FROM sys_transactions t WHERE g.tx_id = t.id",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    // ============================================================================
    // DELETE Tests
    // ============================================================================

    #[test]
    fn test_delete_own_schema() {
        let ctx = test_ctx();
        let result =
            validate_query_permissions("DELETE FROM plugin_goals.goals WHERE id = '1'", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_delete_denied_table() {
        let ctx = test_ctx();
        let result =
            validate_query_permissions("DELETE FROM sys_transactions WHERE id = '1'", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    #[test]
    fn test_delete_with_subquery() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "DELETE FROM plugin_goals.goals WHERE account_id IN (SELECT id FROM accounts)",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_delete_with_denied_subquery() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "DELETE FROM plugin_goals.goals WHERE tx_id IN (SELECT id FROM sys_transactions)",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    // ============================================================================
    // DDL Tests (CREATE, DROP, ALTER)
    // ============================================================================

    #[test]
    fn test_create_table_own_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "CREATE TABLE IF NOT EXISTS plugin_goals.goals (id VARCHAR PRIMARY KEY)",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_table_denied_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "CREATE TABLE main.malicious_table (id VARCHAR PRIMARY KEY)",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    #[test]
    fn test_create_table_as_select_allowed() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "CREATE TABLE plugin_goals.account_cache AS SELECT id, name FROM accounts",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_table_as_select_denied_source() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "CREATE TABLE plugin_goals.tx_cache AS SELECT * FROM sys_transactions",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    #[test]
    fn test_drop_table_own_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions("DROP TABLE plugin_goals.goals", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_drop_table_denied() {
        let ctx = test_ctx();
        let result = validate_query_permissions("DROP TABLE sys_transactions", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    #[test]
    fn test_alter_table_own_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "ALTER TABLE plugin_goals.goals ADD COLUMN description VARCHAR",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_alter_table_denied() {
        let ctx = test_ctx();
        let result =
            validate_query_permissions("ALTER TABLE accounts ADD COLUMN malicious VARCHAR", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    #[test]
    fn test_create_index_own_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "CREATE INDEX idx_goals_name ON plugin_goals.goals(name)",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_index_denied() {
        let ctx = test_ctx();
        let result =
            validate_query_permissions("CREATE INDEX idx_accounts_name ON accounts(name)", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    #[test]
    fn test_create_schema_own() {
        let ctx = test_ctx();
        let result = validate_query_permissions("CREATE SCHEMA IF NOT EXISTS plugin_goals", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_schema_denied() {
        let ctx = test_ctx();
        let result = validate_query_permissions("CREATE SCHEMA malicious_schema", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    // ============================================================================
    // CTE (Common Table Expression) Tests
    // ============================================================================

    #[test]
    fn test_cte_not_treated_as_table() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "WITH monthly AS (SELECT * FROM accounts) SELECT * FROM monthly",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_cte_with_denied_source() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "WITH tx_summary AS (SELECT * FROM sys_transactions) SELECT * FROM tx_summary",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    #[test]
    fn test_nested_cte() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "WITH cte1 AS (SELECT * FROM accounts), cte2 AS (SELECT * FROM cte1) SELECT * FROM cte2",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_cte_with_multiple_tables() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "WITH combined AS (SELECT a.id, s.balance FROM accounts a JOIN sys_balance_snapshots s ON a.id = s.account_id) SELECT * FROM combined",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_cte_shadowing_real_table() {
        // CTE named 'accounts' should shadow the real accounts table
        // The query only reads from the CTE, not the real table
        let ctx = PluginContext {
            plugin_id: "test".to_string(),
            plugin_schema: "plugin_test".to_string(),
            allowed_reads: vec![], // No read permissions
            allowed_writes: vec![],
        };
        let result = validate_query_permissions(
            "WITH accounts AS (SELECT 1 AS id) SELECT * FROM accounts",
            &ctx,
        );
        assert!(result.is_ok());
    }

    // ============================================================================
    // UNION / Set Operation Tests
    // ============================================================================

    #[test]
    fn test_union_allowed_tables() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT id FROM accounts UNION SELECT account_id FROM sys_balance_snapshots",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_union_with_denied_table() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT id FROM accounts UNION SELECT id FROM sys_transactions",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    #[test]
    fn test_union_all_multiple() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT id FROM accounts UNION ALL SELECT id FROM accounts UNION ALL SELECT account_id FROM sys_balance_snapshots",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_intersect() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT id FROM accounts INTERSECT SELECT account_id FROM sys_balance_snapshots",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_except_with_denied() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT id FROM accounts EXCEPT SELECT account_id FROM sys_transactions",
            &ctx,
        );
        assert!(result.is_err());
    }

    // ============================================================================
    // Wildcard Permission Tests
    // ============================================================================

    #[test]
    fn test_wildcard_read() {
        let ctx = PluginContext {
            plugin_id: "query".to_string(),
            plugin_schema: "plugin_query".to_string(),
            allowed_reads: vec!["*".to_string()],
            allowed_writes: vec![],
        };
        let result = validate_query_permissions("SELECT * FROM any_table_at_all", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_wildcard_write() {
        let ctx = PluginContext {
            plugin_id: "admin".to_string(),
            plugin_schema: "plugin_admin".to_string(),
            allowed_reads: vec!["*".to_string()],
            allowed_writes: vec!["*".to_string()],
        };
        let result = validate_query_permissions("INSERT INTO any_table (id) VALUES ('1')", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_wildcard_read_no_write() {
        let ctx = PluginContext {
            plugin_id: "query".to_string(),
            plugin_schema: "plugin_query".to_string(),
            allowed_reads: vec!["*".to_string()],
            allowed_writes: vec![],
        };
        let result = validate_query_permissions("INSERT INTO some_table (id) VALUES ('1')", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot write"));
    }

    // ============================================================================
    // JOIN Tests
    // ============================================================================

    #[test]
    fn test_join_tables() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts a JOIN sys_balance_snapshots s ON a.id = s.account_id",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_join_with_denied_table() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts a JOIN sys_transactions t ON a.id = t.account_id",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    #[test]
    fn test_left_join() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts a LEFT JOIN sys_balance_snapshots s ON a.id = s.account_id",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_joins() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts a JOIN sys_balance_snapshots s1 ON a.id = s1.account_id JOIN sys_balance_snapshots s2 ON a.id = s2.account_id",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_join_with_subquery_in_on() {
        let ctx = test_ctx();
        // Subquery in ON clause
        let result = validate_query_permissions(
            "SELECT * FROM accounts a JOIN sys_balance_snapshots s ON a.id = s.account_id AND s.balance > (SELECT AVG(balance) FROM sys_balance_snapshots)",
            &ctx,
        );
        assert!(result.is_ok());
    }

    // ============================================================================
    // Subquery Tests
    // ============================================================================

    #[test]
    fn test_subquery() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts WHERE id IN (SELECT account_id FROM sys_balance_snapshots)",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_subquery_denied() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts WHERE id IN (SELECT account_id FROM sys_transactions)",
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_exists_subquery() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts a WHERE EXISTS (SELECT 1 FROM sys_balance_snapshots s WHERE s.account_id = a.id)",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_exists_subquery_denied() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts a WHERE EXISTS (SELECT 1 FROM sys_transactions t WHERE t.account_id = a.id)",
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_scalar_subquery() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT id, (SELECT COUNT(*) FROM sys_balance_snapshots WHERE account_id = a.id) as snapshot_count FROM accounts a",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_derived_table() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM (SELECT id, name FROM accounts) AS subq",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_derived_table_denied() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM (SELECT * FROM sys_transactions) AS subq",
            &ctx,
        );
        assert!(result.is_err());
    }

    // ============================================================================
    // Case Sensitivity Tests
    // ============================================================================

    #[test]
    fn test_case_insensitive_table_name() {
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECT * FROM ACCOUNTS", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_case_insensitive_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECT * FROM PLUGIN_GOALS.goals", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mixed_case_schema() {
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECT * FROM Plugin_Goals.Goals", &ctx);
        assert!(result.is_ok());
    }

    // ============================================================================
    // Multiple Statement Tests
    // ============================================================================

    #[test]
    fn test_multiple_statements_all_allowed() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts; SELECT * FROM sys_balance_snapshots;",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_statements_one_denied() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM accounts; SELECT * FROM sys_transactions;",
            &ctx,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    #[test]
    fn test_multiple_statements_write_and_read() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "INSERT INTO plugin_goals.goals (id) VALUES ('1'); SELECT * FROM accounts;",
            &ctx,
        );
        assert!(result.is_ok());
    }

    // ============================================================================
    // Schema-Qualified Table Tests
    // ============================================================================

    #[test]
    fn test_schema_qualified_allowed() {
        let ctx = PluginContext {
            plugin_id: "goals".to_string(),
            plugin_schema: "plugin_goals".to_string(),
            allowed_reads: vec!["main.accounts".to_string()],
            allowed_writes: vec![],
        };
        let result = validate_query_permissions("SELECT * FROM main.accounts", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_schema_qualified_in_permissions() {
        // Permission is "main.accounts", query uses "accounts"
        let ctx = PluginContext {
            plugin_id: "goals".to_string(),
            plugin_schema: "plugin_goals".to_string(),
            allowed_reads: vec!["main.accounts".to_string()],
            allowed_writes: vec![],
        };
        let result = validate_query_permissions("SELECT * FROM accounts", &ctx);
        // This should work because unqualified names assume "main" schema
        assert!(result.is_ok());
    }

    // ============================================================================
    // Edge Cases and Error Handling
    // ============================================================================

    #[test]
    fn test_invalid_sql() {
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECTT * FROMM accounts", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("SQL parse error"));
    }

    #[test]
    fn test_empty_query() {
        let ctx = test_ctx();
        let result = validate_query_permissions("", &ctx);
        // Empty string should parse as empty statement list, which is OK
        assert!(result.is_ok());
    }

    #[test]
    fn test_comment_only() {
        let ctx = test_ctx();
        let result = validate_query_permissions("-- just a comment", &ctx);
        // Comments should parse fine
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_permissions_at_all() {
        let ctx = PluginContext {
            plugin_id: "isolated".to_string(),
            plugin_schema: "plugin_isolated".to_string(),
            allowed_reads: vec![],
            allowed_writes: vec![],
        };
        // Should still be able to access own schema
        let result = validate_query_permissions("SELECT * FROM plugin_isolated.data", &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_permissions_denied_external() {
        let ctx = PluginContext {
            plugin_id: "isolated".to_string(),
            plugin_schema: "plugin_isolated".to_string(),
            allowed_reads: vec![],
            allowed_writes: vec![],
        };
        let result = validate_query_permissions("SELECT * FROM accounts", &ctx);
        assert!(result.is_err());
    }

    // ============================================================================
    // CASE Expression Tests
    // ============================================================================

    #[test]
    fn test_case_expression_with_subquery() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT CASE WHEN id IN (SELECT account_id FROM sys_balance_snapshots) THEN 'has_balance' ELSE 'no_balance' END FROM accounts",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_case_expression_with_denied_subquery() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT CASE WHEN id IN (SELECT account_id FROM sys_transactions) THEN 'has_tx' ELSE 'no_tx' END FROM accounts",
            &ctx,
        );
        assert!(result.is_err());
    }

    // ============================================================================
    // Function Tests
    // ============================================================================

    #[test]
    fn test_aggregate_function() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT COUNT(*), SUM(balance) FROM sys_balance_snapshots",
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_function_with_subquery_arg() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT COALESCE((SELECT balance FROM sys_balance_snapshots LIMIT 1), 0)",
            &ctx,
        );
        assert!(result.is_ok());
    }

    // ============================================================================
    // Complex Real-World Query Tests
    // ============================================================================

    #[test]
    fn test_complex_analytics_query() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            r#"
            WITH latest_balances AS (
                SELECT account_id, balance, date,
                       ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY date DESC) as rn
                FROM sys_balance_snapshots
            )
            SELECT a.id, a.name, lb.balance
            FROM accounts a
            LEFT JOIN latest_balances lb ON a.id = lb.account_id AND lb.rn = 1
            WHERE a.id IN (SELECT DISTINCT account_id FROM sys_balance_snapshots WHERE balance > 0)
            ORDER BY lb.balance DESC
            "#,
            &ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_complex_query_with_denied_table() {
        let ctx = test_ctx();
        let result = validate_query_permissions(
            r#"
            WITH tx_summary AS (
                SELECT account_id, SUM(amount) as total
                FROM sys_transactions
                GROUP BY account_id
            )
            SELECT a.name, ts.total
            FROM accounts a
            JOIN tx_summary ts ON a.id = ts.account_id
            "#,
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_typical_usage() {
        // Typical plugin: read from allowed tables, write to own schema
        let ctx = test_ctx();
        let result = validate_query_permissions(
            r#"
            INSERT INTO plugin_goals.account_balances (account_id, balance, as_of)
            SELECT s.account_id, s.balance, s.date
            FROM sys_balance_snapshots s
            JOIN accounts a ON s.account_id = a.id
            WHERE s.date = (SELECT MAX(date) FROM sys_balance_snapshots WHERE account_id = s.account_id)
            "#,
            &ctx,
        );
        assert!(result.is_ok());
    }

    // ============================================================================
    // DuckDB-Specific Syntax Tests
    // These test DuckDB syntax features that require sqlparser 0.60+
    // ============================================================================

    #[test]
    fn test_duckdb_struct_literal() {
        // DuckDB struct literal syntax: {'field': value}
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT {'name': 'test', 'value': 123} AS my_struct FROM accounts",
            &ctx,
        );
        assert!(
            result.is_ok(),
            "Struct literal syntax should parse: {:?}",
            result
        );
    }

    #[test]
    fn test_duckdb_list_syntax() {
        // DuckDB list/array syntax
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECT [1, 2, 3] AS my_list FROM accounts", &ctx);
        assert!(result.is_ok(), "List syntax should parse: {:?}", result);
    }

    #[test]
    fn test_duckdb_filter_aggregate() {
        // FILTER clause on aggregates - common in financial queries
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT COUNT(*) FILTER (WHERE balance > 0) FROM sys_balance_snapshots",
            &ctx,
        );
        assert!(
            result.is_ok(),
            "FILTER aggregate syntax should parse: {:?}",
            result
        );
    }

    #[test]
    fn test_duckdb_exclude_columns() {
        // EXCLUDE syntax for selecting all columns except some
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECT * EXCLUDE (id) FROM accounts", &ctx);
        assert!(result.is_ok(), "EXCLUDE syntax should parse: {:?}", result);
    }

    #[test]
    fn test_duckdb_replace_columns() {
        // REPLACE syntax for transforming columns in SELECT *
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * REPLACE (balance * 100 AS balance) FROM sys_balance_snapshots",
            &ctx,
        );
        assert!(result.is_ok(), "REPLACE syntax should parse: {:?}", result);
    }

    #[test]
    fn test_duckdb_group_by_all() {
        // GROUP BY ALL - automatically groups by all non-aggregate columns
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT account_id, SUM(balance) FROM sys_balance_snapshots GROUP BY ALL",
            &ctx,
        );
        assert!(
            result.is_ok(),
            "GROUP BY ALL syntax should parse: {:?}",
            result
        );
    }

    #[test]
    fn test_duckdb_qualify_clause() {
        // QUALIFY clause for filtering window function results
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT * FROM sys_balance_snapshots QUALIFY row_number() OVER (PARTITION BY account_id ORDER BY date DESC) = 1",
            &ctx,
        );
        assert!(result.is_ok(), "QUALIFY syntax should parse: {:?}", result);
    }

    #[test]
    fn test_duckdb_string_concat() {
        // String concatenation with ||
        let ctx = test_ctx();
        let result =
            validate_query_permissions("SELECT name || ' - ' || id AS label FROM accounts", &ctx);
        assert!(
            result.is_ok(),
            "String concat syntax should parse: {:?}",
            result
        );
    }

    #[test]
    fn test_duckdb_list_aggregate() {
        // list_agg / array_agg functions
        let ctx = test_ctx();
        let result = validate_query_permissions(
            "SELECT account_id, list(balance) AS balances FROM sys_balance_snapshots GROUP BY account_id",
            &ctx,
        );
        assert!(
            result.is_ok(),
            "list() aggregate should parse: {:?}",
            result
        );
    }

    #[test]
    fn test_duckdb_unnest() {
        // UNNEST for expanding arrays
        let ctx = test_ctx();
        let result = validate_query_permissions("SELECT unnest([1, 2, 3]) AS num", &ctx);
        assert!(result.is_ok(), "UNNEST syntax should parse: {:?}", result);
    }
}
