<script lang="ts">
  import { onMount } from "svelte";
  import { executeQueryWithParams, toast } from "../../sdk";
  import type { PluginContext } from "../../sdk/api";
  import Icon from "../../shared/Icon.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { hotReloadPlugin, loadNewPlugin } from "../index";

  const PLUGIN_ID = "ai-builder";
  const PLUGIN_SCHEMA = "plugin_ai_builder";
  const pluginContext: PluginContext = {
    plugin_id: PLUGIN_ID,
    plugin_schema: PLUGIN_SCHEMA,
    allowed_reads: ["*"],
    allowed_writes: [],
  };

  // Helper functions
  async function query(sql: string, params: (string | number | boolean | null)[] = []) {
    return executeQueryWithParams(sql, params, { readonly: true, pluginContext });
  }

  async function execute(sql: string, params: (string | number | boolean | null)[] = []) {
    return executeQueryWithParams(sql, params, { readonly: false, pluginContext });
  }

  // State
  interface Message {
    id?: number;
    role: "user" | "assistant" | "system";
    content: string;
  }

  interface Conversation {
    id: number;
    pluginId: string | null;
    title: string;
    createdAt: Date;
    updatedAt: Date;
  }

  interface GeneratedPlugin {
    pluginId: string;
    name: string;
    description: string;
    version: string;
    sourceCode: string;
    manifestJson: string;
    conversationId: number | null;
    createdAt: Date;
    updatedAt: Date;
  }

  let conversations = $state<Conversation[]>([]);
  let currentConversation = $state<Conversation | null>(null);
  let messages = $state<Message[]>([]);
  let inputValue = $state("");
  let isLoading = $state(false);
  let isBuilding = $state(false);
  let currentPlugin = $state<GeneratedPlugin | null>(null);
  let showCodePanel = $state(false);
  let apiKeyConfigured = $state(false);
  let showApiKeyModal = $state(false);
  let apiKeyInput = $state("");

  // Schema info for context
  let schemaInfo = $state<string>("");

  // Panel states
  let showConversations = $state(true);

  async function loadConversations() {
    try {
      const result = await query(`
        SELECT conversation_id, plugin_id, title, created_at, updated_at
        FROM plugin_ai_builder.conversations
        ORDER BY updated_at DESC
        LIMIT 50
      `);
      conversations = result.rows.map(row => ({
        id: row[0] as number,
        pluginId: row[1] as string | null,
        title: row[2] as string,
        createdAt: new Date(row[3] as string),
        updatedAt: new Date(row[4] as string),
      }));
    } catch (e) {
      console.error("Failed to load conversations:", e);
    }
  }

  async function loadMessages(conversationId: number) {
    try {
      const result = await query(`
        SELECT message_id, role, content
        FROM plugin_ai_builder.messages
        WHERE conversation_id = ?
        ORDER BY created_at ASC
      `, [conversationId]);
      messages = result.rows.map(row => ({
        id: row[0] as number,
        role: row[1] as "user" | "assistant" | "system",
        content: row[2] as string,
      }));
    } catch (e) {
      console.error("Failed to load messages:", e);
      messages = [];
    }
  }

  async function loadPluginForConversation(conversationId: number) {
    try {
      const result = await query(`
        SELECT plugin_id, name, description, version, source_code, manifest_json, created_at, updated_at
        FROM plugin_ai_builder.generated_plugins
        WHERE conversation_id = ?
        LIMIT 1
      `, [conversationId]);
      if (result.rows.length > 0) {
        const row = result.rows[0];
        currentPlugin = {
          pluginId: row[0] as string,
          name: row[1] as string,
          description: row[2] as string,
          version: row[3] as string,
          sourceCode: row[4] as string,
          manifestJson: row[5] as string,
          conversationId,
          createdAt: new Date(row[6] as string),
          updatedAt: new Date(row[7] as string),
        };
      } else {
        currentPlugin = null;
      }
    } catch (e) {
      console.error("Failed to load plugin:", e);
      currentPlugin = null;
    }
  }

  async function selectConversation(conversation: Conversation) {
    currentConversation = conversation;
    await loadMessages(conversation.id);
    await loadPluginForConversation(conversation.id);
    showCodePanel = currentPlugin !== null;
  }

  async function startNewConversation() {
    currentConversation = null;
    messages = [];
    currentPlugin = null;
    showCodePanel = false;
    inputValue = "";
  }

  async function createConversation(title: string): Promise<number> {
    const now = new Date().toISOString();
    await execute(`
      INSERT INTO plugin_ai_builder.conversations (conversation_id, title, created_at, updated_at)
      VALUES (nextval('plugin_ai_builder.seq_conversation_id'), ?, ?::TIMESTAMP, ?::TIMESTAMP)
    `, [title, now, now]);

    const result = await query(`
      SELECT conversation_id FROM plugin_ai_builder.conversations
      ORDER BY created_at DESC LIMIT 1
    `);
    return result.rows[0][0] as number;
  }

  async function saveMessage(conversationId: number, role: string, content: string) {
    const now = new Date().toISOString();
    await execute(`
      INSERT INTO plugin_ai_builder.messages (message_id, conversation_id, role, content, created_at)
      VALUES (nextval('plugin_ai_builder.seq_message_id'), ?, ?, ?, ?::TIMESTAMP)
    `, [conversationId, role, content, now]);

    // Update conversation timestamp
    await execute(`
      UPDATE plugin_ai_builder.conversations
      SET updated_at = ?::TIMESTAMP
      WHERE conversation_id = ?
    `, [now, conversationId]);
  }

  async function loadSchemaInfo() {
    try {
      const tablesResult = await query(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = 'main'"
      );
      const tableNames = tablesResult.rows.map(row => row[0] as string);

      let schemaText = "Database Schema:\n\n";
      for (const tableName of tableNames) {
        const columnsResult = await query(
          `SELECT column_name, data_type FROM information_schema.columns WHERE table_name = ? ORDER BY ordinal_position`,
          [tableName]
        );
        schemaText += `Table: ${tableName}\n`;
        for (const col of columnsResult.rows) {
          schemaText += `  - ${col[0]}: ${col[1]}\n`;
        }
        schemaText += "\n";
      }
      schemaInfo = schemaText;
    } catch (e) {
      console.error("Failed to load schema:", e);
    }
  }

  async function checkApiKey() {
    try {
      const hasKey = await invoke<boolean>("has_ai_api_key");
      apiKeyConfigured = hasKey;
    } catch (e) {
      console.error("Failed to check API key:", e);
      apiKeyConfigured = false;
    }
  }

  async function saveApiKey() {
    if (!apiKeyInput.trim()) return;
    try {
      await invoke("set_ai_api_key", { key: apiKeyInput.trim() });
      apiKeyConfigured = true;
      showApiKeyModal = false;
      apiKeyInput = "";
      toast.success("API key saved", "You can now generate plugins");
    } catch (e) {
      toast.error("Failed to save API key", e instanceof Error ? e.message : String(e));
    }
  }

  async function sendMessage() {
    if (!inputValue.trim() || isLoading) return;
    if (!apiKeyConfigured) {
      showApiKeyModal = true;
      return;
    }

    const userMessage = inputValue.trim();
    inputValue = "";
    isLoading = true;

    try {
      // Create conversation if needed
      let conversationId = currentConversation?.id;
      if (!conversationId) {
        const title = userMessage.slice(0, 50) + (userMessage.length > 50 ? "..." : "");
        conversationId = await createConversation(title);
        currentConversation = {
          id: conversationId,
          pluginId: null,
          title,
          createdAt: new Date(),
          updatedAt: new Date(),
        };
        await loadConversations();
      }

      // Add user message to UI immediately
      messages = [...messages, { role: "user", content: userMessage }];

      // Save user message
      await saveMessage(conversationId, "user", userMessage);

      // Build context for LLM
      const systemPrompt = buildSystemPrompt();
      const conversationHistory = messages.map(m => ({
        role: m.role,
        content: m.content,
      }));

      // Call LLM
      const response = await invoke<string>("generate_plugin_code", {
        systemPrompt,
        messages: conversationHistory,
        schemaInfo,
      });

      // Parse response to extract code if present
      const { message, plugin } = parseAIResponse(response);

      // Add assistant message
      messages = [...messages, { role: "assistant", content: message }];
      await saveMessage(conversationId, "assistant", message);

      // If plugin code was generated, save it
      if (plugin) {
        await saveGeneratedPlugin(conversationId, plugin);
        currentPlugin = plugin;
        showCodePanel = true;
      }

    } catch (e) {
      console.error("Failed to send message:", e);
      toast.error("Failed to generate response", e instanceof Error ? e.message : String(e));
    } finally {
      isLoading = false;
    }
  }

  function buildSystemPrompt(): string {
    return `You are an expert Treeline plugin developer. You help users create plugins for Treeline, a personal finance application built with Svelte 5 and DuckDB.

## Plugin Structure

Plugins consist of two files:
1. manifest.json - Plugin metadata and permissions
2. src/index.ts - Plugin code that exports a \`plugin\` object

## Manifest Format
\`\`\`json
{
  "id": "plugin-id",
  "name": "Plugin Name",
  "version": "0.1.0",
  "description": "What the plugin does",
  "author": "Author Name",
  "main": "index.js",
  "permissions": {
    "read": ["transactions", "accounts"],
    "schemaName": "plugin_plugin_id"
  }
}
\`\`\`

## Plugin Code Structure
\`\`\`typescript
import type { Plugin, PluginContext, PluginSDK } from "@treeline-money/plugin-sdk";
import { mount, unmount } from "svelte";

// Define your Svelte component inline or import it
const MyView = ... // Svelte 5 component

export const plugin: Plugin = {
  manifest: {
    id: "plugin-id",
    name: "Plugin Name",
    version: "0.1.0",
    description: "Description",
    author: "Author",
    permissions: {
      read: ["transactions", "accounts"],
      schemaName: "plugin_plugin_id",
    },
  },

  migrations: [
    // Optional: database migrations for plugin-specific tables
    {
      version: 1,
      name: "create_my_table",
      up: \`CREATE TABLE IF NOT EXISTS plugin_plugin_id.my_table (...)\`,
    },
  ],

  activate(context: PluginContext) {
    context.registerView({
      id: "my-view",
      name: "My View",
      icon: "chart-bar", // Lucide icon name or emoji
      mount: (target: HTMLElement, props: { sdk: PluginSDK }) => {
        const instance = mount(MyView, { target, props });
        return () => unmount(instance);
      },
    });

    context.registerSidebarItem({
      sectionId: "plugins",
      id: "my-sidebar-item",
      label: "My Plugin",
      icon: "chart-bar",
      viewId: "my-view",
    });
  },
};
\`\`\`

## PluginSDK API (available in view components via props.sdk)

- \`sdk.query<T>(sql, params?)\` - Execute SQL query, returns Promise<T[]>
- \`sdk.execute(sql, params?)\` - Execute SQL (INSERT/UPDATE/DELETE), returns Promise<{rowsAffected}>
- \`sdk.getSchemaName()\` - Get plugin's schema name
- \`sdk.toast.success/error/info/warning(message, description?)\` - Show notifications
- \`sdk.openView(viewId, props?)\` - Navigate to another view
- \`sdk.onDataRefresh(callback)\` - Subscribe to data refresh events, returns unsubscribe function
- \`sdk.emitDataRefresh()\` - Trigger data refresh
- \`sdk.updateBadge(count)\` - Update sidebar badge count
- \`sdk.theme.current()\` - Get current theme ("light" | "dark")
- \`sdk.theme.subscribe(callback)\` - Subscribe to theme changes
- \`sdk.currency.format(amount)\` - Format currency amount
- \`sdk.settings.get<T>()\` / \`sdk.settings.set<T>(settings)\` - Plugin settings
- \`sdk.state.read<T>()\` / \`sdk.state.write<T>(state)\` - Plugin state

## Svelte 5 Patterns

Use Svelte 5 runes:
- \`let count = $state(0)\` - Reactive state
- \`let doubled = $derived(count * 2)\` - Derived values
- \`$effect(() => { ... })\` - Side effects
- \`let { sdk }: Props = $props()\` - Component props

## Important Guidelines

1. Always generate complete, working code
2. Use proper TypeScript types
3. Include error handling
4. Use the SDK for database access (never raw SQL outside sdk.query/execute)
5. Follow Svelte 5 patterns (runes, not stores)
6. Make the UI match Treeline's style using CSS variables:
   - \`var(--bg-primary)\`, \`var(--bg-secondary)\`, \`var(--bg-tertiary)\`
   - \`var(--text-primary)\`, \`var(--text-secondary)\`, \`var(--text-muted)\`
   - \`var(--accent-primary)\`, \`var(--accent-danger)\`, \`var(--accent-warning)\`
   - \`var(--border-primary)\`
   - \`var(--spacing-xs)\`, \`var(--spacing-sm)\`, \`var(--spacing-md)\`, \`var(--spacing-lg)\`
   - \`var(--radius-sm)\`, \`var(--radius-md)\`, \`var(--radius-lg)\`

## Response Format

When generating a plugin, include the code in a code block with the language marker:

\`\`\`plugin-manifest
{ ... manifest json ... }
\`\`\`

\`\`\`plugin-code
// Full plugin TypeScript code
\`\`\`

Always explain what the plugin does and how to use it.`;
  }

  function parseAIResponse(response: string): { message: string; plugin: GeneratedPlugin | null } {
    const manifestMatch = response.match(/```plugin-manifest\n([\s\S]*?)```/);
    const codeMatch = response.match(/```plugin-code\n([\s\S]*?)```/);

    if (manifestMatch && codeMatch) {
      try {
        const manifest = JSON.parse(manifestMatch[1]);
        const sourceCode = codeMatch[1];

        return {
          message: response,
          plugin: {
            pluginId: manifest.id,
            name: manifest.name,
            description: manifest.description || "",
            version: manifest.version || "0.1.0",
            sourceCode,
            manifestJson: manifestMatch[1],
            conversationId: currentConversation?.id || null,
            createdAt: new Date(),
            updatedAt: new Date(),
          },
        };
      } catch (e) {
        console.error("Failed to parse plugin code:", e);
      }
    }

    return { message: response, plugin: null };
  }

  async function saveGeneratedPlugin(conversationId: number, plugin: GeneratedPlugin) {
    const now = new Date().toISOString();

    // Check if plugin already exists
    const existing = await query(`
      SELECT plugin_id FROM plugin_ai_builder.generated_plugins WHERE plugin_id = ?
    `, [plugin.pluginId]);

    if (existing.rows.length > 0) {
      // Update existing
      await execute(`
        UPDATE plugin_ai_builder.generated_plugins
        SET name = ?, description = ?, version = ?, source_code = ?, manifest_json = ?, updated_at = ?::TIMESTAMP
        WHERE plugin_id = ?
      `, [plugin.name, plugin.description, plugin.version, plugin.sourceCode, plugin.manifestJson, now, plugin.pluginId]);
    } else {
      // Insert new
      await execute(`
        INSERT INTO plugin_ai_builder.generated_plugins
        (plugin_id, name, description, version, source_code, manifest_json, conversation_id, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?::TIMESTAMP, ?::TIMESTAMP)
      `, [plugin.pluginId, plugin.name, plugin.description, plugin.version, plugin.sourceCode, plugin.manifestJson, conversationId, now, now]);
    }

    // Update conversation with plugin_id
    await execute(`
      UPDATE plugin_ai_builder.conversations SET plugin_id = ? WHERE conversation_id = ?
    `, [plugin.pluginId, conversationId]);
  }

  async function buildAndInstallPlugin() {
    if (!currentPlugin) return;

    isBuilding = true;
    try {
      // Write plugin files and build
      const result = await invoke<{ success: boolean; error?: string }>("build_ai_plugin", {
        pluginId: currentPlugin.pluginId,
        manifestJson: currentPlugin.manifestJson,
        sourceCode: currentPlugin.sourceCode,
      });

      if (!result.success) {
        throw new Error(result.error || "Build failed");
      }

      // Hot reload the plugin
      const reloadResult = await loadNewPlugin(currentPlugin.pluginId);

      if (!reloadResult.success) {
        throw new Error(reloadResult.error || "Failed to load plugin");
      }

      toast.success("Plugin installed!", `${currentPlugin.name} is now active`);
    } catch (e) {
      console.error("Failed to build plugin:", e);
      toast.error("Build failed", e instanceof Error ? e.message : String(e));
    } finally {
      isBuilding = false;
    }
  }

  async function rebuildPlugin() {
    if (!currentPlugin) return;

    isBuilding = true;
    try {
      const result = await invoke<{ success: boolean; error?: string }>("build_ai_plugin", {
        pluginId: currentPlugin.pluginId,
        manifestJson: currentPlugin.manifestJson,
        sourceCode: currentPlugin.sourceCode,
      });

      if (!result.success) {
        throw new Error(result.error || "Build failed");
      }

      const reloadResult = await hotReloadPlugin(currentPlugin.pluginId);

      if (!reloadResult.success) {
        throw new Error(reloadResult.error || "Failed to reload plugin");
      }

      toast.success("Plugin reloaded!", "Changes are now live");
    } catch (e) {
      console.error("Failed to rebuild plugin:", e);
      toast.error("Rebuild failed", e instanceof Error ? e.message : String(e));
    } finally {
      isBuilding = false;
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }

  let messagesContainer: HTMLDivElement;

  $effect(() => {
    // Scroll to bottom when messages change
    if (messagesContainer && messages.length > 0) {
      messagesContainer.scrollTop = messagesContainer.scrollHeight;
    }
  });

  onMount(async () => {
    await checkApiKey();
    await loadConversations();
    await loadSchemaInfo();
  });
</script>

<div class="ai-builder">
  <!-- Sidebar with conversations -->
  <aside class="conversations-panel" class:collapsed={!showConversations}>
    <div class="panel-header">
      <h3>Conversations</h3>
      <button class="icon-button" onclick={startNewConversation} title="New conversation">
        <Icon name="plus" size={16} />
      </button>
    </div>
    <div class="conversations-list">
      {#each conversations as conv}
        <button
          class="conversation-item"
          class:active={currentConversation?.id === conv.id}
          onclick={() => selectConversation(conv)}
        >
          <span class="conv-title">{conv.title}</span>
          {#if conv.pluginId}
            <span class="conv-badge">📦</span>
          {/if}
        </button>
      {/each}
      {#if conversations.length === 0}
        <div class="empty-state">
          <p>No conversations yet</p>
          <p class="hint">Describe the plugin you want to build</p>
        </div>
      {/if}
    </div>
  </aside>

  <!-- Main chat area -->
  <main class="chat-panel">
    <div class="chat-header">
      <button class="toggle-sidebar" onclick={() => showConversations = !showConversations}>
        <Icon name={showConversations ? "panel-left-close" : "panel-left-open"} size={18} />
      </button>
      <h2>
        {#if currentConversation}
          {currentConversation.title}
        {:else}
          New Plugin
        {/if}
      </h2>
      {#if currentPlugin}
        <div class="header-actions">
          <button class="toggle-code" onclick={() => showCodePanel = !showCodePanel}>
            <Icon name="code" size={16} />
            {showCodePanel ? "Hide Code" : "Show Code"}
          </button>
        </div>
      {/if}
    </div>

    <div class="chat-content">
      <div class="messages-area" bind:this={messagesContainer}>
        {#if messages.length === 0}
          <div class="welcome-message">
            <div class="welcome-icon">🤖</div>
            <h3>Build a Plugin</h3>
            <p>Describe what you want your plugin to do, and I'll generate the code for you.</p>
            <div class="suggestions">
              <button onclick={() => inputValue = "Create a plugin that shows my spending by category as a pie chart"}>
                📊 Spending by category chart
              </button>
              <button onclick={() => inputValue = "Build a plugin that tracks my savings goals and progress"}>
                🎯 Savings goals tracker
              </button>
              <button onclick={() => inputValue = "Make a plugin that shows my net worth over time"}>
                📈 Net worth history
              </button>
              <button onclick={() => inputValue = "Create a budget tracker that compares spending to limits"}>
                💰 Budget tracker
              </button>
            </div>
          </div>
        {:else}
          {#each messages as message}
            <div class="message" class:user={message.role === "user"} class:assistant={message.role === "assistant"}>
              <div class="message-avatar">
                {#if message.role === "user"}
                  <Icon name="user" size={16} />
                {:else}
                  🤖
                {/if}
              </div>
              <div class="message-content">
                {#if message.role === "assistant"}
                  {@html formatMessage(message.content)}
                {:else}
                  <p>{message.content}</p>
                {/if}
              </div>
            </div>
          {/each}
          {#if isLoading}
            <div class="message assistant loading">
              <div class="message-avatar">🤖</div>
              <div class="message-content">
                <div class="typing-indicator">
                  <span></span><span></span><span></span>
                </div>
              </div>
            </div>
          {/if}
        {/if}
      </div>

      <div class="input-area">
        <textarea
          bind:value={inputValue}
          onkeydown={handleKeyDown}
          placeholder="Describe the plugin you want to build..."
          rows="3"
          disabled={isLoading}
        ></textarea>
        <button class="send-button" onclick={sendMessage} disabled={isLoading || !inputValue.trim()}>
          {#if isLoading}
            <div class="spinner"></div>
          {:else}
            <Icon name="send" size={18} />
          {/if}
        </button>
      </div>
    </div>
  </main>

  <!-- Code panel -->
  {#if showCodePanel && currentPlugin}
    <aside class="code-panel">
      <div class="panel-header">
        <h3>{currentPlugin.name}</h3>
        <div class="code-actions">
          <button class="action-button" onclick={rebuildPlugin} disabled={isBuilding}>
            <Icon name="refresh-cw" size={14} />
            {isBuilding ? "Building..." : "Rebuild"}
          </button>
          <button class="action-button primary" onclick={buildAndInstallPlugin} disabled={isBuilding}>
            <Icon name="play" size={14} />
            Install
          </button>
        </div>
      </div>
      <div class="code-content">
        <div class="code-section">
          <h4>manifest.json</h4>
          <pre><code>{currentPlugin.manifestJson}</code></pre>
        </div>
        <div class="code-section">
          <h4>index.ts</h4>
          <pre><code>{currentPlugin.sourceCode}</code></pre>
        </div>
      </div>
    </aside>
  {/if}
</div>

<!-- API Key Modal -->
{#if showApiKeyModal}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="modal-overlay" role="dialog" aria-modal="true" tabindex="-1" onclick={() => showApiKeyModal = false} onkeydown={(e) => e.key === "Escape" && (showApiKeyModal = false)}>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="modal" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h3>Configure API Key</h3>
      </div>
      <div class="modal-body">
        <p>Enter your Anthropic API key to enable AI plugin generation.</p>
        <input
          type="password"
          bind:value={apiKeyInput}
          placeholder="sk-ant-..."
          onkeydown={(e) => e.key === "Enter" && saveApiKey()}
        />
        <p class="hint">Your API key is stored locally and never shared.</p>
      </div>
      <div class="modal-footer">
        <button class="btn secondary" onclick={() => showApiKeyModal = false}>Cancel</button>
        <button class="btn primary" onclick={saveApiKey} disabled={!apiKeyInput.trim()}>Save</button>
      </div>
    </div>
  </div>
{/if}

<script module lang="ts">
  function formatMessage(content: string): string {
    // Basic markdown-like formatting
    let html = content
      // Escape HTML
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      // Code blocks (but hide plugin-manifest and plugin-code blocks in UI)
      .replace(/```plugin-manifest\n[\s\S]*?```/g, '<div class="code-generated">✓ Manifest generated</div>')
      .replace(/```plugin-code\n[\s\S]*?```/g, '<div class="code-generated">✓ Plugin code generated</div>')
      .replace(/```(\w*)\n([\s\S]*?)```/g, '<pre><code class="language-$1">$2</code></pre>')
      // Inline code
      .replace(/`([^`]+)`/g, '<code>$1</code>')
      // Bold
      .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
      // Line breaks
      .replace(/\n/g, '<br>');

    return html;
  }
</script>

<style>
  .ai-builder {
    height: 100%;
    display: flex;
    background: var(--bg-primary);
  }

  /* Conversations sidebar */
  .conversations-panel {
    width: 260px;
    border-right: 1px solid var(--border-primary);
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    transition: width 0.2s, opacity 0.2s;
  }

  .conversations-panel.collapsed {
    width: 0;
    opacity: 0;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-md);
    border-bottom: 1px solid var(--border-primary);
  }

  .panel-header h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .icon-button {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: var(--spacing-xs);
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .icon-button:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .conversations-list {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-sm);
  }

  .conversation-item {
    width: 100%;
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-sm) var(--spacing-md);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    text-align: left;
    color: var(--text-primary);
    font-size: 13px;
    transition: background 0.15s;
  }

  .conversation-item:hover {
    background: var(--bg-tertiary);
  }

  .conversation-item.active {
    background: var(--bg-tertiary);
    color: var(--accent-primary);
  }

  .conv-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .conv-badge {
    font-size: 12px;
    margin-left: var(--spacing-xs);
  }

  .empty-state {
    padding: var(--spacing-lg);
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }

  .empty-state .hint {
    font-size: 12px;
    margin-top: var(--spacing-xs);
  }

  /* Main chat area */
  .chat-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .chat-header {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    padding: var(--spacing-md) var(--spacing-lg);
    border-bottom: 1px solid var(--border-primary);
    background: var(--bg-secondary);
  }

  .toggle-sidebar {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: var(--spacing-xs);
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
  }

  .toggle-sidebar:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .chat-header h2 {
    flex: 1;
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .header-actions {
    display: flex;
    gap: var(--spacing-sm);
  }

  .toggle-code {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    padding: var(--spacing-xs) var(--spacing-sm);
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .toggle-code:hover {
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }

  .chat-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .messages-area {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-lg);
  }

  /* Welcome message */
  .welcome-message {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    text-align: center;
    color: var(--text-secondary);
  }

  .welcome-icon {
    font-size: 48px;
    margin-bottom: var(--spacing-md);
  }

  .welcome-message h3 {
    margin: 0 0 var(--spacing-sm) 0;
    font-size: 20px;
    color: var(--text-primary);
  }

  .welcome-message p {
    margin: 0 0 var(--spacing-lg) 0;
    max-width: 400px;
  }

  .suggestions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--spacing-sm);
    justify-content: center;
    max-width: 500px;
  }

  .suggestions button {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    padding: var(--spacing-sm) var(--spacing-md);
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
    transition: all 0.15s;
  }

  .suggestions button:hover {
    border-color: var(--accent-primary);
    background: var(--bg-tertiary);
  }

  /* Messages */
  .message {
    display: flex;
    gap: var(--spacing-md);
    margin-bottom: var(--spacing-lg);
  }

  .message-avatar {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    font-size: 14px;
    color: var(--text-muted);
  }

  .message.user .message-avatar {
    background: var(--accent-primary);
    color: white;
  }

  .message-content {
    flex: 1;
    min-width: 0;
  }

  .message-content p {
    margin: 0;
    line-height: 1.6;
  }

  .message-content :global(pre) {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    padding: var(--spacing-md);
    overflow-x: auto;
    margin: var(--spacing-sm) 0;
  }

  .message-content :global(code) {
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .message-content :global(.code-generated) {
    background: var(--bg-tertiary);
    border: 1px solid var(--accent-primary);
    border-radius: var(--radius-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    font-size: 12px;
    color: var(--accent-primary);
    display: inline-block;
    margin: var(--spacing-xs) 0;
  }

  /* Typing indicator */
  .typing-indicator {
    display: flex;
    gap: 4px;
    padding: var(--spacing-sm);
  }

  .typing-indicator span {
    width: 8px;
    height: 8px;
    background: var(--text-muted);
    border-radius: 50%;
    animation: bounce 1.4s infinite ease-in-out;
  }

  .typing-indicator span:nth-child(1) { animation-delay: 0s; }
  .typing-indicator span:nth-child(2) { animation-delay: 0.2s; }
  .typing-indicator span:nth-child(3) { animation-delay: 0.4s; }

  @keyframes bounce {
    0%, 80%, 100% { transform: translateY(0); }
    40% { transform: translateY(-6px); }
  }

  /* Input area */
  .input-area {
    display: flex;
    gap: var(--spacing-sm);
    padding: var(--spacing-md) var(--spacing-lg);
    border-top: 1px solid var(--border-primary);
    background: var(--bg-secondary);
  }

  .input-area textarea {
    flex: 1;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    padding: var(--spacing-md);
    font-size: 14px;
    color: var(--text-primary);
    resize: none;
    font-family: inherit;
  }

  .input-area textarea:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .input-area textarea::placeholder {
    color: var(--text-muted);
  }

  .send-button {
    width: 44px;
    height: 44px;
    background: var(--accent-primary);
    border: none;
    border-radius: var(--radius-md);
    color: white;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    align-self: flex-end;
    transition: opacity 0.15s;
  }

  .send-button:hover:not(:disabled) {
    opacity: 0.9;
  }

  .send-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .spinner {
    width: 18px;
    height: 18px;
    border: 2px solid rgba(255,255,255,0.3);
    border-top-color: white;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Code panel */
  .code-panel {
    width: 400px;
    border-left: 1px solid var(--border-primary);
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
  }

  .code-panel .panel-header {
    flex-direction: column;
    align-items: flex-start;
    gap: var(--spacing-sm);
  }

  .code-actions {
    display: flex;
    gap: var(--spacing-sm);
    width: 100%;
  }

  .action-button {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--spacing-xs);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    padding: var(--spacing-sm);
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .action-button:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }

  .action-button.primary {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: white;
  }

  .action-button.primary:hover:not(:disabled) {
    opacity: 0.9;
  }

  .action-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .code-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-md);
  }

  .code-section {
    margin-bottom: var(--spacing-lg);
  }

  .code-section h4 {
    margin: 0 0 var(--spacing-sm) 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .code-section pre {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    padding: var(--spacing-md);
    overflow-x: auto;
    margin: 0;
  }

  .code-section code {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-primary);
    white-space: pre-wrap;
    word-break: break-word;
  }

  /* Modal */
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-lg);
    width: 400px;
    max-width: 90vw;
  }

  .modal-header {
    padding: var(--spacing-md) var(--spacing-lg);
    border-bottom: 1px solid var(--border-primary);
  }

  .modal-header h3 {
    margin: 0;
    font-size: 16px;
  }

  .modal-body {
    padding: var(--spacing-lg);
  }

  .modal-body p {
    margin: 0 0 var(--spacing-md) 0;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .modal-body input {
    width: 100%;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    padding: var(--spacing-md);
    font-size: 14px;
    color: var(--text-primary);
    font-family: var(--font-mono);
  }

  .modal-body input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .modal-body .hint {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: var(--spacing-sm);
  }

  .modal-footer {
    padding: var(--spacing-md) var(--spacing-lg);
    border-top: 1px solid var(--border-primary);
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-sm);
  }

  .btn {
    padding: var(--spacing-sm) var(--spacing-md);
    border-radius: var(--radius-sm);
    font-size: 13px;
    cursor: pointer;
  }

  .btn.secondary {
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    color: var(--text-primary);
  }

  .btn.primary {
    background: var(--accent-primary);
    border: none;
    color: white;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
