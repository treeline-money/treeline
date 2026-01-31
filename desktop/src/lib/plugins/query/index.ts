import type { Plugin, PluginContext, PluginMigration } from "../../sdk/types";
import QueryView from "./QueryView.svelte";

// Database migrations - run in order by version when plugin loads
const migrations: PluginMigration[] = [
  {
    version: 1,
    name: "create_history_table",
    up: `
      CREATE TABLE IF NOT EXISTS plugin_query.history (
        history_id INTEGER PRIMARY KEY,
        query VARCHAR NOT NULL,
        success BOOLEAN DEFAULT TRUE,
        executed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      );
      CREATE INDEX IF NOT EXISTS idx_plugin_query_history_executed
        ON plugin_query.history(executed_at DESC)
    `,
  },
  {
    version: 2,
    name: "add_history_sequence",
    // Use same sequence name as CLI migration 009. Start at 10000 to avoid conflicts.
    up: `CREATE SEQUENCE IF NOT EXISTS plugin_query.seq_history_id START 10000`,
  },
  {
    version: 3,
    name: "fix_history_sequence",
    // Fix sequence conflict: migration 009 created seq starting at 1, but copied existing data.
    // The sequence was never updated to start after max(history_id), causing duplicate key errors.
    // Drop and recreate at 100000 to avoid conflicts (users won't have 100k history entries).
    // CASCADE removes any column defaults referencing the sequence - that's fine since we
    // explicitly call nextval() in INSERT statements anyway.
    up: `
      DROP SEQUENCE IF EXISTS plugin_query.seq_history_id CASCADE;
      CREATE SEQUENCE plugin_query.seq_history_id START 100000
    `,
  },
];

export const plugin: Plugin = {
  manifest: {
    id: "query",
    name: "Query Runner",
    version: "0.1.0",
    description: "Execute SQL queries against your financial data",
    author: "Treeline",
    permissions: {
      // Query plugin uses its own schema (plugin_query) and can read/write any table
      read: ["*"],
      write: ["*"],
      schemaName: "plugin_query",
    },
  },

  migrations,

  activate(context: PluginContext) {
    // Register view - allowMultiple enables opening multiple query tabs
    context.registerView({
      id: "query",
      name: "Query",
      icon: "⚡",
      component: QueryView,
      allowMultiple: true,
    });

    // Add sidebar item
    context.registerSidebarItem({
      sectionId: "main",
      id: "query",
      label: "Query",
      icon: "⚡",
      viewId: "query",
    });
  },
};
