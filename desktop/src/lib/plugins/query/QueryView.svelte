<script lang="ts">
  import { executeQueryWithParams, type QueryResult, modKey, toast } from "../../sdk";
  import { readPluginState, writePluginState } from "../../sdk/settings";
  import type { PluginContext } from "../../sdk/api";
  import Icon from "../../shared/Icon.svelte";
  import { onMount, onDestroy } from "svelte";
  import { EditorView, keymap } from "@codemirror/view";
  import { EditorState, Prec, Compartment } from "@codemirror/state";
  import { basicSetup } from "codemirror";
  import { sql } from "@codemirror/lang-sql";
  import { oneDark } from "@codemirror/theme-one-dark";
  import { themeManager } from "../../sdk";

  // Plugin context for permission validation - query plugin has read/write access to all tables
  const PLUGIN_ID = "query";
  const PLUGIN_SCHEMA = "plugin_query";
  const pluginContext: PluginContext = {
    plugin_id: PLUGIN_ID,
    plugin_schema: PLUGIN_SCHEMA,
    allowed_reads: ["*"],
    allowed_writes: ["*"],
  };

  // State for write mode feature
  interface QueryPluginState {
    writeModeEnabled?: boolean;
    writeModeWarningDismissed?: boolean;
  }
  let writeModeEnabled = $state(false);
  let writeModeWarningDismissed = $state(false);
  let showWriteWarningModal = $state(false);
  let isCreatingBackup = $state(false);
  let dontShowAgainChecked = $state(false);

  // Helper to execute read queries with plugin context
  async function queryRead(sql: string, params: (string | number | boolean | null)[] = []) {
    return executeQueryWithParams(sql, params, { readonly: true, pluginContext });
  }

  // Helper to execute write queries with plugin context (for internal use like history)
  async function queryWrite(sql: string, params: (string | number | boolean | null)[] = []) {
    return executeQueryWithParams(sql, params, { readonly: false, pluginContext });
  }

  // Detect if a query is a write query
  function isWriteQuery(sql: string): boolean {
    const trimmed = sql.trim().toUpperCase();
    const firstWord = trimmed.split(/\s+/)[0];
    return ["INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE"].includes(firstWord);
  }

  // Save write mode preferences
  async function saveWriteModeState() {
    await writePluginState<QueryPluginState>(PLUGIN_ID, {
      writeModeEnabled,
      writeModeWarningDismissed,
    });
  }

  // Props passed from openView
  interface Props {
    initialQuery?: string;
  }
  let { initialQuery = undefined }: Props = $props();

  const MAX_HISTORY = 50;

  interface HistoryEntry {
    id?: number;
    query: string;
    timestamp: number;
    success: boolean;
  }

  let query = $state(initialQuery ?? "SELECT * FROM transactions LIMIT 10");
  let result = $state<QueryResult | null>(null);
  let isLoading = $state(false);
  let error = $state<string | null>(null);
  let history = $state<HistoryEntry[]>(loadHistory());
  let showHistory = $state(false);
  let executionTime = $state<number | null>(null);

  // Schema state
  interface TableSchema {
    name: string;
    columns: { name: string; type: string }[];
  }
  let schema = $state<TableSchema[]>([]);
  let showSchema = $state(false);
  let schemaLoading = $state(false);
  let selectedTableSchema = $state<TableSchema | null>(null);

  async function loadSchemaData() {
    if (schema.length > 0) return;

    schemaLoading = true;
    try {
      // Get list of tables
      const tablesResult = await queryRead("SELECT table_name FROM information_schema.tables WHERE table_schema = 'main'");
      const tableNames = tablesResult.rows.map(row => row[0] as string);

      // Get columns for each table
      const tables: TableSchema[] = [];
      for (const tableName of tableNames) {
        const columnsResult = await queryRead(
          `SELECT column_name, data_type FROM information_schema.columns WHERE table_name = ? ORDER BY ordinal_position`,
          [tableName]
        );
        tables.push({
          name: tableName,
          columns: columnsResult.rows.map(row => ({
            name: row[0] as string,
            type: row[1] as string,
          })),
        });
      }
      schema = tables;
    } catch (e) {
      console.error("Failed to load schema:", e);
    } finally {
      schemaLoading = false;
    }
  }

  async function loadSchema() {
    if (schema.length > 0) {
      showSchema = !showSchema;
      return;
    }
    await loadSchemaData();
    showSchema = true;
  }

  function selectTable(table: TableSchema) {
    selectedTableSchema = table;
  }

  function clearSelectedTable() {
    selectedTableSchema = null;
  }

  // Sorting state
  let sortColumn = $state<number | null>(null);
  let sortDirection = $state<"asc" | "desc">("asc");

  // Derived sorted rows
  let sortedRows = $derived.by(() => {
    if (!result || sortColumn === null) return result?.rows ?? [];

    const col = sortColumn; // Narrow type for closure
    const rows = [...result.rows];
    rows.sort((a, b) => {
      const aVal = a[col];
      const bVal = b[col];

      // Handle nulls
      if (aVal === null && bVal === null) return 0;
      if (aVal === null) return sortDirection === "asc" ? 1 : -1;
      if (bVal === null) return sortDirection === "asc" ? -1 : 1;

      // Compare values
      let cmp = 0;
      if (typeof aVal === "number" && typeof bVal === "number") {
        cmp = aVal - bVal;
      } else if (Array.isArray(aVal) && Array.isArray(bVal)) {
        cmp = aVal.join(",").localeCompare(bVal.join(","));
      } else {
        cmp = String(aVal).localeCompare(String(bVal));
      }

      return sortDirection === "asc" ? cmp : -cmp;
    });

    return rows;
  });

  function toggleSort(colIndex: number) {
    if (sortColumn === colIndex) {
      // Toggle direction or clear
      if (sortDirection === "asc") {
        sortDirection = "desc";
      } else {
        sortColumn = null;
        sortDirection = "asc";
      }
    } else {
      sortColumn = colIndex;
      sortDirection = "asc";
    }
  }

  // Copy to clipboard
  let copiedCell = $state<{ row: number; col: number } | null>(null);

  async function copyCell(value: unknown, rowIndex: number, colIndex: number) {
    let text: string;
    if (value === null) {
      text = "";
    } else if (Array.isArray(value)) {
      text = value.join(", ");
    } else {
      text = String(value);
    }

    try {
      await navigator.clipboard.writeText(text);
      copiedCell = { row: rowIndex, col: colIndex };
      setTimeout(() => {
        copiedCell = null;
      }, 1500);
    } catch (e) {
      console.error("Failed to copy:", e);
    }
  }

  // Load history from DuckDB (async, but we initialize synchronously with empty array)
  function loadHistory(): HistoryEntry[] {
    // Return empty initially, will be loaded async in onMount
    return [];
  }

  async function loadHistoryAsync(): Promise<HistoryEntry[]> {
    try {
      const result = await queryRead(`
        SELECT history_id, query, success, executed_at
        FROM plugin_query.history
        ORDER BY executed_at DESC
        LIMIT ${MAX_HISTORY}
      `);

      return result.rows.map((row) => ({
        id: row[0] as number,
        query: row[1] as string,
        success: row[2] as boolean,
        timestamp: new Date(row[3] as string).getTime(),
      }));
    } catch (e) {
      // Table might not exist yet
      console.warn("Failed to load query history:", e);
      return [];
    }
  }

  async function addToHistory(queryText: string, success: boolean) {
    // Don't add duplicates of the most recent query
    if (history.length > 0 && history[0].query === queryText) {
      return;
    }

    try {
      // Insert into DuckDB using the sequence for ID (matches CLI migration 009)
      // Use JS-computed timestamp to avoid ICU extension dependency
      const now = new Date().toISOString();
      await queryWrite(
        `INSERT INTO plugin_query.history (history_id, query, success, executed_at)
         VALUES (nextval('plugin_query.seq_history_id'), ?, ?, ?::TIMESTAMP)`,
        [queryText, success, now]
      );

      // Reload history from DB to get the new entry with ID
      history = await loadHistoryAsync();
    } catch (e) {
      // Fallback to in-memory only if DB fails
      console.warn("Failed to save query history:", e);
      const entry: HistoryEntry = {
        query: queryText,
        timestamp: Date.now(),
        success,
      };
      history = [entry, ...history.slice(0, MAX_HISTORY - 1)];
    }
  }

  function loadFromHistory(entry: HistoryEntry) {
    // Directly update editor
    if (editorView) {
      editorView.dispatch({
        changes: { from: 0, to: editorView.state.doc.length, insert: entry.query }
      });
    }
    showHistory = false;
  }

  function handleClickOutside(event: MouseEvent) {
    const target = event.target as HTMLElement;
    if (!target.closest(".history-container")) {
      showHistory = false;
    }
  }

  $effect(() => {
    if (showHistory) {
      // Use setTimeout to avoid the click that opened the dropdown from immediately closing it
      const timeout = setTimeout(() => {
        document.addEventListener("click", handleClickOutside);
      }, 0);
      return () => {
        clearTimeout(timeout);
        document.removeEventListener("click", handleClickOutside);
      };
    }
  });

  async function clearHistory() {
    try {
      await queryWrite("DELETE FROM plugin_query.history");
      history = [];
    } catch (e) {
      console.warn("Failed to clear query history:", e);
      history = [];
    }
  }

  function formatTimestamp(ts: number): string {
    const date = new Date(ts);
    const now = new Date();
    const diff = now.getTime() - ts;

    if (diff < 60000) return "just now";
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
    if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
    if (date.toDateString() === now.toDateString()) return "today";

    return date.toLocaleDateString();
  }

  async function runQuery() {
    const currentQuery = editorView?.state.doc.toString() ?? "";
    if (!currentQuery.trim()) {
      error = "Please enter a query";
      return;
    }

    // Check if this is a write query and write mode is not enabled
    const isWrite = isWriteQuery(currentQuery);
    if (isWrite && !writeModeEnabled) {
      error = "Write mode is not enabled. Check \"Enable write queries\" to run INSERT, UPDATE, DELETE, or other write operations.";
      return;
    }

    isLoading = true;
    error = null;
    result = null;
    executionTime = null;
    sortColumn = null;
    sortDirection = "asc";

    const startTime = performance.now();

    try {
      // Execute with plugin context for permission validation
      result = await executeQueryWithParams(currentQuery, [], {
        readonly: !isWrite,
        pluginContext
      });
      executionTime = performance.now() - startTime;
      await addToHistory(currentQuery, true);
    } catch (e) {
      executionTime = performance.now() - startTime;
      error = e instanceof Error ? e.message : "Failed to execute query";
      await addToHistory(currentQuery, false);
      console.error("Query error:", e);
    } finally {
      isLoading = false;
    }
  }

  function clearQuery() {
    result = null;
    error = null;
    // Clear CodeMirror content
    if (editorView) {
      editorView.dispatch({
        changes: { from: 0, to: editorView.state.doc.length, insert: "" }
      });
    }
  }

  let viewEl: HTMLDivElement;
  let editorContainer: HTMLDivElement;
  let editorView: EditorView | null = null;

  // Compartments for dynamic reconfiguration
  const editorThemeCompartment = new Compartment();
  const sqlCompartment = new Compartment();

  function getEditorThemeExtension() {
    const isDark = themeManager.current === "dark";
    return isDark ? oneDark : [];
  }

  function getSqlExtension(schemaData: TableSchema[]) {
    if (schemaData.length === 0) {
      return sql();
    }
    // Convert schema to CodeMirror's format: { tableName: ["col1", "col2"] }
    const cmSchema: Record<string, string[]> = {};
    for (const table of schemaData) {
      cmSchema[table.name] = table.columns.map(c => c.name);
    }
    return sql({ schema: cmSchema });
  }

  // Resizable editor height
  let editorHeight = $state(180);
  let isResizing = $state(false);
  let startY = 0;
  let startHeight = 0;

  function startResize(e: MouseEvent) {
    isResizing = true;
    startY = e.clientY;
    startHeight = editorHeight;
    document.addEventListener("mousemove", handleResize);
    document.addEventListener("mouseup", stopResize);
    document.body.style.cursor = "ns-resize";
    document.body.style.userSelect = "none";
  }

  function handleResize(e: MouseEvent) {
    if (!isResizing) return;
    const delta = e.clientY - startY;
    const newHeight = Math.max(80, Math.min(500, startHeight + delta));
    editorHeight = newHeight;
  }

  function stopResize() {
    isResizing = false;
    document.removeEventListener("mousemove", handleResize);
    document.removeEventListener("mouseup", stopResize);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }

  // Custom theme using CSS variables
  // Note: Avoid overriding lineHeight - it breaks CodeMirror's cursor positioning
  const customTheme = EditorView.theme({
    "&": {
      backgroundColor: "var(--bg-primary)",
      color: "var(--text-primary)",
      fontSize: "13px",
      height: "100%",
    },
    ".cm-content": {
      fontFamily: "var(--font-mono)",
      caretColor: "var(--text-primary)",
      padding: "var(--spacing-md)",
    },
    ".cm-cursor": {
      borderLeftColor: "var(--text-primary)",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
      backgroundColor: "rgba(255, 255, 255, 0.1)",
    },
    ".cm-activeLine": {
      backgroundColor: "transparent",
    },
    ".cm-gutters": {
      display: "none",
    },
    ".cm-placeholder": {
      color: "var(--text-muted)",
    },
    "&.cm-focused": {
      outline: "none",
    },
  });

  onMount(async () => {
    // Load write mode state
    const savedState = await readPluginState<QueryPluginState>(PLUGIN_ID);
    if (savedState) {
      writeModeEnabled = savedState.writeModeEnabled ?? false;
      writeModeWarningDismissed = savedState.writeModeWarningDismissed ?? false;
    }

    // Load history and schema automatically
    history = await loadHistoryAsync();
    loadSchemaData(); // Load tables in background (don't await)

    const state = EditorState.create({
      doc: query,
      extensions: [
        basicSetup,
        sqlCompartment.of(getSqlExtension(schema)),
        editorThemeCompartment.of(getEditorThemeExtension()),
        customTheme,
        // High-precedence keymap for query execution - runs before default Enter handling
        Prec.high(keymap.of([
          { key: "Mod-Enter", run: () => { runQuery(); return true; } },
        ])),
      ],
    });

    editorView = new EditorView({
      state,
      parent: editorContainer,
    });
  });

  onDestroy(() => {
    editorView?.destroy();
  });

  // Subscribe to theme changes and update editor theme
  $effect(() => {
    return themeManager.subscribe(() => {
      if (editorView) {
        editorView.dispatch({
          effects: editorThemeCompartment.reconfigure(getEditorThemeExtension())
        });
      }
    });
  });

  // Update SQL extension when schema loads (for autocomplete)
  $effect(() => {
    if (editorView && schema.length > 0) {
      editorView.dispatch({
        effects: sqlCompartment.reconfigure(getSqlExtension(schema))
      });
    }
  });

  function handleGlobalKeyDown(e: KeyboardEvent) {
    // Only handle if we're inside the query view
    if (!viewEl?.contains(document.activeElement) && document.activeElement !== document.body) {
      return;
    }

    const isMod = e.metaKey || e.ctrlKey;

    // Cmd/Ctrl + Enter to run query
    // Skip if editor has focus - CodeMirror has its own Mod-Enter handler
    if (isMod && e.key === "Enter") {
      if (editorContainer?.contains(document.activeElement)) {
        return;
      }
      e.preventDefault();
      runQuery();
    }
    // Cmd/Ctrl + L to clear
    else if (isMod && e.key === "l") {
      e.preventDefault();
      clearQuery();
    }
    // Cmd/Ctrl + Shift + F to format
    else if (isMod && e.shiftKey && e.key === "f") {
      e.preventDefault();
      formatQuery();
    }
  }

  // Example queries
  const examples = [
    {
      name: "Recent transactions",
      query: "SELECT transaction_date, description, amount, tags FROM transactions ORDER BY transaction_date DESC LIMIT 20",
    },
    {
      name: "Spending by tag",
      query: "SELECT tag, SUM(amount) as total FROM (SELECT unnest(tags) as tag, amount FROM transactions WHERE amount < 0) GROUP BY tag ORDER BY total",
    },
    {
      name: "Monthly spending",
      query: "SELECT strftime('%Y-%m', transaction_date) as month, SUM(amount) as total FROM transactions WHERE amount < 0 GROUP BY month ORDER BY month DESC LIMIT 12",
    },
    {
      name: "Untagged transactions",
      query: "SELECT transaction_date, description, amount FROM transactions WHERE CAST(tags AS VARCHAR) = '[]' ORDER BY transaction_date DESC LIMIT 50",
    },
  ];

  function loadExample(exampleQuery: string) {
    // Directly update editor
    if (editorView) {
      editorView.dispatch({
        changes: { from: 0, to: editorView.state.doc.length, insert: exampleQuery }
      });
    }
  }

  function exportCSV() {
    if (!result) return;

    const escapeCSV = (val: unknown): string => {
      if (val === null) return "";
      if (Array.isArray(val)) val = `[${val.join(", ")}]`;
      const str = String(val);
      if (str.includes(",") || str.includes('"') || str.includes("\n")) {
        return `"${str.replace(/"/g, '""')}"`;
      }
      return str;
    };

    const header = result.columns.map(escapeCSV).join(",");
    const rows = result.rows.map((row) => row.map(escapeCSV).join(","));
    const csv = [header, ...rows].join("\n");

    downloadFile(csv, "query-results.csv", "text/csv");
  }

  function exportJSON() {
    if (!result) return;

    const data = result.rows.map((row) => {
      const obj: Record<string, unknown> = {};
      result!.columns.forEach((col, i) => {
        obj[col] = row[i];
      });
      return obj;
    });

    const json = JSON.stringify(data, null, 2);
    downloadFile(json, "query-results.json", "application/json");
  }

  function downloadFile(content: string, filename: string, mimeType: string) {
    const blob = new Blob([content], { type: mimeType });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }

  function formatQuery() {
    if (!editorView) return;

    // Simple SQL formatter
    const keywords = [
      "SELECT", "FROM", "WHERE", "AND", "OR", "ORDER BY", "GROUP BY",
      "HAVING", "LIMIT", "OFFSET", "JOIN", "LEFT JOIN", "RIGHT JOIN",
      "INNER JOIN", "OUTER JOIN", "ON", "AS", "DISTINCT", "UNION",
      "INSERT INTO", "VALUES", "UPDATE", "SET", "DELETE FROM", "CREATE",
      "DROP", "ALTER", "WITH"
    ];

    let formatted = editorView.state.doc.toString().trim();

    // Normalize whitespace
    formatted = formatted.replace(/\s+/g, " ");

    // Add newlines before major keywords
    const majorKeywords = [
      "SELECT", "FROM", "WHERE", "ORDER BY", "GROUP BY", "HAVING",
      "LIMIT", "JOIN", "LEFT JOIN", "RIGHT JOIN", "INNER JOIN",
      "OUTER JOIN", "UNION", "WITH"
    ];

    for (const kw of majorKeywords) {
      const regex = new RegExp(`\\b(${kw})\\b`, "gi");
      formatted = formatted.replace(regex, "\n$1");
    }

    // Add newlines and indentation for AND/OR
    formatted = formatted.replace(/\b(AND|OR)\b/gi, "\n  $1");

    // Uppercase keywords
    for (const kw of keywords) {
      const regex = new RegExp(`\\b(${kw})\\b`, "gi");
      formatted = formatted.replace(regex, kw);
    }

    // Clean up leading newline and extra spaces
    formatted = formatted.trim();

    // Update editor directly
    editorView.dispatch({
      changes: { from: 0, to: editorView.state.doc.length, insert: formatted }
    });
  }

  // Handle write mode toggle
  function handleWriteModeToggle(event: Event) {
    const checkbox = event.target as HTMLInputElement;
    const isEnabling = checkbox.checked;

    if (isEnabling) {
      // Turning on - check if we need to show warning
      if (!writeModeWarningDismissed) {
        dontShowAgainChecked = false; // Reset checkbox when modal opens
        showWriteWarningModal = true;
        // Reset checkbox until user confirms
        checkbox.checked = false;
      } else {
        writeModeEnabled = true;
        saveWriteModeState();
      }
    } else {
      // Turning off - no warning needed
      writeModeEnabled = false;
      saveWriteModeState();
    }
  }

  // Confirm enabling write mode from modal
  async function confirmEnableWriteMode(dontShowAgain: boolean) {
    writeModeEnabled = true;
    if (dontShowAgain) {
      writeModeWarningDismissed = true;
    }
    showWriteWarningModal = false;
    await saveWriteModeState();
  }

  // Cancel write mode from modal
  function cancelWriteMode() {
    showWriteWarningModal = false;
  }

  // Create backup from modal
  async function handleCreateBackup() {
    const { createBackup } = await import("../../sdk/settings");
    isCreatingBackup = true;
    try {
      const result = await createBackup(10);
      toast.success("Backup created", result.name);
    } catch (e) {
      console.error("Failed to create backup:", e);
      toast.error("Backup failed", e instanceof Error ? e.message : "Unknown error");
    } finally {
      isCreatingBackup = false;
    }
  }
