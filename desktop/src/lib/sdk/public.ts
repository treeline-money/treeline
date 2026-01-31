/**
 * Treeline Public Plugin SDK
 *
 * This module provides the implementation of the PluginSDK interface.
 * Types are imported from @treeline-money/plugin-sdk npm package.
 */

import type { PluginSDK } from "@treeline-money/plugin-sdk";
import { executeQuery, executeQueryWithParams, type QueryResult, type QueryParam } from "./api";
import { showToast, toast } from "./toast.svelte";
import { themeManager } from "./theme";
import { registry } from "./registry";
import { modKey, formatShortcut, isMac } from "./platform";
import {
  getPluginSettings,
  setPluginSettings,
  readPluginState,
  writePluginState,
} from "./settings";
import {
  SUPPORTED_CURRENCIES,
  DEFAULT_CURRENCY,
  getCurrencySymbol,
  formatCurrency,
  formatCurrencyCompact,
  formatAmount,
} from "../shared/currency";
import {
  getCurrency,
  formatUserCurrency,
  formatUserCurrencyCompact,
  getUserCurrencySymbol,
} from "../shared/currencyStore.svelte";

// Re-export types from npm package for convenience
export type {
  Plugin,
  PluginManifest,
  PluginContext,
  PluginPermissions,
  PluginSDK,
} from "@treeline-money/plugin-sdk";
export type { QueryResult, QueryParam } from "./api";

/**
 * Plugin context passed to Rust for permission validation
 */
interface PluginContext {
  plugin_id: string;
  plugin_schema: string;
  allowed_reads: string[];
  allowed_writes: string[];
}

/**
 * Full permissions object for a plugin (internal use)
 */
export interface PluginTablePermissions {
  read?: string[];       // Tables allowed for SELECT (outside own schema)
  write?: string[];      // Tables allowed for write (outside own schema)
  schemaName?: string;   // Optional schema name override
}

/**
 * Create an SDK instance for a specific plugin.
 * This is called internally when mounting external plugin views.
 *
 * @param pluginId - The plugin's unique identifier
 * @param permissions - Table permissions
 */
export function createPluginSDK(pluginId: string, permissions: PluginTablePermissions): PluginSDK & { getSchemaName: () => string } {
  // Compute plugin schema name: plugin_<id> with hyphens replaced by underscores
  const pluginSchema = permissions.schemaName ?? `plugin_${pluginId.replace(/-/g, '_')}`;

  // Create context for Rust-side permission validation
  const pluginContext: PluginContext = {
    plugin_id: pluginId,
    plugin_schema: pluginSchema,
    allowed_reads: permissions.read ?? [],
    allowed_writes: permissions.write ?? [],
  };

  return {
    // Database - read-only queries with parameters
    query: async <T = Record<string, unknown>>(sql: string, params: QueryParam[] = []): Promise<T[]> => {
      // Permission validation happens in Rust via pluginContext
      const result = await executeQueryWithParams(sql, params, { readonly: true, pluginContext });
      return result.rows as T[];
    },

    // Database - write queries with parameters
    execute: async (sql: string, params: QueryParam[] = []): Promise<{ rowsAffected: number }> => {
      // Permission validation happens in Rust via pluginContext
      const result = await executeQueryWithParams(sql, params, { readonly: false, pluginContext });
      return { rowsAffected: result.row_count };
    },

    // Get the schema name for this plugin
    getSchemaName: () => pluginSchema,

    // Toast notifications
    toast: {
      show: (message: string, description?: string) => showToast({ title: message, type: "info", message: description }),
      success: (message: string, description?: string) => toast.success(message, description),
      error: (message: string, description?: string) => toast.error(message, description),
      warning: (message: string, description?: string) => toast.warning(message, description),
      info: (message: string, description?: string) => toast.info(message, description),
    },

    // Navigation
    openView: (viewId: string, props?: Record<string, any>) => {
      registry.openView(viewId, props);
    },

    // Events
    onDataRefresh: (callback: () => void) => {
      return registry.on("data:refresh", callback);
    },

    emitDataRefresh: () => {
      registry.emit("data:refresh");
    },

    // Badge - update sidebar badge for this plugin
    updateBadge: (count: number | undefined) => {
      registry.updateSidebarBadge(pluginId, count);
    },

    // Theme
    theme: {
      current: () => themeManager.current as "light" | "dark",
      subscribe: (callback: (theme: string) => void) => themeManager.subscribe(callback),
    },

    // Platform
    modKey: modKey() as "Cmd" | "Ctrl",
    formatShortcut,

    // Plugin settings (scoped)
    settings: {
      get: <T extends Record<string, unknown>>() => getPluginSettings<T>(pluginId, {} as T),
      set: <T extends Record<string, unknown>>(settings: T) => setPluginSettings(pluginId, settings),
    },

    // Plugin state (scoped)
    state: {
      read: <T>() => readPluginState<T>(pluginId),
      write: <T>(state: T) => writePluginState(pluginId, state),
    },

    // Currency formatting (uses user's currency preference by default)
    currency: {
      format: (amount: number, currency?: string) => currency ? formatCurrency(amount, currency) : formatUserCurrency(amount),
      formatCompact: (amount: number, currency?: string) => currency ? formatCurrencyCompact(amount, currency) : formatUserCurrencyCompact(amount),
      formatAmount: (amount: number) => formatAmount(amount),
      getSymbol: (currency?: string) => currency ? getCurrencySymbol(currency) : getUserCurrencySymbol(),
      getUserCurrency: () => getCurrency(),
      supportedCurrencies: Object.keys(SUPPORTED_CURRENCIES),
    },
  };
}

// Also export individual functions for core plugins that import directly
export { showToast, modKey, formatShortcut, isMac };

// Currency utilities for core plugins
export {
  SUPPORTED_CURRENCIES,
  DEFAULT_CURRENCY,
  getCurrencySymbol,
  formatCurrency,
  formatCurrencyCompact,
  formatAmount,
};
