import type { Plugin, PluginContext, PluginMigration } from "../../sdk/types";
import AIBuilderView from "./AIBuilderView.svelte";

// Database migrations for storing chat history and generated plugins
const migrations: PluginMigration[] = [
  {
    version: 1,
    name: "create_conversations_table",
    up: `
      CREATE TABLE IF NOT EXISTS plugin_ai_builder.conversations (
        conversation_id INTEGER PRIMARY KEY,
        plugin_id VARCHAR,
        title VARCHAR NOT NULL,
        created_at TIMESTAMP,
        updated_at TIMESTAMP
      );
      CREATE INDEX IF NOT EXISTS idx_conversations_updated
        ON plugin_ai_builder.conversations(updated_at DESC)
    `,
  },
  {
    version: 2,
    name: "create_messages_table",
    up: `
      CREATE TABLE IF NOT EXISTS plugin_ai_builder.messages (
        message_id INTEGER PRIMARY KEY,
        conversation_id INTEGER NOT NULL,
        role VARCHAR NOT NULL,
        content TEXT NOT NULL,
        created_at TIMESTAMP,
        FOREIGN KEY (conversation_id) REFERENCES plugin_ai_builder.conversations(conversation_id)
      );
      CREATE INDEX IF NOT EXISTS idx_messages_conversation
        ON plugin_ai_builder.messages(conversation_id, created_at)
    `,
  },
  {
    version: 3,
    name: "create_generated_plugins_table",
    up: `
      CREATE TABLE IF NOT EXISTS plugin_ai_builder.generated_plugins (
        plugin_id VARCHAR PRIMARY KEY,
        name VARCHAR NOT NULL,
        description TEXT,
        version VARCHAR DEFAULT '0.1.0',
        source_code TEXT NOT NULL,
        manifest_json TEXT NOT NULL,
        conversation_id INTEGER,
        created_at TIMESTAMP,
        updated_at TIMESTAMP,
        FOREIGN KEY (conversation_id) REFERENCES plugin_ai_builder.conversations(conversation_id)
      )
    `,
  },
  {
    version: 4,
    name: "add_sequences",
    up: `
      CREATE SEQUENCE IF NOT EXISTS plugin_ai_builder.seq_conversation_id START 1;
      CREATE SEQUENCE IF NOT EXISTS plugin_ai_builder.seq_message_id START 1
    `,
  },
];

export const plugin: Plugin = {
  manifest: {
    id: "ai-builder",
    name: "AI Plugin Builder",
    version: "0.1.0",
    description: "Build plugins by describing what you want in natural language",
    author: "Treeline",
    permissions: {
      // Need read access to understand schema for generating useful plugins
      read: ["*"],
      write: [],
      schemaName: "plugin_ai_builder",
    },
  },

  migrations,

  activate(context: PluginContext) {
    // Register the main view
    context.registerView({
      id: "ai-builder",
      name: "Plugin Builder",
      icon: "🤖",
      component: AIBuilderView,
    });

    // Add sidebar item
    context.registerSidebarItem({
      sectionId: "main",
      id: "ai-builder",
      label: "Plugin Builder",
      icon: "🤖",
      viewId: "ai-builder",
    });

    // Register command
    context.registerCommand({
      id: "ai-builder.new",
      name: "New AI Plugin",
      execute: () => {
        context.openView("ai-builder");
      },
    });
  },
};
