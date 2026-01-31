import type { Plugin, PluginContext } from "../../sdk/types";
import { registry } from "../../sdk/registry";
import { executeQuery } from "../../sdk";
import TransactionsView from "./TransactionsView.svelte";

async function updateUntaggedBadge() {
  try {
    const result = await executeQuery(
      "SELECT COUNT(*) FROM transactions WHERE len(tags) = 0"
    );
    const count = (result.rows?.[0]?.[0] as number) ?? 0;
    registry.updateSidebarBadge("transactions", count > 0 ? count : undefined);
  } catch (e) {
    // Database might not be ready yet, ignore
  }
}

export const plugin: Plugin = {
  manifest: {
    id: "transactions",
    name: "Transactions",
    version: "0.1.0",
    description: "Browse, tag, edit, and split transactions",
    author: "Treeline",
    permissions: {
      // Core plugin: stays in main schema, writes to sys_transactions and sys_transactions_rules
      read: ["transactions", "accounts", "sys_transactions", "sys_transactions_rules"],
      write: ["sys_transactions", "sys_transactions_rules"],
    },
  },

  activate(context: PluginContext) {
    // Register view
    context.registerView({
      id: "transactions",
      name: "Transactions",
      icon: "💳",
      component: TransactionsView,
    });

    // Add sidebar item
    context.registerSidebarItem({
      sectionId: "main",
      id: "transactions",
      label: "Transactions",
      icon: "💳",
      viewId: "transactions",
    });

    // Update badge on activation and data refresh
    updateUntaggedBadge();
    registry.on("data:refresh", updateUntaggedBadge);
  },
};