</script>

<svelte:window onkeydown={handleGlobalKeyDown} />

<div class="query-view" bind:this={viewEl} role="region" aria-label="SQL Query Editor">
  <div class="query-panel" role="form" aria-label="Query input">
    <div class="panel-header">
      <div class="panel-header-left">
        <h2 class="panel-title">SQL Query</h2>
        <label class="write-mode-toggle" class:enabled={writeModeEnabled}>
          <input
            type="checkbox"
            checked={writeModeEnabled}
            onchange={handleWriteModeToggle}
            aria-describedby="write-mode-description"
          />
          <span class="write-mode-label">Write mode</span>
        </label>
      </div>
      <div class="header-actions" role="toolbar" aria-label="Query actions">
        <div class="history-container">
          <button
            class="history-button"
            class:active={showHistory}
            onclick={() => (showHistory = !showHistory)}
            disabled={history.length === 0}
            aria-expanded={showHistory}
            aria-haspopup="listbox"
            aria-label={`Query history, ${history.length} entries`}
          >
            History ({history.length})
          </button>
          {#if showHistory}
            <div class="history-dropdown" role="listbox" aria-label="Query history">
              <div class="history-header">
                <span id="history-title">Query History</span>
                <button class="clear-history" onclick={clearHistory} aria-label="Clear all history">Clear</button>
              </div>
              <div class="history-list" aria-labelledby="history-title">
                {#each history as entry, i}
                  <button
                    class="history-item"
                    class:failed={!entry.success}
                    onclick={() => loadFromHistory(entry)}
                    role="option"
                    aria-selected="false"
                    aria-label={`${entry.success ? '' : 'Failed: '}${entry.query.slice(0, 50)}${entry.query.length > 50 ? '...' : ''}, ${formatTimestamp(entry.timestamp)}`}
                  >
                    <pre class="history-query">{entry.query}</pre>
                    <span class="history-time">{formatTimestamp(entry.timestamp)}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}
        </div>
        <button class="format-button" onclick={formatQuery} aria-label="Format SQL query">
          Format
        </button>
        <button
          class="schema-button"
          class:active={showSchema}
          onclick={loadSchema}
          disabled={schemaLoading}
          aria-expanded={showSchema}
          aria-label={schemaLoading ? "Loading database schema" : "Show database schema"}
        >
          {schemaLoading ? "Loading..." : "Schema"}
        </button>
        <button class="run-button" onclick={runQuery} disabled={isLoading} aria-label="Run query (Command+Enter)">
          {isLoading ? "Running..." : "Run Query"}
          <span class="shortcut" aria-hidden="true">{modKey()}↵</span>
        </button>
      </div>
    </div>

    <div class="query-editor-container">
      <div class="query-editor" class:write-mode={writeModeEnabled} bind:this={editorContainer} role="textbox" aria-label="SQL query input" aria-multiline="true" style="height: {editorHeight}px;"></div>
      {#if showSchema}
        <aside class="schema-panel" aria-label="Database schema">
          <div class="schema-header">
            <span id="schema-title">Database Schema</span>
            <button class="schema-close" onclick={() => showSchema = false} aria-label="Close schema panel">×</button>
          </div>
          <div class="schema-content" aria-labelledby="schema-title">
            {#each [...schema].sort((a, b) => {
              const aIsSys = a.name.startsWith('sys_');
              const bIsSys = b.name.startsWith('sys_');
              if (aIsSys !== bIsSys) return aIsSys ? 1 : -1;
              return a.name.localeCompare(b.name);
            }) as table}
              <div class="schema-table" class:sys-table={table.name.startsWith('sys_')}>
                <div class="table-name" role="heading" aria-level="3">{table.name}</div>
                <ul class="table-columns" aria-label={`Columns in ${table.name}`}>
                  {#each table.columns as column}
                    <li class="column-row">
                      <span class="column-name">{column.name}</span>
                      <span class="column-type" aria-label="type">{column.type}</span>
                    </li>
                  {/each}
                </ul>
              </div>
            {/each}
          </div>
        </aside>
      {/if}
    </div>

    <div class="query-footer">
      <div class="examples" role="group" aria-label="Example queries">
        <div class="examples-label" id="examples-label">Examples:</div>
        <div class="examples-list" aria-labelledby="examples-label">
          {#each examples as example}
            <button class="example-button" onclick={() => loadExample(example.query)} aria-label={`Load example: ${example.name}`}>
              {example.name}
            </button>
          {/each}
        </div>
      </div>
      <div class="shortcuts" aria-label="Keyboard shortcuts">
        <span class="shortcut-item"><kbd>{modKey()}</kbd><kbd>↵</kbd> Run</span>
        <span class="shortcut-item"><kbd>{modKey()}</kbd><kbd>L</kbd> Clear</span>
        <span class="shortcut-item"><kbd>{modKey()}</kbd><kbd>⇧</kbd><kbd>F</kbd> Format</span>
      </div>
    </div>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="resize-handle"
    class:resizing={isResizing}
    onmousedown={startResize}
    role="separator"
    aria-orientation="horizontal"
    aria-label="Resize query editor"
  >
    <div class="resize-handle-line"></div>
  </div>

  <div class="results-panel" role="region" aria-label="Query results" aria-live="polite">
    {#if isLoading}
      <div class="status" role="status" aria-busy="true">
        <div class="spinner" aria-hidden="true"></div>
        <span>Running query...</span>
      </div>
    {:else if error}
      <div class="status error" role="alert">
        <svg class="error-icon" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-2h2v2zm0-4h-2V7h2v6z"/>
        </svg>
        <div class="error-content">
          <div class="error-title">Query Error</div>
          <pre class="error-message">{error}</pre>
        </div>
      </div>
    {:else if result}
      <div class="results-content">
        <div class="results-header">
          <div class="results-meta" aria-live="polite">
            <span class="result-count">{result.row_count} {result.row_count === 1 ? 'row' : 'rows'}</span>
            {#if executionTime !== null}
              <span class="execution-time" aria-label={`Query took ${executionTime < 1000 ? Math.round(executionTime) + ' milliseconds' : (executionTime / 1000).toFixed(2) + ' seconds'}`}>
                {executionTime < 1000
                  ? `${Math.round(executionTime)}ms`
                  : `${(executionTime / 1000).toFixed(2)}s`}
              </span>
            {/if}
          </div>
          <div class="export-buttons" role="group" aria-label="Export options">
            <button class="export-button" onclick={exportCSV} aria-label="Export results as CSV">Export CSV</button>
            <button class="export-button" onclick={exportJSON} aria-label="Export results as JSON">Export JSON</button>
          </div>
        </div>

        {#if result.row_count === 0}
          <div class="no-results" role="status">No results returned</div>
        {:else}
          <div class="table-container">
            <table class="results-table" aria-label="Query results">
              <thead>
                <tr>
                  {#each result.columns as column, i}
                    <th
                      class="sortable"
                      class:sorted={sortColumn === i}
                      onclick={() => toggleSort(i)}
                      aria-sort={sortColumn === i ? (sortDirection === "asc" ? "ascending" : "descending") : "none"}
                      role="columnheader"
                      tabindex="0"
                      onkeydown={(e) => e.key === 'Enter' && toggleSort(i)}
                    >
                      <span class="column-name">{column}</span>
                      {#if sortColumn === i}
                        <span class="sort-indicator" aria-hidden="true">{sortDirection === "asc" ? "▲" : "▼"}</span>
                      {/if}
                    </th>
                  {/each}
                </tr>
              </thead>
              <tbody>
                {#each sortedRows as row, rowIndex}
                  <tr>
                    {#each row as cell, colIndex}
                      <td
                        class="copyable"
                        class:copied={copiedCell?.row === rowIndex && copiedCell?.col === colIndex}
                        onclick={() => copyCell(cell, rowIndex, colIndex)}
                        onkeydown={(e) => e.key === 'Enter' && copyCell(cell, rowIndex, colIndex)}
                        tabindex="0"
                        role="gridcell"
                        aria-label={`${result.columns[colIndex]}: ${cell === null ? 'null' : Array.isArray(cell) ? cell.join(', ') : cell}. Click to copy`}
                      >
                        {#if copiedCell?.row === rowIndex && copiedCell?.col === colIndex}
                          <span class="copied-indicator" role="status">Copied!</span>
                        {:else if cell === null}
                          <span class="null-value">NULL</span>
                        {:else if Array.isArray(cell)}
                          <span class="array-value">[{cell.join(", ")}]</span>
                        {:else}
                          {cell}
                        {/if}
                      </td>
                    {/each}
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </div>
    {:else}
      <div class="status empty with-browser" role="status">
        <div class="tables-panel">
          {#if schemaLoading}
            <div class="loading-tables">Loading tables...</div>
          {:else if schema.length > 0}
            <div class="tables-header">Available Tables</div>
            <nav class="tables-list" aria-label="Database tables">
              {#each [...schema].sort((a, b) => {
                const aIsSys = a.name.startsWith('sys_');
                const bIsSys = b.name.startsWith('sys_');
                if (aIsSys !== bIsSys) return aIsSys ? 1 : -1;
                return a.name.localeCompare(b.name);
              }) as table}
                <button
                  class="table-item"
                  class:sys-table={table.name.startsWith('sys_')}
                  class:selected={selectedTableSchema?.name === table.name}
                  onclick={() => selectTable(table)}
                  aria-label={`View schema for ${table.name}`}
                >
                  <span class="table-name">{table.name}</span>
                  <span class="table-cols">{table.columns.length} cols</span>
                </button>
              {/each}
            </nav>
          {:else}
            <div class="empty-hint">
              <svg class="empty-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
                <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
              </svg>
              <span>Run a query to see results</span>
            </div>
          {/if}
        </div>
        <div class="schema-panel">
          {#if selectedTableSchema}
            {@const table = selectedTableSchema}
            <div class="schema-panel-header">
              <span class="schema-panel-title">{table.name}</span>
            </div>
            <div class="schema-panel-columns">
              {#each table.columns as col}
                <div class="schema-col">
                  <span class="schema-col-name">{col.name}</span>
                  <span class="schema-col-type">{col.type}</span>
                </div>
              {/each}
            </div>
          {:else}
            <div class="schema-panel-empty">
              <span>Click a table to view its schema</span>
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<!-- Write Mode Info Modal -->
{#if showWriteWarningModal}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="modal-overlay" onclick={cancelWriteMode} onkeydown={(e) => e.key === 'Escape' && cancelWriteMode()} role="dialog" aria-modal="true" aria-labelledby="write-warning-title" tabindex="-1">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="modal-content" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h3 id="write-warning-title">Write Mode</h3>
      </div>
      <div class="modal-body">
        <div class="info-icon-container">
          <Icon name="info" size={32} class="info-icon" />
        </div>
        <p class="info-text">
          Write mode allows you to run queries that modify data
          (<code>INSERT</code>, <code>UPDATE</code>, <code>DELETE</code>, etc.).
          Changes are permanent and cannot be undone.
        </p>
        <div class="backup-section">
          <p class="backup-hint">Consider creating a backup before making changes:</p>
          <button
            class="backup-button"
            onclick={handleCreateBackup}
            disabled={isCreatingBackup}
          >
            {isCreatingBackup ? "Creating..." : "Create Backup"}
          </button>
        </div>
        <label class="dont-show-again">
          <input type="checkbox" bind:checked={dontShowAgainChecked} />
          <span>Don't show this again</span>
        </label>
      </div>
      <div class="modal-footer">
        <button class="btn secondary" onclick={cancelWriteMode}>Cancel</button>
        <button class="btn primary" onclick={() => confirmEnableWriteMode(dontShowAgainChecked)}>Enable</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .query-view {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-primary);
  }

  .query-panel {
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
    padding: var(--spacing-lg);
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
  }

  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .panel-header-left {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
  }

  .header-actions {
    display: flex;
    gap: var(--spacing-sm);
    align-items: center;
  }

  .history-container {
    position: relative;
  }

  .history-button {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
  }

  .history-button:hover:not(:disabled) {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .history-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .history-button.active {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
    color: var(--text-primary);
  }

  .format-button {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
  }

  .format-button:hover:not(:disabled) {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .format-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .history-dropdown {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: var(--spacing-xs);
    width: 400px;
    max-height: 300px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    z-index: 100;
    display: flex;
    flex-direction: column;
  }

  .history-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-sm) var(--spacing-md);
    border-bottom: 1px solid var(--border-primary);
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .clear-history {
    background: none;
    border: none;
    color: var(--accent-danger);
    font-size: 11px;
    cursor: pointer;
    padding: 2px var(--spacing-xs);
  }

  .clear-history:hover {
    text-decoration: underline;
  }

  .history-list {
    flex: 1;
    overflow-y: auto;
  }

  .history-item {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--spacing-xs);
    padding: var(--spacing-sm) var(--spacing-md);
    background: none;
    border: none;
    border-bottom: 1px solid var(--border-primary);
    cursor: pointer;
    text-align: left;
    transition: background 0.2s;
  }

  .history-item:hover {
    background: var(--bg-tertiary);
  }

  .history-item:last-child {
    border-bottom: none;
  }

  .history-item.failed {
    border-left: 2px solid var(--accent-danger);
  }

  .history-query {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-primary);
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  .history-time {
    font-size: 10px;
    color: var(--text-muted);
  }

  .panel-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .run-button {
    background: var(--accent-primary);
    color: var(--bg-primary);
    border: none;
    border-radius: var(--radius-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    transition: opacity 0.2s;
  }

  .run-button:hover:not(:disabled) {
    opacity: 0.9;
  }

  .run-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .shortcut {
    font-family: var(--font-mono);
    font-size: 11px;
    opacity: 0.7;
  }

  .query-editor {
    width: 100%;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .query-editor:focus-within {
    border-color: var(--accent-primary);
  }

  .query-editor :global(.cm-editor) {
    height: 100%;
  }

  .query-editor :global(.cm-scroller) {
    overflow: auto;
  }

  .resize-handle {
    height: 8px;
    background: var(--bg-secondary);
    cursor: ns-resize;
    display: flex;
    align-items: center;
    justify-content: center;
    border-bottom: 1px solid var(--border-primary);
    transition: background 0.15s;
  }

  .resize-handle:hover,
  .resize-handle.resizing {
    background: var(--bg-tertiary);
  }

  .resize-handle-line {
    width: 40px;
    height: 3px;
    background: var(--border-primary);
    border-radius: 2px;
    transition: background 0.15s;
  }

  .resize-handle:hover .resize-handle-line,
  .resize-handle.resizing .resize-handle-line {
    background: var(--text-muted);
  }

  .query-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .examples {
    display: flex;
    gap: var(--spacing-sm);
    align-items: center;
  }

  .shortcuts {
    display: flex;
    gap: var(--spacing-md);
    font-size: 11px;
    color: var(--text-muted);
  }

  .shortcut-item {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .shortcut-item kbd {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 3px;
    padding: 1px 4px;
    font-family: var(--font-mono);
    font-size: 10px;
  }

  .examples-label {
    font-size: 12px;
    color: var(--text-muted);
    font-weight: 500;
  }

  .examples-list {
    display: flex;
    gap: var(--spacing-xs);
    flex-wrap: wrap;
  }

  .example-button {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    padding: 4px var(--spacing-sm);
    font-size: 11px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
  }

  .example-button:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }

  .results-panel {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .status {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    gap: var(--spacing-md);
    flex: 1;
    min-height: 0;
    color: var(--text-muted);
    font-size: 14px;
  }

  .status.error {
    color: var(--accent-danger);
  }

  .status.empty {
    color: var(--text-muted);
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--border-primary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .error-icon,
  .empty-icon {
    width: 48px;
    height: 48px;
  }

  .error-icon {
    color: var(--accent-danger);
  }

  .empty-icon {
    color: var(--accent-primary);
    opacity: 0.6;
  }

  .error-content {
    max-width: 600px;
    text-align: center;
  }

  .error-title {
    font-weight: 600;
    margin-bottom: var(--spacing-sm);
  }

  .error-message {
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--bg-secondary);
    padding: var(--spacing-md);
    border-radius: var(--radius-sm);
    overflow-x: auto;
    text-align: left;
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .results-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .results-header {
    padding: var(--spacing-md) var(--spacing-lg);
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .results-meta {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
  }

  .result-count {
    font-size: 12px;
    font-family: var(--font-mono);
    color: var(--text-muted);
  }

  .execution-time {
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--text-muted);
    padding: 2px 6px;
    background: var(--bg-primary);
    border-radius: var(--radius-sm);
  }

  .export-buttons {
    display: flex;
    gap: var(--spacing-xs);
  }

  .export-button {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    padding: 4px var(--spacing-sm);
    font-size: 11px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
  }

  .export-button:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }

  .no-results {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    color: var(--text-muted);
    font-size: 14px;
  }

  .table-container {
    flex: 1;
    overflow: auto;
  }

  .results-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }

  .results-table thead {
    position: sticky;
    top: 0;
    background: var(--bg-secondary);
    z-index: 10;
  }

  .results-table th {
    text-align: left;
    padding: var(--spacing-sm) var(--spacing-md);
    font-weight: 600;
    color: var(--text-secondary);
    border-bottom: 2px solid var(--border-primary);
    white-space: nowrap;
  }

  .results-table th.sortable {
    cursor: pointer;
    user-select: none;
    transition: color 0.2s;
  }

  .results-table th.sortable:hover {
    color: var(--text-primary);
  }

  .results-table th.sorted {
    color: var(--accent-primary);
  }

  .results-table th .column-name {
    margin-right: var(--spacing-xs);
  }

  .results-table th .sort-indicator {
    font-size: 10px;
    opacity: 0.8;
  }

  .results-table td {
    padding: var(--spacing-sm) var(--spacing-md);
    border-bottom: 1px solid var(--border-primary);
    color: var(--text-primary);
    max-width: 400px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .results-table td.copyable {
    cursor: pointer;
    transition: background 0.15s;
  }

  .results-table td.copyable:hover {
    background: var(--bg-tertiary);
  }

  .results-table td.copied {
    background: rgba(152, 195, 121, 0.2);
  }

  .copied-indicator {
    color: #98c379;
    font-size: 11px;
    font-weight: 500;
  }

  .results-table tbody tr:hover {
    background: var(--bg-secondary);
  }

  .null-value {
    color: var(--text-muted);
    font-style: italic;
    font-size: 11px;
  }

  .array-value {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--accent-primary);
  }

  /* Schema button and panel */
  .schema-button {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
  }

  .schema-button:hover:not(:disabled) {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .schema-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .schema-button.active {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
    color: var(--text-primary);
  }

  .query-editor-container {
    display: flex;
    gap: var(--spacing-md);
  }

  .query-editor-container .query-editor {
    flex: 1;
  }

  .schema-panel {
    width: 280px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
    max-height: 200px;
    overflow: hidden;
  }

  .schema-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-sm) var(--spacing-md);
    border-bottom: 1px solid var(--border-primary);
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .schema-close {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0;
    line-height: 1;
  }

  .schema-close:hover {
    color: var(--text-primary);
  }

  .schema-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-sm);
  }

  .schema-table {
    margin-bottom: var(--spacing-sm);
  }

  .schema-table:last-child {
    margin-bottom: 0;
  }

  .schema-table .table-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-primary);
    margin-bottom: var(--spacing-xs);
    padding: 2px var(--spacing-xs);
    background: var(--bg-tertiary);
    border-radius: var(--radius-sm);
  }

  .schema-table.sys-table {
    opacity: 0.5;
  }

  .schema-table.sys-table .table-name {
    color: var(--text-muted);
  }

  .table-columns {
    padding-left: var(--spacing-sm);
    list-style: none;
    margin: 0;
  }

  .column-row {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    padding: 2px 0;
  }

  ul.table-columns {
    padding-left: var(--spacing-sm);
  }

  li.column-row {
    list-style: none;
  }

  .column-row .column-name {
    color: var(--text-primary);
    font-family: var(--font-mono);
  }

  .column-row .column-type {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 10px;
  }

  /* Empty state schema display */
  /* Tables browser in empty state - two panel layout */
  .status.empty.with-browser {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  .tables-panel {
    position: absolute;
    top: 0;
    left: 0;
    bottom: 0;
    width: 260px;
    display: flex;
    flex-direction: column;
    padding: var(--spacing-md);
    overflow-y: auto;
    border-right: 1px solid var(--border-primary);
    background: var(--bg-primary);
  }

  .empty-hint {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    gap: var(--spacing-sm);
  }

  .loading-tables {
    color: var(--text-muted);
    font-size: 13px;
    padding: var(--spacing-lg);
  }

  .tables-header {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: var(--spacing-sm);
    padding: 0 var(--spacing-xs);
  }

  .tables-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .table-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    padding: var(--spacing-xs) var(--spacing-sm);
    font-size: 12px;
    font-family: var(--font-mono);
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    transition: background 0.15s;
  }

  .table-item:hover {
    background: var(--bg-secondary);
  }

  .table-item.selected {
    background: var(--bg-tertiary);
    color: var(--accent-primary);
  }

  .table-item.sys-table {
    opacity: 0.6;
  }

  .table-item.sys-table:hover {
    opacity: 0.8;
  }

  .table-name {
    flex: 1;
  }

  .table-cols {
    font-size: 10px;
    color: var(--text-muted);
  }

  /* Schema panel on right side (inside empty state) */
  .status.empty.with-browser .schema-panel {
    position: absolute;
    top: 0;
    left: 260px;
    right: 0;
    bottom: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    overflow: hidden;
    border: none;
    border-radius: 0;
    width: auto;
    max-height: none;
  }

  .schema-panel-header {
    padding: var(--spacing-sm) var(--spacing-md);
    border-bottom: 1px solid var(--border-primary);
    background: var(--bg-tertiary);
    flex-shrink: 0;
  }

  .schema-panel-title {
    font-size: 13px;
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--accent-primary);
  }

  .schema-panel-columns {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-md);
  }

  .schema-panel-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    color: var(--text-muted);
    font-size: 13px;
  }

  .schema-col {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-xs) var(--spacing-sm);
    font-size: 12px;
    border-radius: var(--radius-sm);
  }

  .schema-col:hover {
    background: var(--bg-tertiary);
  }

  .schema-col-name {
    font-family: var(--font-mono);
    color: var(--text-primary);
  }

  .schema-col-type {
    font-size: 10px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    text-transform: uppercase;
  }

  /* Write mode toggle */
  .write-mode-toggle {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    padding: var(--spacing-xs) var(--spacing-sm);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: 11px;
    color: var(--text-muted);
    transition: all 0.2s;
  }

  .write-mode-toggle:hover {
    color: var(--text-secondary);
  }

  .write-mode-toggle.enabled {
    color: var(--accent-warning);
  }

  .write-mode-toggle input[type="checkbox"] {
    margin: 0;
    cursor: pointer;
    accent-color: var(--accent-warning);
  }

  .write-mode-label {
    user-select: none;
  }

  /* Editor write mode visual indicator */
  .query-editor.write-mode {
    border-color: var(--accent-warning);
  }

  /* Modal styles */
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-content {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-lg);
    width: 400px;
    max-width: 90vw;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
  }

  .modal-header {
    padding: var(--spacing-md) var(--spacing-lg);
    border-bottom: 1px solid var(--border-primary);
  }

  .modal-header h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .modal-body {
    padding: var(--spacing-lg);
  }

  .info-icon-container {
    display: flex;
    justify-content: center;
    margin-bottom: var(--spacing-md);
  }

  .info-icon-container :global(.info-icon) {
    color: var(--accent-warning);
  }

  .info-text {
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.6;
    margin: 0 0 var(--spacing-md) 0;
    text-align: center;
  }

  .info-text code {
    background: var(--bg-tertiary);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-primary);
  }

  .backup-section {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    padding: var(--spacing-md);
    margin-bottom: var(--spacing-md);
    text-align: center;
  }

  .backup-hint {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0 0 var(--spacing-sm) 0;
  }

  .backup-button {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
    transition: all 0.2s;
  }

  .backup-button:hover:not(:disabled) {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
  }

  .backup-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .dont-show-again {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .dont-show-again input {
    margin: 0;
    cursor: pointer;
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
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
  }

  .btn.secondary {
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    color: var(--text-primary);
  }

  .btn.secondary:hover {
    background: var(--bg-primary);
  }

  .btn.primary {
    background: var(--accent-primary);
    border: none;
    color: white;
  }

  .btn.primary:hover {
    opacity: 0.9;
  }
</style>
