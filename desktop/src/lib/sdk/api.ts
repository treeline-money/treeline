/**
 * API interface to Tauri backend
 */

import { invoke } from "@tauri-apps/api/core";
import { logger } from "./logging";

export interface QueryResult {
  columns: string[];
  rows: unknown[][];
  row_count: number;
}

/**
 * Plugin context for permission validation in Rust
 */
export interface PluginContext {
  plugin_id: string;
  plugin_schema: string;
  allowed_reads: string[];
  allowed_writes: string[];
}

export interface ExecuteQueryOptions {
  readonly?: boolean;
  pluginContext?: PluginContext;
}

/**
 * Execute a SQL query against the DuckDB database
 * @param query SQL query string
 * @param options.readonly If true (default), opens read-only connection. Set to false for writes.
 */
export async function executeQuery(query: string, options: ExecuteQueryOptions = {}): Promise<QueryResult> {
  const { readonly = true } = options;

  try {
    const jsonString = await invoke<string>("execute_query", { query, readonly });

    // Parse JSON string from Rust backend
    const response = JSON.parse(jsonString);

    return {
      columns: response.columns || [],
      rows: response.rows || [],
      row_count: response.row_count || 0,
    };
  } catch (e) {
    // Log query errors (but NOT the query itself which might contain user data)
    logger.error("query_error", typeof e === "string" ? e : "Query execution failed");
    // Tauri invoke errors come as strings from Rust's Result::Err
    if (typeof e === "string") {
      throw new Error(e);
    }
    throw e;
  }
}

/**
 * Query parameter type - supports primitives and arrays
 */
export type QueryParam = string | number | boolean | null | string[] | number[];

/**
 * Execute a parameterized SQL query against the DuckDB database.
 * This is the SAFE way to execute queries with user-provided values.
 *
 * @param query SQL query string with ? placeholders for parameters
 * @param params Array of values to bind to the ? placeholders
 * @param options.readonly If true (default), opens read-only connection. Set to false for writes.
 *
 * @example
 * // SELECT with parameters
 * const result = await executeQueryWithParams(
 *   "SELECT * FROM transactions WHERE amount > ? AND description LIKE ?",
 *   [100, "%coffee%"]
 * );
 *
 * @example
 * // INSERT with parameters
 * await executeQueryWithParams(
 *   "INSERT INTO categories (name, budget) VALUES (?, ?)",
 *   ["Groceries", 500],
 *   { readonly: false }
 * );
 */
export async function executeQueryWithParams(
  query: string,
  params: QueryParam[] = [],
  options: ExecuteQueryOptions = {}
): Promise<QueryResult> {
  const { readonly = true, pluginContext } = options;

  try {
    const jsonString = await invoke<string>("execute_query_with_params", {
      query,
      params,
      readonly,
      pluginContext: pluginContext ?? null
    });

    // Parse JSON string from Rust backend
    const response = JSON.parse(jsonString);

    return {
      columns: response.columns || [],
      rows: response.rows || [],
      row_count: response.row_count || 0,
    };
  } catch (e) {
    // Log query errors (but NOT the query itself which might contain user data)
    logger.error("query_error", typeof e === "string" ? e : "Query execution failed");
    // Tauri invoke errors come as strings from Rust's Result::Err
    if (typeof e === "string") {
      throw new Error(e);
    }
    throw e;
  }
}

/**
 * Database helper object with convenience methods for parameterized queries.
 * Always use these methods instead of string interpolation to prevent SQL injection.
 */
export const db = {
  /**
   * Execute a SELECT query with parameters (read-only)
   */
  select: <T = Record<string, unknown>>(sql: string, params: QueryParam[] = []): Promise<T[]> =>
    executeQueryWithParams(sql, params, { readonly: true }).then(r => r.rows as T[]),

  /**
   * Execute a write query (INSERT/UPDATE/DELETE) with parameters
   */
  execute: (sql: string, params: QueryParam[] = []): Promise<{ rowsAffected: number }> =>
    executeQueryWithParams(sql, params, { readonly: false }).then(r => ({ rowsAffected: r.row_count })),

  /**
   * Execute a raw query with parameters and return full result
   */
  query: (sql: string, params: QueryParam[] = [], options: ExecuteQueryOptions = {}): Promise<QueryResult> =>
    executeQueryWithParams(sql, params, options),
};

/**
 * Delete an account and all associated data (transactions, balance snapshots)
 * This is a cascading delete handled by rust-core
 */
export async function deleteAccount(accountId: string): Promise<void> {
  try {
    await invoke("delete_account", { accountId });
  } catch (e) {
    if (typeof e === 'string') {
      throw new Error(e);
    }
    throw e;
  }
}
