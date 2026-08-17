import type { Plugin, PluginContext, PluginSDK } from "@treeline-money/plugin-sdk";
import HelloWorldView from "./HelloWorldView.svelte";
import { mount, unmount } from "svelte";

// Migrations create your plugin's tables. They run in version order when the
// plugin loads. Uncomment (and add PluginMigration to the import above) to use.
//
// A view named `doctor` in your schema is the one headless surface a plugin
// has: `tl doctor` and the MCP doctor tool read it directly, without running
// any plugin code. One row per check, with columns check_id (stable
// snake_case), name, status ('pass' | 'warning' | 'error'), message, and an
// optional details list.
//
// const migrations: PluginMigration[] = [
//   {
//     version: 1,
//     name: "create_items_table",
//     up: `
//       CREATE TABLE IF NOT EXISTS plugin_hello_world.items (
//         id VARCHAR PRIMARY KEY,
//         name VARCHAR NOT NULL,
//         updated_at TIMESTAMP
//       )
//     `,
//   },
//   {
//     version: 2,
//     name: "create_doctor_view",
//     up: `
//       CREATE OR REPLACE VIEW plugin_hello_world.doctor AS
//       SELECT
//         'stale_items' AS check_id,
//         'Stale items' AS name,
//         CASE WHEN COUNT(*) = 0 THEN 'pass' ELSE 'warning' END AS status,
//         COUNT(*) || ' item(s) not updated in 90 days' AS message,
//         list({'id': id, 'name': name}) AS details
//       FROM plugin_hello_world.items
//       WHERE updated_at < CURRENT_DATE - INTERVAL 90 DAY
//     `,
//   },
// ];

export const plugin: Plugin = {
  manifest: {
    id: "hello-world",
    name: "Hello World",
    version: "0.1.0",
    description: "An example plugin demonstrating the Treeline plugin SDK",
    author: "Your Name",
    // Plugins can read core tables (transactions, accounts) and write to their own schema.
    // Own schema (plugin_{id}.*) is always writable - no declaration needed.
    permissions: {
      read: ["transactions", "accounts"],
      schemaName: "plugin_hello_world",
    },
  },

  // migrations,

  activate(context: PluginContext) {
    console.log("Hello World plugin activated!");

    // Register the view with a mount function
    // Use Lucide icon names like "target", "shield", "repeat", etc.
    context.registerView({
      id: "hello-world-view",
      name: "Hello World",
      icon: "zap",
      mount: (target: HTMLElement, props: { sdk: PluginSDK }) => {
        // Mount the Svelte component using our bundled Svelte runtime
        const instance = mount(HelloWorldView, {
          target,
          props,
        });

        // Return cleanup function
        return () => {
          unmount(instance);
        };
      },
    });

    // Add sidebar item
    context.registerSidebarItem({
      sectionId: "main",
      id: "hello-world",
      label: "Hello World",
      icon: "zap",  // Lucide icon name (or emoji like "👋")
      viewId: "hello-world-view",
    });

    // Register a command (optional)
    context.registerCommand({
      id: "hello-world.greet",
      name: "Say Hello",
      description: "Display a greeting message",
      execute: () => {
        console.log("👋 Hello from the Hello World plugin!");
      },
    });

    console.log("✓ Hello World plugin registered");
  },

  deactivate() {
    console.log("Hello World plugin deactivated");
  },
};
