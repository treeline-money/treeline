<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { PluginSDK } from "@treeline-money/plugin-sdk";

  // Props passed by Treeline when mounting the view
  interface Props {
    sdk: PluginSDK;
  }
  let { sdk }: Props = $props();

  // State
  let accountCount = $state(0);
  let transactionCount = $state(0);
  let isLoading = $state(true);
  let theme = $state<"light" | "dark">("light");

  // Unsubscribe function for data refresh
  let unsubscribe: (() => void) | null = null;

  onMount(async () => {
    // Get initial theme
    theme = sdk.theme.current();

    // Subscribe to theme changes
    sdk.theme.subscribe((newTheme) => {
      theme = newTheme as "light" | "dark";
    });

    // Subscribe to data refresh events
    unsubscribe = sdk.onDataRefresh(() => {
      loadData();
    });

    // Load initial data
    await loadData();
  });

  onDestroy(() => {
    if (unsubscribe) {
      unsubscribe();
    }
  });

  async function loadData() {
    isLoading = true;
    try {
      // Use sdk.query() to fetch data
      const accounts = await sdk.query<{ count: number }>(
        "SELECT COUNT(*) as count FROM sys_accounts"
      );
      accountCount = accounts[0]?.count ?? 0;

      const transactions = await sdk.query<{ count: number }>(
        "SELECT COUNT(*) as count FROM transactions"
      );
      transactionCount = transactions[0]?.count ?? 0;
    } catch (e) {
      sdk.toast.error("Failed to load data", e instanceof Error ? e.message : String(e));
    } finally {
      isLoading = false;
    }
  }

  function showToastDemo() {
    sdk.toast.success("Hello from plugin!", "Toast notifications work great");
  }

  function openQueryView() {
    // Navigate to another view
    sdk.openView("query", {
      initialQuery: "SELECT * FROM transactions LIMIT 10"
    });
  }
</script>

<div class="container" class:dark={theme === "dark"}>
  <h1>Hello World Plugin</h1>

  <p class="description">
    This plugin demonstrates the Treeline Plugin SDK.
    It's loaded from <code>~/.treeline/plugins/</code>
  </p>

  <div class="stats-grid">
    <div class="stat-card">
      <span class="stat-label">Accounts</span>
      <span class="stat-value">{isLoading ? "..." : accountCount}</span>
    </div>
    <div class="stat-card">
      <span class="stat-label">Transactions</span>
      <span class="stat-value">{isLoading ? "..." : transactionCount}</span>
    </div>
    <div class="stat-card">
      <span class="stat-label">Theme</span>
      <span class="stat-value">{theme}</span>
    </div>
    <div class="stat-card">
      <span class="stat-label">Mod Key</span>
      <span class="stat-value">{sdk.modKey}</span>
    </div>
  </div>

  <div class="actions">
    <button class="btn primary" onclick={showToastDemo}>
      Show Toast
    </button>
    <button class="btn secondary" onclick={openQueryView}>
      Open Query View
    </button>
    <button class="btn secondary" onclick={loadData}>
      Refresh Data
    </button>
  </div>

  <div class="info-section">
    <h2>SDK Features</h2>
    <ul>
      <li><code>sdk.query(sql)</code> - Read data from the database</li>
      <li><code>sdk.execute(sql)</code> - Write to your plugin's tables</li>
      <li><code>sdk.toast.success/error/info()</code> - Show notifications</li>
      <li><code>sdk.openView(viewId, props)</code> - Navigate to views</li>
      <li><code>sdk.onDataRefresh(callback)</code> - React to data changes</li>
      <li><code>sdk.theme.current()</code> - Get current theme</li>
      <li><code>sdk.settings.get/set()</code> - Persist plugin settings</li>
    </ul>
  </div>
</div>

<style>
  .container {
    padding: 24px;
    font-family: system-ui, -apple-system, sans-serif;
    color: #1a1a1a;
    background: #ffffff;
    min-height: 100%;
  }

  .container.dark {
    color: #e5e5e5;
    background: #1a1a1a;
  }

  h1 {
    font-size: 24px;
    font-weight: 600;
    margin: 0 0 8px;
  }

  h2 {
    font-size: 16px;
    font-weight: 600;
    margin: 0 0 12px;
  }

  .description {
    color: #666;
    margin: 0 0 24px;
  }

  .dark .description {
    color: #999;
  }

  code {
    font-family: ui-monospace, monospace;
    font-size: 13px;
    background: #f3f4f6;
    padding: 2px 6px;
    border-radius: 4px;
  }

  .dark code {
    background: #2a2a2a;
  }

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 16px;
    margin-bottom: 24px;
  }

  .stat-card {
    background: #f8f9fa;
    border: 1px solid #e5e7eb;
    border-radius: 8px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .dark .stat-card {
    background: #2a2a2a;
    border-color: #3a3a3a;
  }

  .stat-label {
    font-size: 12px;
    color: #666;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .dark .stat-label {
    color: #888;
  }

  .stat-value {
    font-size: 24px;
    font-weight: 600;
  }

  .actions {
    display: flex;
    gap: 12px;
    margin-bottom: 24px;
  }

  .btn {
    padding: 8px 16px;
    border-radius: 6px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    border: none;
    transition: all 0.15s;
  }

  .btn.primary {
    background: #3b82f6;
    color: white;
  }

  .btn.primary:hover {
    background: #2563eb;
  }

  .btn.secondary {
    background: #e5e7eb;
    color: #374151;
  }

  .dark .btn.secondary {
    background: #3a3a3a;
    color: #e5e5e5;
  }

  .btn.secondary:hover {
    background: #d1d5db;
  }

  .dark .btn.secondary:hover {
    background: #4a4a4a;
  }

  .info-section {
    background: #f0f9ff;
    border: 1px solid #bae6fd;
    border-radius: 8px;
    padding: 16px;
  }

  .dark .info-section {
    background: #1e3a5f;
    border-color: #2563eb;
  }

  ul {
    margin: 0;
    padding-left: 20px;
  }

  li {
    margin-bottom: 8px;
    font-size: 14px;
  }
</style>
