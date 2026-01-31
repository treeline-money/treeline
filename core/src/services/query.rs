//! Query service - SQL query execution

use std::sync::Arc;

use anyhow::Result;

use crate::adapters::duckdb::{DuckDbRepository, QueryResult};

/// Query service for SQL execution
pub struct QueryService {
    repository: Arc<DuckDbRepository>,
}

impl QueryService {
    pub fn new(repository: Arc<DuckDbRepository>) -> Self {
        Self { repository }
    }

    /// Execute a read-only SQL query (SELECT only)
    pub fn execute(&self, sql: &str) -> Result<QueryResult> {
        self.repository.execute_query(sql)
    }

    /// Execute arbitrary SQL (read or write)
    ///
    /// For SELECT queries, returns columns and rows.
    /// For write queries (INSERT/UPDATE/DELETE), returns affected_rows count.
    pub fn execute_sql(&self, sql: &str) -> Result<QueryResult> {
        self.repository.execute_sql(sql)
    }

    /// Execute parameterized SQL (read or write)
    ///
    /// Parameters are passed as JSON values and bound to ? placeholders.
    pub fn execute_sql_with_params(
        &self,
        sql: &str,
        params: &[serde_json::Value],
    ) -> Result<QueryResult> {
        self.repository.execute_sql_with_params(sql, params)
    }
}
