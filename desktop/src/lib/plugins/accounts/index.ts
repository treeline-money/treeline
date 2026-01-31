import type { Plugin, PluginContext, PluginMigration } from "../../sdk/types";
import AccountsView from "./AccountsView.svelte";

// Database migrations - run in order by version when plugin loads
const migrations: PluginMigration[] = [
  {
    version: 1,
    name: "create_overrides_table",
    up: `
      CREATE TABLE IF NOT EXISTS plugin_accounts.overrides (
        account_id VARCHAR PRIMARY KEY,
        classification_override VARCHAR,
        exclude_from_net_worth BOOLEAN DEFAULT FALSE
      )
    `,
  },
  {
    version: 2,
    name: "drop_overrides_table",
    // Classification is now stored directly in sys_accounts (migrated by Rust core 011_account_classification.sql)
    up: `DROP TABLE IF EXISTS plugin_accounts.overrides`,
  },
];

export const plugin: Plugin = {
  manifest: {
    id: "accounts",
    name: "Accounts",
    version: "0.1.0",
    description: "View and manage financial accounts",
    author: "Treeline",
    permissions: {
      read: ["accounts", "sys_accounts", "sys_balance_snapshots", "transactions"],
      write: ["sys_accounts", "sys_balance_snapshots"],  // For delete operations
      schemaName: "plugin_accounts",
    },
  },

  migrations,

  activate(context: PluginContext) {
    // Register view
    context.registerView({
      id: "accounts",
      name: "Accounts",
      icon: "🏦",
      component: AccountsView,
    });

    // Add sidebar item
    context.registerSidebarItem({
      sectionId: "main",
      id: "accounts",
      label: "Accounts",
      icon: "🏦",
      viewId: "accounts",
    });

  },
};
