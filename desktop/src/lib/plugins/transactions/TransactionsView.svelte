<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { executeQuery, executeQueryWithParams, type QueryParam, showToast, registry, modKey, getPluginSettings, updatePluginSettings } from "../../sdk";
  import { RowMenu, type RowMenuItem, Icon, CopyButton, formatUserCurrency, TagInput, Modal } from "../../shared";
  import { FrequencyBasedSuggester } from "./suggestions";
  import type { Transaction, TagSuggestion, SplitAmount, AccountInfo } from "./types";
  import DeleteConfirmModal from "./DeleteConfirmModal.svelte";
  import UnsplitConfirmModal from "./UnsplitConfirmModal.svelte";
  import SplitTransactionModal from "./SplitTransactionModal.svelte";
  import AddTransactionModal from "./AddTransactionModal.svelte";
  import EditTransactionModal from "./EditTransactionModal.svelte";
  import RulesManagementModal from "./RulesManagementModal.svelte";
  import PinnedTagsModal from "./PinnedTagsModal.svelte";

  // Props - accountId can be passed to filter to a specific account
  interface Props {
    accountId?: string;
  }
  let { accountId }: Props = $props();

  // Initialize suggester
  const suggester = new FrequencyBasedSuggester();

  let transactions = $state<Transaction[]>([]);
  let suggestions = $state<Map<string, TagSuggestion[]>>(new Map());
  let isLoading = $state(true);
  let isLoadingMore = $state(false);
  let error = $state<string | null>(null);
  let hasMore = $state(true);
  const PAGE_SIZE = 200;

  // Selection state
  let selectedIndices = $state<Set<number>>(new Set());
  let cursorIndex = $state(0);

  // Filter state - default to "all"
  let filterMode = $state<"all" | "untagged">("all");
  let searchQuery = $state("");
  let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null;

  // Derived: check if search input has focus (replaces isSearching flag)
  function isSearchFocused(): boolean {
    return document.activeElement === searchInputEl;
  }

  // Account filter state (multi-select)
  const ACCOUNT_FILTER_KEY = "tagging-selected-accounts";
  let selectedAccounts = $state<Set<string>>(new Set());
  let availableAccounts = $state<string[]>([]);
  let accountsWithIds = $state<AccountInfo[]>([]);
  let isAccountDropdownOpen = $state(false);
  let accountCursorIndex = $state(0);

  function loadPersistedAccounts(): Set<string> {
    try {
      const stored = localStorage.getItem(ACCOUNT_FILTER_KEY);
      if (stored) {
        const accounts = JSON.parse(stored) as string[];
        return new Set(accounts);
      }
    } catch (e) {
      console.error("Failed to load persisted accounts:", e);
    }
    return new Set();
  }

  function persistSelectedAccounts() {
    try {
      const accounts = Array.from(selectedAccounts);
      localStorage.setItem(ACCOUNT_FILTER_KEY, JSON.stringify(accounts));
    } catch (e) {
      console.error("Failed to persist accounts:", e);
    }
  }

  // Inline tag editing mode (on the transaction row)
  let inlineEditingIndex = $state<number | null>(null); // Which row is being edited, null = none
  let inlineTagInput = $state("");
  let inlineTagInputRef = $state<{ focus: () => void } | null>(null);

  // Legacy custom tag input mode (kept for sidebar, but will be removed)
  let isCustomTagging = $state(false);
  let customTagInput = $state("");
  let autocompleteIndex = $state(-1); // -1 = no selection, 0+ = selected suggestion

  // Pinned quick tags (user-specified, always shown first)
  interface TaggingPluginSettings extends Record<string, unknown> {
    pinnedQuickTags: string[];
  }
  const DEFAULT_TAGGING_SETTINGS: TaggingPluginSettings = { pinnedQuickTags: [] };
  let pinnedQuickTags = $state<string[]>([]);
  let hasAnyTags = $state(false); // True if user has tagged any transactions
  let showPinnedTagsModal = $state(false);

  // Undo stack for tag operations
  interface UndoEntry {
    transactionId: string;
    previousTags: string[];
    newTags: string[];
  }
  let undoStack = $state<UndoEntry[][]>([]); // Each entry is an array of changes (for bulk ops)
  const MAX_UNDO_DEPTH = 20;

  // Flag to skip reload when we triggered the data:refresh ourselves
  let isInternalUpdate = false;

  // Transaction edit modal
  let isTagModalOpen = $state(false);
  let editingTransaction = $state<Transaction | null>(null);
  // Delete confirmation
  let showDeleteConfirm = $state(false);
  // Bulk delete confirmation
  let showBulkDeleteConfirm = $state(false);
  let bulkDeleteCount = $state(0);

  // Split modal
  let showSplitModal = $state(false);

  // Row context menu
  let contextMenuTxn = $state<Transaction | null>(null);

  // Rules modal (unified)
  let showRulesModal = $state(false);
  let rulesModalMode = $state<"list" | "create">("list");
  let rulesModalPrefillTransactions = $state<Transaction[]>([]);
  let rulesModalPrefillTags = $state<string[]>([]);

  // Threshold for prompting to save as rule (min transactions tagged at once)
  const RULE_PROMPT_THRESHOLD = 3;

  // Overflow menu state
  let showOverflowMenu = $state(false);
  let overflowMenuRef = $state<HTMLDivElement | null>(null);

  function toggleOverflowMenu(e: MouseEvent) {
    e.stopPropagation();
    showOverflowMenu = !showOverflowMenu;
  }

  // Close overflow menu when clicking outside
  $effect(() => {
    if (!showOverflowMenu) return;

    function handleClickOutside(e: MouseEvent) {
      if (overflowMenuRef && !overflowMenuRef.contains(e.target as Node)) {
        showOverflowMenu = false;
      }
    }

    // Use setTimeout to avoid immediate close from the toggle click
    setTimeout(() => {
      document.addEventListener("click", handleClickOutside);
    }, 0);

    return () => document.removeEventListener("click", handleClickOutside);
  });

  // Copy to clipboard (mirrors query plugin pattern)

  function getRowMenuItems(txn: Transaction): RowMenuItem[] {
    const items: RowMenuItem[] = [
      { label: "Edit", action: () => handleContextEdit() },
    ];

    if (txn.parent_transaction_id) {
      items.push({ label: "Unsplit", action: () => handleContextUnsplit() });
    } else {
      items.push({ label: "Split", action: () => handleContextSplit() });
    }

    items.push({ label: "Create rule from...", action: () => handleContextCreateRule() });
    items.push({ label: "Delete", action: () => handleContextDelete(), danger: true });

    return items;
  }

  // Element refs
  let customTagInputEl = $state<HTMLInputElement | null>(null);
  let searchInputEl = $state<HTMLInputElement | null>(null);
  let containerEl = $state<HTMLDivElement | null>(null);

  // Pinned tags for display (stable, user-controlled favorites)
  let pinnedTagsDisplay = $derived.by(() => {
    return pinnedQuickTags.slice(0, 9).map((tag, i) => ({
      tag,
      shortcutNumber: i + 1,
      isPinned: true
    }));
  });

  // Computed suggestions for current/selected transactions (dynamic, changes per transaction)
  let computedSuggestionsDisplay = $derived.by(() => {
    const result: { tag: string; score: number; count: number; shortcutNumber: number }[] = [];
    const pinnedSet = new Set(pinnedQuickTags);
    const startNumber = pinnedQuickTags.length + 1;

    // Get computed suggestions from frequency-based suggester
    const indices = selectedIndices.size > 0
      ? Array.from(selectedIndices)
      : [cursorIndex];

    if (indices.length === 0) return result;

    // Aggregate suggestions from all relevant transactions
    const tagScores = new Map<string, { tag: string; score: number; count: number }>();

    for (const idx of indices) {
      const txn = transactions[idx];
      if (!txn) continue;
      const txnSuggestions = suggestions.get(txn.transaction_id) || [];
      for (const s of txnSuggestions) {
        // Skip tags already pinned
        if (pinnedSet.has(s.tag)) continue;

        const existing = tagScores.get(s.tag);
        if (existing) {
          existing.score += s.score;
          existing.count += 1;
        } else {
          tagScores.set(s.tag, { tag: s.tag, score: s.score, count: 1 });
        }
      }
    }

    // Sort computed suggestions and fill remaining slots (up to 9 total)
    const computed = Array.from(tagScores.values())
      .sort((a, b) => b.count - a.count || b.score - a.score);

    let shortcutNum = startNumber;
    for (const suggestion of computed) {
      if (shortcutNum > 9) break;
      result.push({ ...suggestion, shortcutNumber: shortcutNum });
      shortcutNum++;
    }

    return result;
  });

  // Combined list for keyboard shortcuts (1-9 mapping)
  let currentSuggestions = $derived.by(() => {
    const result: { tag: string; score: number; count: number; isPinned?: boolean }[] = [];

    // Add pinned tags first
    for (const tag of pinnedQuickTags) {
      if (result.length >= 9) break;
      result.push({ tag, score: Infinity, count: 0, isPinned: true });
    }

    // Add computed suggestions
    for (const s of computedSuggestionsDisplay) {
      if (result.length >= 9) break;
      result.push({ tag: s.tag, score: s.score, count: s.count });
    }

    return result;
  });

  // Selection stats (Excel-style summary)
  let selectionStats = $derived.by(() => {
    if (selectedIndices.size === 0) return null;

    const selected = Array.from(selectedIndices).map(i => transactions[i]).filter(Boolean);
    if (selected.length === 0) return null;

    const amounts = selected.map(t => t.amount);
    const sum = amounts.reduce((a, b) => a + b, 0);
    const avg = sum / amounts.length;
    const min = Math.min(...amounts);
    const max = Math.max(...amounts);

    // Count positive vs negative
    const positiveCount = amounts.filter(a => a >= 0).length;
    const negativeCount = amounts.filter(a => a < 0).length;

    return {
      count: selected.length,
      sum,
      avg,
      min,
      max,
      positiveCount,
      negativeCount,
    };
  });

  // Group split transactions by parent_id for visual grouping
  let splitGroups = $derived.by(() => {
    const groups = new Map<string, number[]>();
    transactions.forEach((txn, idx) => {
      if (txn.parent_transaction_id) {
        const existing = groups.get(txn.parent_transaction_id) || [];
        existing.push(idx);
        groups.set(txn.parent_transaction_id, existing);
      }
    });
    return groups;
  });

  // Helper to check if a transaction is part of a split group
  function getSplitGroupInfo(txn: Transaction, index: number): { isFirst: boolean; isLast: boolean; siblingCount: number } | null {
    if (!txn.parent_transaction_id) return null;
    const group = splitGroups.get(txn.parent_transaction_id);
    if (!group || group.length < 2) return null;
    const posInGroup = group.indexOf(index);
    return {
      isFirst: posInGroup === 0,
      isLast: posInGroup === group.length - 1,
      siblingCount: group.length
    };
  }

  // Stats for visible transactions
  let visibleTaggedCount = $derived(transactions.filter(t => t.tags.length > 0).length);

  // Selection state helpers
  let isAllSelected = $derived(transactions.length > 0 && selectedIndices.size === transactions.length);
  let isSomeSelected = $derived(selectedIndices.size > 0 && selectedIndices.size < transactions.length);

  function toggleSelectAll() {
    if (isAllSelected) {
      deselectAll();
    } else {
      selectAll();
    }
  }

  // Global stats (all transactions in database)
  let globalStats = $state({ total: 0, tagged: 0 });
  let globalUntaggedCount = $derived(globalStats.total - globalStats.tagged);
  let progressPercent = $derived(globalStats.total > 0 ? Math.floor((globalStats.tagged / globalStats.total) * 100) : 0);
  let isAllTagged = $derived(globalStats.total > 0 && globalStats.tagged === globalStats.total);

  // Celebration state
  let showCelebration = $state(false);
  let prevIsAllTagged = false;

  // Watch for 100% completion (all transactions tagged)
  $effect(() => {
    if (isAllTagged && !prevIsAllTagged && !isLoading) {
      triggerCelebration();
    }
    prevIsAllTagged = isAllTagged;
  });

  // Track previous accountId to detect changes (for when tab is already open)
  let prevAccountId: string | undefined = undefined;

  // Watch for accountId prop changes (when navigating to existing tab)
  $effect(() => {
    // Skip on initial mount (handled by onMount)
    if (prevAccountId === undefined) {
      prevAccountId = accountId;
      return;
    }

    // Only react if accountId actually changed
    if (accountId !== prevAccountId) {
      prevAccountId = accountId;

      if (accountId && accountsWithIds.length > 0) {
        const account = accountsWithIds.find(a => a.id === accountId);
        if (account) {
          selectedAccounts = new Set([account.name]);
          loadTransactions();
        }
      }
    }
  });

  function triggerCelebration() {
    showCelebration = true;
    setTimeout(() => {
      showCelebration = false;
    }, 3000);
  }

  async function loadGlobalStats() {
    try {
      const result = await executeQuery(`
        SELECT
          COUNT(*) as total,
          COUNT(*) FILTER (WHERE len(tags) > 0) as tagged
        FROM transactions
      `);
      if (result.rows.length > 0) {
        globalStats = {
          total: result.rows[0][0] as number,
          tagged: result.rows[0][1] as number
        };
      }
    } catch (e) {
      console.error("Failed to load global stats:", e);
    }
  }

  async function loadAvailableAccounts() {
    try {
      // Load account names for filtering (from transactions view)
      const result = await executeQuery(`
        SELECT DISTINCT account_name
        FROM transactions
        WHERE account_name IS NOT NULL AND account_name != ''
        ORDER BY account_name
      `);
      availableAccounts = result.rows.map(r => r[0] as string);

      // Load all accounts with IDs for the add transaction modal
      const accountsResult = await executeQuery(`
        SELECT account_id, name
        FROM sys_accounts
        WHERE name IS NOT NULL AND name != ''
        ORDER BY name
      `);
      accountsWithIds = accountsResult.rows.map(r => ({
        id: r[0] as string,
        name: r[1] as string,
      }));
    } catch (e) {
      console.error("Failed to load accounts:", e);
    }
  }

  // All known tags for autocomplete
  let allTags = $state<string[]>([]);

  // Autocomplete suggestions for custom tag input (legacy, sidebar)
  let tagAutocompleteSuggestions = $derived.by(() => {
    if (!customTagInput || allTags.length === 0) return [];

    // Get the partial tag being typed (after last comma)
    const parts = customTagInput.split(",");
    const partial = parts[parts.length - 1].trim().toLowerCase();
    if (!partial || partial.length < 1) return [];

    // Find matching tags (contains match, not just prefix)
    return allTags
      .filter(tag => tag.toLowerCase().includes(partial) && tag.toLowerCase() !== partial)
      .slice(0, 5);
  });

  // For Tab completion, get the first suggestion's remaining text
  let tagAutocomplete = $derived.by(() => {
    if (tagAutocompleteSuggestions.length === 0) return "";
    const parts = customTagInput.split(",");
    const partial = parts[parts.length - 1].trim().toLowerCase();
    const firstMatch = tagAutocompleteSuggestions[0];
    if (firstMatch.toLowerCase().startsWith(partial)) {
      return firstMatch.slice(partial.length);
    }
    return "";
  });

  // Reset autocomplete selection when input changes
  let lastAutocompleteInput = "";
  $effect(() => {
    if (customTagInput !== lastAutocompleteInput) {
      lastAutocompleteInput = customTagInput;
      autocompleteIndex = -1;
    }
  });

  function selectAutocompleteTag(tag: string) {
    // Replace the partial tag being typed with the selected tag
    const parts = customTagInput.split(",");
    parts[parts.length - 1] = tag;
    customTagInput = parts.join(", ");
    autocompleteIndex = -1;
    customTagInputEl?.focus();
  }

  function buildQuery(offset: number = 0): { sql: string; params: QueryParam[] } {
    let query = `
      SELECT
        t.transaction_id,
        t.account_id,
        t.transaction_date,
        t.description,
        t.amount,
        t.tags,
        t.account_name,
        t.parent_transaction_id,
        a.nickname as account_nickname,
        t.tags_auto_applied
      FROM transactions t
      LEFT JOIN sys_accounts a ON t.account_id = a.account_id
    `;

    const conditions: string[] = [];
    const params: QueryParam[] = [];

    // Account filter (multi-select)
    if (selectedAccounts.size > 0) {
      const accounts = Array.from(selectedAccounts);
      const placeholders = accounts.map(() => '?').join(', ');
      conditions.push(`t.account_name IN (${placeholders})`);
      params.push(...accounts);
    }

    // Untagged filter - works alongside search
    // Note: Using string cast comparison instead of `tags = []` which can trigger DuckDB bugs
    if (filterMode === "untagged") {
      conditions.push("CAST(t.tags AS VARCHAR) = '[]'");
    }

    // Search filter
    if (searchQuery.trim()) {
      const searchPattern = `%${searchQuery.trim()}%`;
      // Search across description, account_name, amount (as string), and tags
      conditions.push(`(
        t.description ILIKE ?
        OR t.account_name ILIKE ?
        OR CAST(t.amount AS VARCHAR) LIKE ?
        OR array_to_string(t.tags, ',') ILIKE ?
      )`);
      params.push(searchPattern, searchPattern, searchPattern, searchPattern);
    }

    if (conditions.length > 0) {
      query += " WHERE " + conditions.join(" AND ");
    }

    query += ` ORDER BY t.transaction_date DESC LIMIT ${PAGE_SIZE + 1} OFFSET ${offset}`;
    return { sql: query, params };
  }

  function parseRows(rows: unknown[][]): Transaction[] {
    return rows.map(row => ({
      transaction_id: row[0] as string,
      account_id: row[1] as string,
      transaction_date: row[2] as string,
      description: row[3] as string,
      amount: row[4] as number,
      tags: (row[5] as string[]) || [],
      account_name: row[6] as string,
      parent_transaction_id: row[7] as string | null,
      account_nickname: row[8] as string | null,
      tags_auto_applied: row[9] as boolean | null ?? false,
    }));
  }

  async function loadTransactions() {
    isLoading = true;
    error = null;
    hasMore = true;

    try {
      const { sql, params } = buildQuery(0);
      const result = await executeQueryWithParams(sql, params);
      const rows = parseRows(result.rows);

      // Check if there are more results
      if (rows.length > PAGE_SIZE) {
        transactions = rows.slice(0, PAGE_SIZE);
        hasMore = true;
      } else {
        transactions = rows;
        hasMore = false;
      }

      // Reset cursor
      cursorIndex = 0;

      // Compute suggestions for all loaded transactions
      suggestions = await suggester.suggestBatch(transactions, 9);
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to load transactions";
      console.error("Failed to load transactions:", e);
    } finally {
      isLoading = false;
    }
  }

  async function loadMoreTransactions() {
    if (!hasMore || isLoadingMore) return;

    isLoadingMore = true;

    try {
      const { sql, params } = buildQuery(transactions.length);
      const result = await executeQueryWithParams(sql, params);
      const rows = parseRows(result.rows);

      if (rows.length > PAGE_SIZE) {
        transactions = [...transactions, ...rows.slice(0, PAGE_SIZE)];
        hasMore = true;
      } else {
        transactions = [...transactions, ...rows];
        hasMore = false;
      }

      // Compute suggestions for new transactions
      const newSuggestions = await suggester.suggestBatch(rows.slice(0, PAGE_SIZE), 9);
      suggestions = new Map([...suggestions, ...newSuggestions]);
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to load more";
      console.error("Failed to load more transactions:", e);
    } finally {
      isLoadingMore = false;
    }
  }

  function handleSearchInput() {
    // Debounced live search - longer delay so typing feels instant
    if (searchDebounceTimer) {
      clearTimeout(searchDebounceTimer);
    }
    searchDebounceTimer = setTimeout(() => {
      loadTransactions();
    }, 400);
  }

  function handleSearchKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      // Immediately execute search and exit search mode
      if (searchDebounceTimer) {
        clearTimeout(searchDebounceTimer);
      }
      loadTransactions();
      exitSearch();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      exitSearch();
    }
    // Let all other keys flow through naturally
  }

  function handleKeyDown(e: KeyboardEvent) {
    // Search mode - handled separately (check actual focus, not flag)
    if (isSearchFocused()) {
      return;
    }

    // Inline tag editing mode - handled by the input itself
    if (inlineEditingIndex !== null) {
      return;
    }

    // Custom tag input mode (legacy sidebar)
    if (isCustomTagging) {
      handleCustomTagKeyDown(e);
      return;
    }

    // Account dropdown open - handle navigation
    if (isAccountDropdownOpen) {
      handleAccountFilterKeyDown(e);
      return;
    }

    // Ctrl+Z / Cmd+Z to undo
    if ((e.ctrlKey || e.metaKey) && e.key === "z") {
      e.preventDefault();
      performUndo();
      return;
    }

    // Cmd+F / Ctrl+F to focus search
    if ((e.ctrlKey || e.metaKey) && e.key === "f") {
      e.preventDefault();
      startSearch();
      return;
    }

    // Number keys 1-9 to apply suggested tags
    if (e.key >= "1" && e.key <= "9") {
      const tagIndex = parseInt(e.key) - 1;
      if (tagIndex < currentSuggestions.length) {
        e.preventDefault();
        applyTagToCurrentOrSelected(currentSuggestions[tagIndex].tag);
      }
      return;
    }

    // Normal navigation mode
    switch(e.key) {
      case "j":
      case "ArrowDown":
        e.preventDefault();
        moveCursor(1);
        break;
      case "k":
      case "ArrowUp":
        e.preventDefault();
        moveCursor(-1);
        break;
      case " ":
        e.preventDefault();
        toggleSelection();
        break;
      case "x":
        e.preventDefault();
        toggleSelectionNoMove();
        break;
      case "a":
      case "A":
        e.preventDefault();
        toggleSelectAll();
        break;
      case "Escape":
        e.preventDefault();
        deselectAll();
        break;
      case "t":
        e.preventDefault();
        startInlineEditing();
        break;
      case "Enter":
      case "e":
        e.preventDefault();
        if (transactions[cursorIndex]) {
          openTagModal(transactions[cursorIndex]);
        }
        break;
      case "d":
        e.preventDefault();
        if (transactions[cursorIndex]) {
          deleteCurrentTransaction();
        }
        break;
      case "c":
        e.preventDefault();
        clearTagsFromCurrent();
        break;
      case "r":
        e.preventDefault();
        resetFilters();
        break;
      case "R":
        // Shift+R to create rule from current transaction
        e.preventDefault();
        if (transactions[cursorIndex]) {
          openRulesModalCreate([transactions[cursorIndex]]);
        }
        break;
      case "u":
        e.preventDefault();
        toggleFilterMode();
        break;
      case "f":
        e.preventDefault();
        startAccountFiltering();
        break;
      case "/":
        e.preventDefault();
        startSearch();
        break;
      case "g":
        e.preventDefault();
        cursorIndex = 0;
        scrollToCursor();
        break;
      case "G":
        e.preventDefault();
        cursorIndex = transactions.length - 1;
        scrollToCursor();
        break;
      case "n":
        e.preventDefault();
        skipToNextUntagged();
        break;
      case "[":
        e.preventDefault();
        pageUp();
        break;
      case "]":
        e.preventDefault();
        pageDown();
        break;
      case "+":
      case "=":
        e.preventDefault();
        openAddModal();
        break;
    }
  }

  function handleCustomTagKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      cancelCustomTagging();
      return;
    }

    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      // If an autocomplete item is selected, use it
      if (autocompleteIndex >= 0 && autocompleteIndex < tagAutocompleteSuggestions.length) {
        selectAutocompleteTag(tagAutocompleteSuggestions[autocompleteIndex]);
        autocompleteIndex = -1;
      } else {
        applyCustomTag();
      }
      return;
    }

    if (e.key === "Tab") {
      e.preventDefault();
      // Apply autocomplete suggestion
      if (tagAutocomplete) {
        customTagInput += tagAutocomplete;
        autocompleteIndex = -1;
      }
      return;
    }

    if (e.key === "ArrowDown") {
      if (tagAutocompleteSuggestions.length > 0) {
        e.preventDefault();
        e.stopPropagation();
        autocompleteIndex = Math.min(autocompleteIndex + 1, tagAutocompleteSuggestions.length - 1);
      }
      return;
    }

    if (e.key === "ArrowUp") {
      if (tagAutocompleteSuggestions.length > 0) {
        e.preventDefault();
        e.stopPropagation();
        autocompleteIndex = Math.max(autocompleteIndex - 1, -1);
      }
      return;
    }
  }

  function handleAccountFilterKeyDown(e: KeyboardEvent) {
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        closeAccountDropdown();
        break;
      case "j":
      case "ArrowDown":
        e.preventDefault();
        accountCursorIndex = Math.min(accountCursorIndex + 1, availableAccounts.length - 1);
        scrollAccountCursor();
        break;
      case "k":
      case "ArrowUp":
        e.preventDefault();
        accountCursorIndex = Math.max(accountCursorIndex - 1, 0);
        scrollAccountCursor();
        break;
      case " ":
      case "x":
        e.preventDefault();
        toggleAccountSelection(availableAccounts[accountCursorIndex]);
        break;
      case "Enter":
        e.preventDefault();
        applyAccountFilterAndClose();
        break;
      case "a":
        // Select all
        e.preventDefault();
        selectedAccounts = new Set(availableAccounts);
        break;
      case "A":
        // Deselect all
        e.preventDefault();
        selectedAccounts = new Set();
        break;
    }
  }

  function scrollAccountCursor() {
    setTimeout(() => {
      const element = document.querySelector(`[data-account-index="${accountCursorIndex}"]`);
      if (element) {
        element.scrollIntoView({ block: "nearest", behavior: "auto" });
      }
    }, 0);
  }

  function startAccountFiltering() {
    if (availableAccounts.length === 0) return;
    isAccountDropdownOpen = true;
    accountCursorIndex = 0;
  }

  function closeAccountDropdown() {
    isAccountDropdownOpen = false;
    containerEl?.focus();
  }

  function toggleAccountSelection(account: string) {
    if (!account) return;
    if (selectedAccounts.has(account)) {
      selectedAccounts.delete(account);
    } else {
      selectedAccounts.add(account);
    }
    selectedAccounts = new Set(selectedAccounts); // trigger reactivity
  }

  function applyAccountFilterAndClose() {
    isAccountDropdownOpen = false;
    persistSelectedAccounts();
    loadTransactions();
    containerEl?.focus();
  }

  function clearAccountFilter() {
    selectedAccounts = new Set();
    persistSelectedAccounts();
    loadTransactions();
  }

  function toggleAccountDropdown() {
    if (isAccountDropdownOpen) {
      closeAccountDropdown();
    } else {
      startAccountFiltering();
    }
  }

  function startSearch() {
    searchInputEl?.focus();
  }

  function exitSearch() {
    if (searchDebounceTimer) {
      clearTimeout(searchDebounceTimer);
    }
    containerEl?.focus();
  }

  function clearSearch() {
    searchQuery = "";
    loadTransactions();
    containerEl?.focus();
  }

  function toggleFilterMode() {
    filterMode = filterMode === "all" ? "untagged" : "all";
    loadTransactions();
  }

  function resetFilters() {
    searchQuery = "";
    filterMode = "all";
    selectedAccounts = new Set();
    persistSelectedAccounts();
    loadTransactions();
  }

  function moveCursor(delta: number) {
    const newIndex = cursorIndex + delta;
    if (newIndex >= 0 && newIndex < transactions.length) {
      cursorIndex = newIndex;
      scrollToCursor();

      // Auto-load more when near the end (within 20 items)
      if (hasMore && !isLoadingMore && cursorIndex >= transactions.length - 20) {
        loadMoreTransactions();
      }
    }
  }

  function pageUp() {
    cursorIndex = Math.max(0, cursorIndex - 20);
    scrollToCursor();
  }

  function pageDown() {
    cursorIndex = Math.min(transactions.length - 1, cursorIndex + 20);
    scrollToCursor();

    // Auto-load more when near the end
    if (hasMore && !isLoadingMore && cursorIndex >= transactions.length - 20) {
      loadMoreTransactions();
    }
  }

  function skipToNextUntagged() {
    for (let i = cursorIndex + 1; i < transactions.length; i++) {
      if (transactions[i].tags.length === 0) {
        cursorIndex = i;
        scrollToCursor();
        return;
      }
    }
    for (let i = 0; i < cursorIndex; i++) {
      if (transactions[i].tags.length === 0) {
        cursorIndex = i;
        scrollToCursor();
        return;
      }
    }
  }

  function scrollToCursor() {
    setTimeout(() => {
      const element = document.querySelector(`[data-index="${cursorIndex}"]`);
      if (element) {
        element.scrollIntoView({ block: "nearest", behavior: "auto" });
      }
    }, 0);
  }

  function handleRowClick(index: number) {
    cursorIndex = index;
    containerEl?.focus();
  }

  function handleRowDoubleClick(index: number) {
    const txn = transactions[index];
    if (txn) {
      openTagModal(txn);
    }
  }

  function handleListScroll(e: Event) {
    const target = e.target as HTMLElement;
    const scrollBottom = target.scrollHeight - target.scrollTop - target.clientHeight;

    // Load more when within 100px of the bottom
    if (scrollBottom < 100 && hasMore && !isLoadingMore) {
      loadMoreTransactions();
    }
  }

  function toggleSelection() {
    toggleSelectionNoMove();
    moveCursor(1);
  }

  function toggleSelectionNoMove() {
    if (selectedIndices.has(cursorIndex)) {
      selectedIndices.delete(cursorIndex);
    } else {
      selectedIndices.add(cursorIndex);
    }
    selectedIndices = new Set(selectedIndices);
  }

  function toggleSelectionAt(index: number) {
    if (selectedIndices.has(index)) {
      selectedIndices.delete(index);
    } else {
      selectedIndices.add(index);
    }
    selectedIndices = new Set(selectedIndices);
  }

  function selectAll() {
    selectedIndices = new Set(transactions.map((_, i) => i));
  }

  function deselectAll() {
    selectedIndices = new Set();
  }

  function getSelectedTransactions(): Transaction[] {
    if (selectedIndices.size === 0) {
      const current = transactions[cursorIndex];
      return current ? [current] : [];
    }
    return Array.from(selectedIndices).map(i => transactions[i]).filter(Boolean);
  }

  function getSelectedIndicesArray(): number[] {
    if (selectedIndices.size === 0) {
      return [cursorIndex];
    }
    return Array.from(selectedIndices);
  }

  // Optimistic update: apply tag locally then persist in background
  async function applyTagToCurrentOrSelected(tag: string) {
    const indices = getSelectedIndicesArray();
    if (indices.length === 0) return;

    // Record undo entries before making changes
    const undoEntries: UndoEntry[] = [];
    const taggedTransactions: Transaction[] = [];

    // Optimistic update - update local state immediately
    for (const idx of indices) {
      const txn = transactions[idx];
      if (!txn) continue;
      const currentTags = txn.tags || [];
      if (currentTags.includes(tag)) continue;

      // Record for undo
      const newTags = [...currentTags, tag];
      undoEntries.push({
        transactionId: txn.transaction_id,
        previousTags: [...currentTags],
        newTags: newTags
      });

      // Update in place
      transactions[idx] = {
        ...txn,
        tags: newTags
      };
      taggedTransactions.push(transactions[idx]);
    }

    // Push to undo stack
    pushUndo(undoEntries);

    // Force reactivity
    transactions = [...transactions];

    // Don't auto-advance cursor or deselect - let users apply multiple tags or clear+tag
    // They can press Esc to deselect or j/k to move when ready

    // Persist and refresh suggestions
    await persistTagChanges(indices.map(i => transactions[i]));

    // Prompt to save as rule if enough transactions were tagged
    if (taggedTransactions.length >= RULE_PROMPT_THRESHOLD) {
      promptSaveAsRule(taggedTransactions, [tag]);
    }
  }

  // Prompt user to save tagging action as a rule (non-intrusive toast)
  function promptSaveAsRule(txns: Transaction[], tags: string[]) {
    // Show a subtle toast with action instead of interrupting with a modal
    showToast({
      type: "info",
      title: `Tagged ${txns.length} transactions`,
      message: "Create a rule to auto-tag similar transactions?",
      duration: 8000,
      action: {
        label: "Save as Rule",
        shortcut: "cmd+s",
        onClick: () => {
          openRulesModalCreate(txns, tags);
        },
      },
    });
  }

  async function handleRuleSaved(info: { appliedToExisting: boolean; appliedCount?: number }) {
    if (info.appliedToExisting && info.appliedCount && info.appliedCount > 0) {
      showToast({ type: "success", title: "Rule saved!", message: `Tags applied to ${info.appliedCount} transaction${info.appliedCount === 1 ? '' : 's'}.` });
    } else {
      showToast({ type: "success", title: "Rule saved!" });
    }
    // Clear selection after rule is created
    deselectAll();
    // Refresh transactions and global stats to show updated tags
    await loadTransactions();
    await loadGlobalStats();
    // Notify sidebar badge of the change
    registry.emit("data:refresh");
  }

  function openRulesModal() {
    rulesModalMode = "list";
    rulesModalPrefillTransactions = [];
    rulesModalPrefillTags = [];
    showRulesModal = true;
  }

  function openRulesModalCreate(txns: Transaction[], tags: string[] = []) {
    rulesModalMode = "create";
    rulesModalPrefillTransactions = txns;
    rulesModalPrefillTags = tags;
    showRulesModal = true;
  }

  function closeRulesModal() {
    showRulesModal = false;
    rulesModalMode = "list";
    rulesModalPrefillTransactions = [];
    rulesModalPrefillTags = [];
    containerEl?.focus();
  }

  async function handleSavePinnedTags(tags: string[]) {
    pinnedQuickTags = tags;
    await updatePluginSettings("tagging", { pinnedQuickTags: tags });
    showToast({ type: "success", title: "Pinned tags saved" });
  }

  async function persistTagChanges(txns: Transaction[], tagAdded: boolean = true) {
    if (txns.length === 0) return;

    try {
      // Use JS-computed timestamp to avoid ICU extension dependency
      const now = new Date().toISOString();

      // Escape a string for SQL (double up single quotes)
      const escapeSql = (s: string) => s.replace(/'/g, "''");

      // Build a single bulk UPDATE using UPDATE...FROM with VALUES
      // This is much faster than one query per transaction
      const valueRows = txns.map(txn => {
        const tagList = txn.tags.length > 0
          ? `list_value(${txn.tags.map(t => `'${escapeSql(t)}'`).join(', ')})`
          : `[]::VARCHAR[]`;
        return `('${escapeSql(txn.transaction_id)}', ${tagList})`;
      }).join(',\n        ');

      const ids = txns.map(t => `'${escapeSql(t.transaction_id)}'`).join(', ');

      await executeQueryWithParams(
        `UPDATE sys_transactions AS t
         SET tags = v.new_tags,
             tags_auto_applied = FALSE,
             updated_at = ?::TIMESTAMP
         FROM (VALUES
           ${valueRows}
         ) AS v(transaction_id, new_tags)
         WHERE t.transaction_id = v.transaction_id
           AND t.transaction_id IN (${ids})`,
        [now],
        { readonly: false }
      );

      // Update local state to clear auto-applied flag (manual tagging overrides)
      const txnIds = new Set(txns.map(t => t.transaction_id));
      transactions = transactions.map(t =>
        txnIds.has(t.transaction_id) ? { ...t, tags_auto_applied: false } : t
      );

      // Refresh global stats after persisting
      await loadGlobalStats();

      // Notify other components (e.g., sidebar badge) of the change
      // Set flag so we don't reload ourselves
      isInternalUpdate = true;
      registry.emit("data:refresh");
      isInternalUpdate = false;

      // Update hasAnyTags flag if we just added tags
      if (tagAdded && txns.some(t => t.tags.length > 0)) {
        hasAnyTags = true;
      }

      // Refresh suggestions so newly added tags appear in quick tags
      await suggester.refresh();
      allTags = suggester.getAllTags();
      if (transactions.length > 0) {
        suggestions = await suggester.suggestBatch(transactions, 9);
      }
    } catch (e) {
      console.error("Failed to persist tags:", e);
      // Could show a toast/notification here
      error = "Failed to save - changes may be lost";
    }
  }

  // Push a batch of tag changes to the undo stack
  function pushUndo(entries: UndoEntry[]) {
    if (entries.length === 0) return;
    undoStack = [...undoStack.slice(-(MAX_UNDO_DEPTH - 1)), entries];
  }

  // Undo the last tag operation
  async function performUndo() {
    if (undoStack.length === 0) {
      showToast({ type: "info", title: "Nothing to undo" });
      return;
    }

    const entries = undoStack[undoStack.length - 1];
    undoStack = undoStack.slice(0, -1);

    // Apply reverts locally
    for (const entry of entries) {
      const idx = transactions.findIndex(t => t.transaction_id === entry.transactionId);
      if (idx >= 0) {
        transactions[idx] = {
          ...transactions[idx],
          tags: [...entry.previousTags]
        };
      }
    }

    transactions = [...transactions];

    // Persist the reverted tags
    const txnsToUpdate = entries
      .map(e => transactions.find(t => t.transaction_id === e.transactionId))
      .filter((t): t is Transaction => t !== undefined);

    await persistTagChanges(txnsToUpdate);
    showToast({ type: "success", title: `Undid ${entries.length} tag change${entries.length > 1 ? 's' : ''}` });
  }

  function startCustomTagging() {
    if (transactions.length === 0) return;
    isCustomTagging = true;
    // Pre-populate with existing tags
    const txn = transactions[cursorIndex];
    if (txn && txn.tags.length > 0) {
      customTagInput = txn.tags.join(", ");
    } else {
      customTagInput = "";
    }
    setTimeout(() => customTagInputEl?.focus(), 10);
  }

  function cancelCustomTagging() {
    isCustomTagging = false;
    customTagInput = "";
    containerEl?.focus();
  }

  async function applyCustomTag() {
    const tags = customTagInput.split(",").map(t => t.trim()).filter(t => t);
    if (tags.length === 0) {
      cancelCustomTagging();
      return;
    }

    const indices = getSelectedIndicesArray();

    // Record undo entries before making changes
    const undoEntries: UndoEntry[] = [];
    const taggedTransactions: Transaction[] = [];

    // Optimistic update
    for (const idx of indices) {
      const txn = transactions[idx];
      if (!txn) continue;

      // Merge tags (dedupe)
      const currentTags = txn.tags || [];
      const mergedTags = [...new Set([...currentTags, ...tags])];

      // Only count as tagged if new tags were actually added
      const addedNewTags = mergedTags.length > currentTags.length;

      // Record for undo
      undoEntries.push({
        transactionId: txn.transaction_id,
        previousTags: [...currentTags],
        newTags: mergedTags
      });

      transactions[idx] = {
        ...txn,
        tags: mergedTags
      };

      if (addedNewTags) {
        taggedTransactions.push(transactions[idx]);
      }
    }

    // Push to undo stack
    pushUndo(undoEntries);

    transactions = [...transactions];

    // Don't auto-advance or deselect - let users apply multiple operations
    cancelCustomTagging();

    // Persist and refresh suggestions
    await persistTagChanges(indices.map(i => transactions[i]));

    // Prompt to save as rule if enough transactions were tagged
    if (taggedTransactions.length >= RULE_PROMPT_THRESHOLD) {
      promptSaveAsRule(taggedTransactions, tags);
    }
  }

  // Inline tag editing functions
  function startInlineEditing(index?: number) {
    // If there are multi-selected transactions, use the first selected one
    // This ensures the input appears on a selected row, not the cursor row
    let targetIndex: number;
    if (index !== undefined) {
      targetIndex = index;
    } else if (selectedIndices.size > 0) {
      // Use the first selected transaction (sorted to get lowest index)
      targetIndex = Math.min(...Array.from(selectedIndices));
    } else {
      targetIndex = cursorIndex;
    }

    if (transactions.length === 0 || targetIndex < 0 || targetIndex >= transactions.length) return;

    // Move cursor to the editing row for visual consistency
    cursorIndex = targetIndex;
    inlineEditingIndex = targetIndex;
    const txn = transactions[targetIndex];
    // Pre-populate with existing tags only for single transaction
    // For multi-select, start blank (we'll ADD tags, not replace)
    if (selectedIndices.size > 1) {
      inlineTagInput = "";
    } else if (txn && txn.tags.length > 0) {
      inlineTagInput = txn.tags.join(", ");
    } else {
      inlineTagInput = "";
    }
    setTimeout(() => inlineTagInputRef?.focus(), 10);
  }

  function cancelInlineEditing() {
    inlineEditingIndex = null;
    inlineTagInput = "";
    containerEl?.focus();
  }

  async function applyInlineTag(overrideTags?: string[]) {
    if (inlineEditingIndex === null) return;

    const tags = overrideTags ?? inlineTagInput.split(",").map(t => t.trim()).filter(t => t);
    const txn = transactions[inlineEditingIndex];
    if (!txn) {
      cancelInlineEditing();
      return;
    }

    // Get indices to apply to (current row + any selected)
    const indices = selectedIndices.size > 0 && selectedIndices.has(inlineEditingIndex)
      ? Array.from(selectedIndices)
      : [inlineEditingIndex];

    // Record undo entries before making changes
    const undoEntries: UndoEntry[] = [];
    const taggedTransactions: Transaction[] = [];
    const isMultiSelect = indices.length > 1;

    // For single transaction: REPLACE tags (user typed the full list)
    // For multi-select: ADD/MERGE tags (preserve each transaction's existing tags)
    for (const idx of indices) {
      const t = transactions[idx];
      if (!t) continue;

      const currentTags = t.tags || [];
      // For multi-select, merge new tags with existing; for single, replace entirely
      const newTags = isMultiSelect
        ? [...new Set([...currentTags, ...tags])] // Merge and dedupe
        : tags;

      // Only count as changed if tags are different
      const tagsChanged = JSON.stringify(currentTags.sort()) !== JSON.stringify(newTags.sort());

      if (tagsChanged) {
        undoEntries.push({
          transactionId: t.transaction_id,
          previousTags: [...currentTags],
          newTags: newTags
        });

        transactions[idx] = { ...t, tags: newTags };
        taggedTransactions.push(transactions[idx]);
      }
    }

    // Push to undo stack
    if (undoEntries.length > 0) {
      pushUndo(undoEntries);
      transactions = [...transactions];

      // Close editor immediately (optimistic), persist in background
      cancelInlineEditing();

      // Persist to DB in background - don't await
      persistTagChanges(indices.map(i => transactions[i])).then(() => {
        // Prompt to save as rule if enough transactions were tagged
        if (taggedTransactions.length >= RULE_PROMPT_THRESHOLD) {
          promptSaveAsRule(taggedTransactions, tags);
        }
      });
    } else {
      cancelInlineEditing();
    }
  }

  function handleInlineSubmit() {
    const finalTags = inlineTagInput.split(",").map(t => t.trim()).filter(t => t);
    applyInlineTag(finalTags);
  }

  // Clear all tags (set to empty)
  async function clearTagsFromCurrent() {
    const indices = getSelectedIndicesArray();
    if (indices.length === 0) return;

    // Record undo entries before making changes
    const undoEntries: UndoEntry[] = [];

    // Optimistic update
    for (const idx of indices) {
      const txn = transactions[idx];
      if (!txn || txn.tags.length === 0) continue;

      // Record for undo
      undoEntries.push({
        transactionId: txn.transaction_id,
        previousTags: [...txn.tags],
        newTags: []
      });

      transactions[idx] = {
        ...txn,
        tags: []
      };
    }

    // Push to undo stack
    pushUndo(undoEntries);

    transactions = [...transactions];

    // Don't auto-advance or deselect - let users clear then tag

    // Persist and refresh suggestions
    await persistTagChanges(indices.map(i => transactions[i]));
  }

  // Remove tags (same as clear for now)
  async function removeTagsFromCurrent() {
    await clearTagsFromCurrent();
  }

  // Remove a single tag from current/selected transactions
  async function removeSingleTag(tagToRemove: string) {
    const indices = getSelectedIndicesArray();
    if (indices.length === 0) return;

    const undoEntries: UndoEntry[] = [];

    for (const idx of indices) {
      const txn = transactions[idx];
      if (!txn || !txn.tags.includes(tagToRemove)) continue;

      const newTags = txn.tags.filter(t => t !== tagToRemove);

      undoEntries.push({
        transactionId: txn.transaction_id,
        previousTags: [...txn.tags],
        newTags
      });

      transactions[idx] = { ...txn, tags: newTags };
    }

    if (undoEntries.length === 0) return;

    pushUndo(undoEntries);
    transactions = [...transactions];
    await persistTagChanges(indices.map(i => transactions[i]));
  }

  // Transaction edit modal functions
  function openTagModal(txn: Transaction) {
    editingTransaction = txn;
    isTagModalOpen = true;
  }

  function closeTagModal() {
    isTagModalOpen = false;
    editingTransaction = null;
    containerEl?.focus();
  }

  // Row context menu functions
  function openContextMenu(txn: Transaction, e: MouseEvent) {
    e.stopPropagation();
    contextMenuTxn = txn;
  }

  function closeContextMenu() {
    contextMenuTxn = null;
  }

  function handleContextEdit() {
    if (contextMenuTxn) {
      openTagModal(contextMenuTxn);
    }
    closeContextMenu();
  }

  function handleContextDelete() {
    if (contextMenuTxn) {
      editingTransaction = contextMenuTxn;
      showDeleteConfirm = true;
    }
    closeContextMenu();
  }

  function handleContextSplit() {
    if (contextMenuTxn) {
      // Check if it's already a split child - can't split a split
      if (contextMenuTxn.parent_transaction_id) {
        showToast({ type: "error", title: "Cannot split", message: "This transaction is already part of a split" });
        closeContextMenu();
        return;
      }
      editingTransaction = contextMenuTxn;
      openSplitModal();
    }
    closeContextMenu();
  }

  function handleContextUnsplit() {
    if (contextMenuTxn && contextMenuTxn.parent_transaction_id) {
      editingTransaction = contextMenuTxn;
      showUnsplitConfirm = true;
    }
    closeContextMenu();
  }

  function handleContextCreateRule() {
    if (contextMenuTxn) {
      openRulesModalCreate([contextMenuTxn]);
    }
    closeContextMenu();
  }

  async function handleEditSave(data: {
    description: string;
    amount: number;
    date: string;
    tags: string[];
  }) {
    if (!editingTransaction) return;

    // Find the transaction in our list and update it
    const idx = transactions.findIndex(t => t.transaction_id === editingTransaction!.transaction_id);
    if (idx >= 0) {
      transactions[idx] = {
        ...transactions[idx],
        tags: data.tags,
        description: data.description,
        amount: data.amount,
        transaction_date: data.date
      };
      transactions = [...transactions];

      // Persist all changes
      await persistTransactionChanges(transactions[idx]);
    }

    closeTagModal();
  }

  async function persistTransactionChanges(txn: Transaction) {
    try {
      // Build parameterized tag list
      const tagListSql = txn.tags.length > 0
        ? `list_value(${txn.tags.map(() => '?').join(', ')})`
        : `[]::VARCHAR[]`;

      await executeQueryWithParams(
        `UPDATE sys_transactions SET
          tags = ${tagListSql},
          description = ?,
          amount = ?,
          transaction_date = ?
        WHERE transaction_id = ?`,
        [...txn.tags, txn.description, txn.amount, txn.transaction_date, txn.transaction_id],
        { readonly: false }
      );

      // Refresh global stats after persisting
      await loadGlobalStats();
    } catch (e) {
      console.error("Failed to persist transaction:", e);
      error = "Failed to save - changes may be lost";
    }
  }

  // Delete current transaction (keyboard shortcut)
  function deleteCurrentTransaction() {
    const txn = transactions[cursorIndex];
    if (!txn) return;
    // Open modal and show delete confirmation
    openTagModal(txn);
    showDeleteConfirm = true;
  }

  // Delete transaction
  async function deleteTransaction() {
    if (!editingTransaction) return;

    try {
      const now = new Date().toISOString();
      await executeQueryWithParams(
        `UPDATE sys_transactions SET deleted_at = ? WHERE transaction_id = ?`,
        [now, editingTransaction.transaction_id],
        { readonly: false }
      );

      // Remove from local list
      transactions = transactions.filter(
        (t) => t.transaction_id !== editingTransaction!.transaction_id
      );

      showDeleteConfirm = false;
      closeTagModal();
      await loadGlobalStats();

      // Notify sidebar badge of the change
      registry.emit("data:refresh");
    } catch (e) {
      console.error("Failed to delete transaction:", e);
      error = e instanceof Error ? e.message : "Failed to delete transaction";
    }
  }

  // Bulk delete selected transactions
  function promptBulkDelete() {
    if (selectedIndices.size === 0) return;
    bulkDeleteCount = selectedIndices.size;
    showBulkDeleteConfirm = true;
  }

  async function bulkDeleteTransactions() {
    const indices = Array.from(selectedIndices);
    const txnsToDelete = indices.map(i => transactions[i]).filter(Boolean);
    if (txnsToDelete.length === 0) return;

    try {
      const now = new Date().toISOString();
      for (const txn of txnsToDelete) {
        await executeQueryWithParams(
          `UPDATE sys_transactions SET deleted_at = ? WHERE transaction_id = ?`,
          [now, txn.transaction_id],
          { readonly: false }
        );
      }

      // Remove from local list
      const idsToDelete = new Set(txnsToDelete.map(t => t.transaction_id));
      transactions = transactions.filter(t => !idsToDelete.has(t.transaction_id));

      // Clear selection
      selectedIndices = new Set();
      showBulkDeleteConfirm = false;
      containerEl?.focus();
      await loadGlobalStats();

      // Notify sidebar badge of the change
      registry.emit("data:refresh");
    } catch (e) {
      console.error("Failed to delete transactions:", e);
      error = e instanceof Error ? e.message : "Failed to delete transactions";
    }
  }

  // Split transaction
  function openSplitModal() {
    if (!editingTransaction) return;
    showSplitModal = true;
  }

  function closeSplitModal() {
    showSplitModal = false;
    containerEl?.focus();
  }

  function generateUUID(): string {
    return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
      const r = (Math.random() * 16) | 0;
      const v = c === "x" ? r : (r & 0x3) | 0x8;
      return v.toString(16);
    });
  }

  async function handleSplit(splitAmounts: SplitAmount[]) {
    if (!editingTransaction) return;

    try {
      const now = new Date().toISOString();
      const parentId = editingTransaction.transaction_id;

      // Soft-delete the parent transaction
      await executeQueryWithParams(
        `UPDATE sys_transactions SET deleted_at = ? WHERE transaction_id = ?`,
        [now, parentId],
        { readonly: false }
      );

      // Insert child transactions
      const newTransactions: Transaction[] = [];
      for (const split of splitAmounts) {
        const amt = parseFloat(split.amount);
        if (isNaN(amt) || amt === 0) continue;

        const childId = generateUUID();

        // Build parameterized tag list
        const tagListSql = editingTransaction.tags.length > 0
          ? `list_value(${editingTransaction.tags.map(() => '?').join(', ')})`
          : `NULL`;

        const params = [
          childId,
          editingTransaction.account_id,
          amt,
          split.description,
          editingTransaction.transaction_date,
          editingTransaction.transaction_date,
          ...editingTransaction.tags,
          parentId,
          now,
          now
        ];

        await executeQueryWithParams(
          `INSERT INTO sys_transactions (
            transaction_id, account_id, amount, description,
            transaction_date, posted_date, tags, external_ids,
            parent_transaction_id, created_at, updated_at
          ) VALUES (
            ?, ?, ?, ?, ?, ?,
            ${tagListSql},
            '{}', ?, ?, ?
          )`,
          params,
          { readonly: false }
        );

        // Add to local list for display
        newTransactions.push({
          transaction_id: childId,
          account_id: editingTransaction.account_id,
          account_name: editingTransaction.account_name,
          amount: amt,
          description: split.description,
          transaction_date: editingTransaction.transaction_date,
          tags: [...editingTransaction.tags],
          parent_transaction_id: parentId,
        });
      }

      // Replace parent with children in local list
      const parentIdx = transactions.findIndex(t => t.transaction_id === parentId);
      if (parentIdx >= 0) {
        transactions.splice(parentIdx, 1, ...newTransactions);
        transactions = [...transactions];
      }

      // Generate suggestions for the new split transactions
      const newSuggestions = await suggester.suggestBatch(newTransactions, 9);
      suggestions = new Map([...suggestions, ...newSuggestions]);

      closeSplitModal();
      closeTagModal();
      await loadGlobalStats();
    } catch (e) {
      console.error("Failed to split transaction:", e);
      error = e instanceof Error ? e.message : "Failed to split transaction";
    }
  }

  // Unsplit confirmation
  let showUnsplitConfirm = $state(false);

  // Add transaction modal
  let showAddModal = $state(false);

  async function unsplitTransaction() {
    if (!editingTransaction || !editingTransaction.parent_transaction_id) return;

    const parentId = editingTransaction.parent_transaction_id;

    try {
      const now = new Date().toISOString();

      // Delete all children of this parent (soft delete)
      await executeQueryWithParams(
        `UPDATE sys_transactions SET deleted_at = ? WHERE parent_transaction_id = ?`,
        [now, parentId],
        { readonly: false }
      );

      // Restore the parent (clear deleted_at)
      await executeQueryWithParams(
        `UPDATE sys_transactions SET deleted_at = NULL WHERE transaction_id = ?`,
        [parentId],
        { readonly: false }
      );

      // Reload transactions to get fresh data
      showUnsplitConfirm = false;
      closeTagModal();
      await loadTransactions();
      await loadGlobalStats();
    } catch (e) {
      console.error("Failed to unsplit transaction:", e);
      error = e instanceof Error ? e.message : "Failed to unsplit transaction";
    }
  }

  // Add transaction functions
  function openAddModal() {
    showAddModal = true;
  }

  function closeAddModal() {
    showAddModal = false;
    containerEl?.focus();
  }

  async function handleAddTransaction(data: {
    description: string;
    amount: number;
    date: string;
    accountId: string;
    tags: string[];
  }) {
    try {
      const now = new Date().toISOString();
      const txnId = generateUUID();

      // Find account name for the selected account
      const selectedAccount = accountsWithIds.find(a => a.id === data.accountId);
      const accountName = selectedAccount?.name || "";

      // Build parameterized tag list
      const tagListSql = data.tags.length > 0
        ? `list_value(${data.tags.map(() => '?').join(', ')})`
        : `NULL`;

      const params = [
        txnId,
        data.accountId,
        data.amount,
        data.description,
        data.date,
        data.date,
        ...data.tags,
        now,
        now
      ];

      await executeQueryWithParams(
        `INSERT INTO sys_transactions (
          transaction_id, account_id, amount, description,
          transaction_date, posted_date, tags, external_ids,
          is_manual, created_at, updated_at
        ) VALUES (
          ?, ?, ?, ?, ?, ?,
          ${tagListSql},
          '{}',
          TRUE,
          ?, ?
        )`,
        params,
        { readonly: false }
      );

      // Add to local list
      const newTxn: Transaction = {
        transaction_id: txnId,
        account_id: data.accountId,
        account_name: accountName,
        amount: data.amount,
        description: data.description,
        transaction_date: data.date,
        tags: data.tags,
        parent_transaction_id: null,
      };

      // Add to front since it's likely recent
      transactions = [newTxn, ...transactions];

      // Generate suggestions for the new transaction
      const newSuggestions = await suggester.suggestBatch([newTxn], 9);
      suggestions = new Map([...suggestions, ...newSuggestions]);

      closeAddModal();
      await loadGlobalStats();
    } catch (e) {
      console.error("Failed to add transaction:", e);
      error = e instanceof Error ? e.message : "Failed to add transaction";
    }
  }


  // Subscribe to global refresh events
  let unsubscribeRefresh: (() => void) | null = null;
  let unsubscribeTabActivated: (() => void) | null = null;

  async function reloadAll() {
    await suggester.loadTagData();
    allTags = suggester.getAllTags();
    await Promise.all([loadGlobalStats(), loadAvailableAccounts()]);
    await loadTransactions();
  }

  onMount(async () => {
    // Load plugin settings (pinned tags)
    const settings = await getPluginSettings<TaggingPluginSettings>("tagging", DEFAULT_TAGGING_SETTINGS);
    pinnedQuickTags = settings.pinnedQuickTags || [];

    // Preload tag data and accounts in parallel with loading transactions
    await suggester.loadTagData();
    allTags = suggester.getAllTags();
    hasAnyTags = allTags.length > 0;
    await Promise.all([loadGlobalStats(), loadAvailableAccounts()]);

    // If accountId prop was passed, filter to that account
    if (accountId) {
      const account = accountsWithIds.find(a => a.id === accountId);
      if (account) {
        selectedAccounts = new Set([account.name]);
        // Don't persist this - it's a temporary filter from navigation
      } else {
        // Account not found, use persisted filter
        selectedAccounts = loadPersistedAccounts();
      }
    } else {
      // Load persisted account filter
      selectedAccounts = loadPersistedAccounts();

      // Filter out any persisted accounts that no longer exist
      if (selectedAccounts.size > 0) {
        const validAccounts = new Set(
          Array.from(selectedAccounts).filter(a => availableAccounts.includes(a))
        );
        if (validAccounts.size !== selectedAccounts.size) {
          selectedAccounts = validAccounts;
          persistSelectedAccounts();
        }
      }
    }

    await loadTransactions();
    containerEl?.focus();

    // Listen for data refresh events (e.g., demo mode toggle, sync)
    // Skip reload if we triggered this ourselves (optimistic update already done)
    unsubscribeRefresh = registry.on("data:refresh", async () => {
      if (isInternalUpdate) return;
      await reloadAll();
    });

    // Listen for tab activation to restore focus when switching back to this tab
    unsubscribeTabActivated = registry.on("tab:activated", () => {
      // Only focus if this tab is the active one
      if (registry.activeTab?.viewId === "transactions") {
        containerEl?.focus();
      }
    });
  });

  onDestroy(() => {
    unsubscribeRefresh?.();
    unsubscribeTabActivated?.();
  });

  function getTargetCount(): number {
    return selectedIndices.size || 1;
  }

  let currentTxn = $derived(transactions[cursorIndex]);
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="tagging-view"
  bind:this={containerEl}
  tabindex="0"
  onkeydown={handleKeyDown}
  role="application"
>
  <!-- Confetti celebration -->
  {#if showCelebration}
    <div class="confetti-container">
      {#each Array(50) as _, i}
        <div class="confetti" style="--delay: {Math.random() * 3}s; --x: {Math.random() * 100}vw; --rotation: {Math.random() * 360}deg; --color: {['#6366f1', '#22c55e', '#f59e0b', '#ef4444', '#8b5cf6', '#06b6d4'][i % 6]}"></div>
      {/each}
    </div>
  {/if}

  <!-- Context menu backdrop -->
  {#if contextMenuTxn}
    <button class="context-menu-backdrop" onclick={closeContextMenu} aria-label="Close context menu"></button>
  {/if}

  <!-- Header -->
  <div class="header">
    <div class="title-row">
      <h1 class="title">Transactions</h1>

      <!-- Progress Ring with untagged count -->
      {#if globalStats.total > 0}
        <div class="progress-ring-container" class:complete={isAllTagged}>
          <svg class="progress-ring" viewBox="0 0 36 36">
            <path
              class="progress-ring-bg"
              d="M18 2.0845 a 15.9155 15.9155 0 0 1 0 31.831 a 15.9155 15.9155 0 0 1 0 -31.831"
            />
            <path
              class="progress-ring-fill"
              stroke-dasharray="{progressPercent}, 100"
              d="M18 2.0845 a 15.9155 15.9155 0 0 1 0 31.831 a 15.9155 15.9155 0 0 1 0 -31.831"
            />
          </svg>
          <span class="progress-text">{progressPercent}%</span>
        </div>
        {#if globalUntaggedCount > 0}
          <span class="untagged-badge">{globalUntaggedCount} left</span>
        {:else}
          <span class="complete-badge">All tagged!</span>
        {/if}
      {/if}
    </div>
  </div>

  <!-- Filter bar -->
  <div class="filter-bar">
    <!-- Search (fills space) -->
    <div class="search-container">
      <span class="search-icon">⌕</span>
      <input
        bind:this={searchInputEl}
        type="text"
        class="search-input"
        bind:value={searchQuery}
        oninput={handleSearchInput}
        onkeydown={handleSearchKeyDown}
        placeholder={`Search... (/ or ${modKey()}F)`}
      />
      {#if searchQuery}
        <button class="clear-search" onclick={clearSearch}>×</button>
      {/if}
    </div>

    <!-- Account filter dropdown -->
    <div class="account-filter-container">
      <button
        class="account-filter-btn"
        class:active={selectedAccounts.size > 0}
        onclick={toggleAccountDropdown}
      >
        {#if selectedAccounts.size === 0}
          All Accounts
        {:else if selectedAccounts.size === 1}
          {Array.from(selectedAccounts)[0]}
        {:else}
          {selectedAccounts.size} accounts
        {/if}
        <span class="dropdown-arrow">{isAccountDropdownOpen ? "▲" : "▼"}</span>
      </button>

      {#if isAccountDropdownOpen}
        <div class="account-dropdown">
          <div class="dropdown-header">
            <span class="dropdown-title">Filter by Account</span>
            {#if selectedAccounts.size > 0}
              <button class="clear-selection" onclick={clearAccountFilter}>Clear</button>
            {/if}
          </div>
          <div class="dropdown-hint">
            <kbd>j</kbd><kbd>k</kbd> navigate | <kbd>space</kbd> toggle | <kbd>Enter</kbd> apply
          </div>
          <div class="account-list">
            {#each availableAccounts as account, i}
              <button
                class="account-option"
                class:cursor={accountCursorIndex === i}
                class:selected={selectedAccounts.has(account)}
                data-account-index={i}
                onclick={() => toggleAccountSelection(account)}
              >
                <span class="checkbox">{selectedAccounts.has(account) ? "☑" : "☐"}</span>
                <span class="account-name">{account}</span>
              </button>
            {/each}
          </div>
          <div class="dropdown-footer">
            <button class="dropdown-apply" onclick={applyAccountFilterAndClose}>
              Apply Filter
            </button>
          </div>
        </div>
      {/if}
    </div>

    <!-- Untagged toggle -->
    <button
      class="filter-toggle"
      class:active={filterMode === "untagged"}
      onclick={toggleFilterMode}
      title="Press 'u' to toggle"
    >
      <span class="toggle-track">
        <span class="toggle-thumb"></span>
      </span>
      <span class="toggle-label">Untagged</span>
    </button>

    <!-- Manage Rules button -->
    <button
      class="manage-rules-btn"
      onclick={openRulesModal}
      title="Manage auto-tag rules"
    >
      <Icon name="zap" size={14} />
    </button>

    <!-- Overflow menu -->
    <div class="overflow-menu-container">
      <button class="overflow-menu-btn" onclick={toggleOverflowMenu} title="More options">
        ⋮
      </button>
      {#if showOverflowMenu}
        <div class="overflow-menu" bind:this={overflowMenuRef}>
          <button class="overflow-menu-item" onclick={() => { showOverflowMenu = false; openAddModal(); }}>
            Add Transaction
          </button>
          <button class="overflow-menu-item" onclick={() => { showOverflowMenu = false; openRulesModal(); }}>
            Manage Rules
          </button>
        </div>
      {/if}
    </div>
  </div>

  {#if error}
    <div class="error-bar">{error}</div>
  {/if}

  <!-- Main content area: list + sidebar -->
  <div class="main-content">
    <!-- Transaction list -->
    <div class="list-container" onscroll={handleListScroll}>
      {#if isLoading}
        <div class="empty-state">Loading...</div>
      {:else if transactions.length === 0}
        <div class="empty-state">
          {#if filterMode === "untagged"}
            All caught up! No untagged transactions.
          {:else if accountsWithIds.length === 0}
            <div class="empty-title">No transactions yet</div>
            <div class="empty-message">
              Import transactions from a CSV file or connect your bank to get started.
            </div>
            <div class="empty-actions">
              <button class="btn primary" onclick={() => registry.executeCommand("data:import")}>
                Import CSV
              </button>
              <span class="empty-or">or</span>
              <button class="btn primary" onclick={() => registry.executeCommand("core:settings:integrations")}>
                Connect Bank
              </button>
            </div>
          {:else}
            <div class="empty-title">No transactions yet</div>
            <div class="empty-message">
              Record a transaction or import from a file.
            </div>
            <div class="empty-actions">
              <button class="btn primary" onclick={openAddModal}>
                Add Transaction
              </button>
              <span class="empty-or">or</span>
              <button class="btn primary" onclick={() => registry.executeCommand("data:import")}>
                Import CSV
              </button>
            </div>
          {/if}
        </div>
      {:else}
        <!-- Header row -->
        <div class="row header-row">
          <button
            class="row-checkbox header-checkbox"
            class:checked={isAllSelected}
            class:indeterminate={isSomeSelected}
            onclick={toggleSelectAll}
            title="Select all (a)"
          >
            {#if isAllSelected}☑{:else if isSomeSelected}▣{:else}☐{/if}
          </button>
          <div class="row-date header-label">Date</div>
          <div class="row-account header-label">Account</div>
          <div class="row-desc header-label">Description</div>
          <div class="row-amount header-label">Amount<span class="header-copy-spacer"></span></div>
          <div class="row-tags header-label">Tags</div>
          <div class="row-menu-placeholder"></div>
        </div>
        {#each transactions as txn, index}
          {@const splitInfo = getSplitGroupInfo(txn, index)}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            class="row"
            class:cursor={cursorIndex === index}
            class:selected={selectedIndices.has(index)}
            class:split-child={!!txn.parent_transaction_id}
            class:split-first={splitInfo?.isFirst}
            class:split-last={splitInfo?.isLast}
            data-index={index}
            onclick={() => handleRowClick(index)}
            ondblclick={() => handleRowDoubleClick(index)}
            onkeydown={(e) => e.key === 'Enter' && handleRowDoubleClick(index)}
            role="option"
            aria-selected={cursorIndex === index}
            tabindex={cursorIndex === index ? 0 : -1}
          >
            <button
              class="row-checkbox"
              class:checked={selectedIndices.has(index)}
              onclick={(e) => { e.stopPropagation(); toggleSelectionAt(index); }}
              title="Toggle selection (space)"
            >
              {selectedIndices.has(index) ? '☑' : '☐'}
            </button>
            <div class="row-date" role="gridcell" tabindex="-1">
              {txn.transaction_date}
            </div>
            <div class="row-account" role="gridcell" tabindex="-1">
              {txn.account_nickname || txn.account_name || ''}
            </div>
            <div class="row-desc" role="gridcell" tabindex="-1">
              {#if txn.parent_transaction_id}
                <span class="split-badge" title="Part of split">⑂</span>
              {/if}
              <span class="desc-text">{txn.description}</span>
              <CopyButton value={txn.description} class="copy-btn-desc" />
            </div>
            <div
              class="row-amount"
              class:negative={txn.amount < 0}
              class:positive={txn.amount >= 0}
              role="gridcell"
              tabindex="-1"
            >
              <span class="amount-text">{formatUserCurrency(txn.amount)}</span>
              <CopyButton value={formatUserCurrency(txn.amount)} class="copy-btn-amount" />
            </div>
            <div
              class="row-tags"
              class:editing={inlineEditingIndex === index}
              ondblclick={(e) => { e.stopPropagation(); startInlineEditing(index); }}
            >
              {#if inlineEditingIndex === index}
                <!-- Inline tag editor -->
                <div class="inline-row-editor">
                  <TagInput
                    bind:this={inlineTagInputRef}
                    bind:value={inlineTagInput}
                    {allTags}
                    class="inline-tag-input-field"
                    placeholder="Type tags..."
                    onsubmit={handleInlineSubmit}
                    oncancel={cancelInlineEditing}
                    onclick={(e) => e.stopPropagation()}
                    submitOnSelect={true}
                    compact={true}
                  />
                </div>
              {:else if txn.tags.length === 0}
                <button
                  class="add-tag-placeholder"
                  onclick={(e) => { e.stopPropagation(); startInlineEditing(index); }}
                  title="Click or press t to add tags"
                >+ tag</button>
              {:else}
                <!-- Show auto-tag indicator if tags were applied by rules -->
                {#if txn.tags_auto_applied}
                  <span class="auto-tag-indicator" data-tooltip="Tags applied by rule">
                    <Icon name="zap" size={12} />
                  </span>
                {/if}
                {#each txn.tags.slice(0, 3) as tag}
                  <span class="tag">{tag}</span>
                {/each}
                {#if txn.tags.length > 3}
                  <span class="tag-more">+{txn.tags.length - 3}</span>
                {/if}
                <button
                  class="add-tag-btn"
                  onclick={(e) => { e.stopPropagation(); startInlineEditing(index); }}
                  title="Add more tags"
                >+</button>
              {/if}
            </div>
            <RowMenu
              items={getRowMenuItems(txn)}
              isOpen={contextMenuTxn?.transaction_id === txn.transaction_id}
              onToggle={(e) => openContextMenu(txn, e)}
              onClose={closeContextMenu}
              title="Transaction actions"
            />
          </div>
        {/each}
        {#if isLoadingMore}
          <div class="loading-spinner">
            <span class="spinner"></span>
            <span>Loading more...</span>
          </div>
        {:else if hasMore}
          <div class="load-more-hint">Scroll for more</div>
        {/if}
      {/if}
    </div>

    <!-- Sidebar -->
    <div class="sidebar">
      {#if selectedIndices.size > 0 && selectionStats}
        <!-- Selection Mode Sidebar -->
        <div class="sidebar-section selection-section">
          <div class="sidebar-title selection-title">
            <span class="selection-badge">{selectionStats.count}</span>
            Selected
          </div>
          <p class="selection-hint">Press <kbd>t</kbd> to tag, or use number keys</p>
          <div class="selection-actions">
            <button class="btn danger small" onclick={promptBulkDelete}>
              Delete Selected
            </button>
          </div>
        </div>

        <!-- Your Favorites section (pinned tags - stable) -->
        {#if pinnedTagsDisplay.length > 0}
          <div class="sidebar-section">
            <div class="sidebar-title">
              Your Favorites
              <button class="sidebar-edit-btn" onclick={() => showPinnedTagsModal = true} title="Edit favorites">Edit</button>
            </div>
            <div class="tag-suggestions">
              {#each pinnedTagsDisplay as item}
                <button
                  class="tag-suggestion pinned"
                  onclick={() => applyTagToCurrentOrSelected(item.tag)}
                >
                  <span class="tag-shortcut">{item.shortcutNumber}</span>
                  <span class="tag-name">{item.tag}</span>
                  <span class="pinned-icon" title="Pinned favorite"><Icon name="pin" size={12} /></span>
                </button>
              {/each}
            </div>
          </div>
        {:else}
          <div class="sidebar-section">
            <div class="sidebar-title">
              Your Favorites
              <button class="sidebar-edit-btn" onclick={() => showPinnedTagsModal = true} title="Add favorites">Add</button>
            </div>
            <div class="sidebar-empty-hint">Pin your most-used tags for quick access</div>
          </div>
        {/if}

        <!-- Suggestions section (dynamic, changes per transaction) -->
        <div class="sidebar-section">
          <div class="sidebar-title">
            Suggestions
            <span class="sidebar-subtitle">for selection</span>
          </div>
          {#if computedSuggestionsDisplay.length === 0}
            <div class="sidebar-empty">No suggestions</div>
          {:else}
            <div class="tag-suggestions">
              {#each computedSuggestionsDisplay as suggestion}
                <button
                  class="tag-suggestion computed"
                  onclick={() => applyTagToCurrentOrSelected(suggestion.tag)}
                >
                  <span class="tag-shortcut">{suggestion.shortcutNumber}</span>
                  <span class="tag-name">{suggestion.tag}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>

        <!-- Selection Stats (Excel-style) -->
        <div class="sidebar-section selection-stats">
          <div class="sidebar-title">Stats</div>
          <div class="stats-grid">
            <div class="stat-row">
              <span class="stat-label">Sum</span>
              <span class="stat-value" class:negative={selectionStats.sum < 0} class:positive={selectionStats.sum >= 0}>
                {formatUserCurrency(selectionStats.sum)}
              </span>
            </div>
            <div class="stat-row">
              <span class="stat-label">Average</span>
              <span class="stat-value" class:negative={selectionStats.avg < 0} class:positive={selectionStats.avg >= 0}>
                {formatUserCurrency(selectionStats.avg)}
              </span>
            </div>
            <div class="stat-row">
              <span class="stat-label">Min</span>
              <span class="stat-value" class:negative={selectionStats.min < 0} class:positive={selectionStats.min >= 0}>
                {formatUserCurrency(selectionStats.min)}
              </span>
            </div>
            <div class="stat-row">
              <span class="stat-label">Max</span>
              <span class="stat-value" class:negative={selectionStats.max < 0} class:positive={selectionStats.max >= 0}>
                {formatUserCurrency(selectionStats.max)}
              </span>
            </div>
          </div>
        </div>
      {:else}
        <!-- Normal Mode Sidebar -->

        <!-- Your Favorites section (pinned tags - stable, never change) -->
        {#if pinnedTagsDisplay.length > 0}
          <div class="sidebar-section">
            <div class="sidebar-title">
              Your Favorites
              <button class="sidebar-edit-btn" onclick={() => showPinnedTagsModal = true} title="Edit favorites">Edit</button>
            </div>
            <div class="tag-suggestions">
              {#each pinnedTagsDisplay as item}
                <button
                  class="tag-suggestion pinned"
                  onclick={() => applyTagToCurrentOrSelected(item.tag)}
                >
                  <span class="tag-shortcut">{item.shortcutNumber}</span>
                  <span class="tag-name">{item.tag}</span>
                  <span class="pinned-icon" title="Pinned favorite"><Icon name="pin" size={12} /></span>
                </button>
              {/each}
            </div>
          </div>
        {:else}
          <div class="sidebar-section">
            <div class="sidebar-title">
              Your Favorites
              <button class="sidebar-edit-btn" onclick={() => showPinnedTagsModal = true} title="Add favorites">Add</button>
            </div>
            <div class="sidebar-empty-hint">Pin your most-used tags for quick access</div>
          </div>
        {/if}

        <!-- Suggestions section (dynamic, changes per transaction) -->
        <div class="sidebar-section">
          <div class="sidebar-title">
            Suggestions
            {#if currentTxn}
              <span class="sidebar-subtitle">for this transaction</span>
            {/if}
          </div>
          {#if computedSuggestionsDisplay.length === 0}
            {#if !hasAnyTags}
              <div class="sidebar-empty-state">
                <p>No suggestions yet</p>
                <p class="empty-hint">Press <kbd>t</kbd> to type a tag</p>
              </div>
            {:else}
              <div class="sidebar-empty">No suggestions for this transaction</div>
            {/if}
          {:else}
            <div class="tag-suggestions">
              {#each computedSuggestionsDisplay as suggestion}
                <button
                  class="tag-suggestion computed"
                  onclick={() => applyTagToCurrentOrSelected(suggestion.tag)}
                >
                  <span class="tag-shortcut">{suggestion.shortcutNumber}</span>
                  <span class="tag-name">{suggestion.tag}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>

        <!-- Transaction Info (details + current tags) -->
        {#if currentTxn}
          <div class="sidebar-section txn-details">
            <div class="sidebar-title">Current Transaction</div>
            <div class="txn-details-content">
              <div class="txn-detail-row">
                <span class="txn-detail-label">Date</span>
                <span class="txn-detail-value">{currentTxn.transaction_date}</span>
              </div>
              <div class="txn-detail-stacked">
                <span class="txn-detail-label">Account</span>
                <span class="txn-detail-value">{currentTxn.account_nickname || currentTxn.account_name || 'Unknown'}</span>
              </div>
              <div class="txn-detail-row">
                <span class="txn-detail-label">Amount</span>
                <span class="txn-detail-value" class:negative={currentTxn.amount < 0} class:positive={currentTxn.amount >= 0}>
                  {formatUserCurrency(currentTxn.amount)}
                </span>
              </div>
              <div class="txn-detail-desc">
                <span class="txn-detail-label">Description</span>
                <span class="txn-detail-value desc">{currentTxn.description}</span>
              </div>
              {#if currentTxn.tags.length > 0}
                <div class="txn-detail-tags">
                  <span class="txn-detail-label">
                    Tags
                    <!-- Show auto-tag indicator if tags were applied by rules -->
                    {#if currentTxn.tags_auto_applied}
                      <span class="auto-tag-badge" data-tooltip="Tags applied by rule">
                        <Icon name="zap" size={11} /> Auto
                      </span>
                    {/if}
                  </span>
                  <div class="current-tags">
                    {#each currentTxn.tags as tag}
                      <span class="current-tag">
                        {tag}
                        <button
                          class="tag-remove-btn"
                          onclick={() => removeSingleTag(tag)}
                          aria-label="Remove tag {tag}"
                        >×</button>
                      </span>
                    {/each}
                  </div>
                </div>
              {/if}
            </div>
          </div>
        {/if}

        <!-- Split Transaction Info -->
        {#if currentTxn?.parent_transaction_id}
          {@const siblingIndices = splitGroups.get(currentTxn.parent_transaction_id) || []}
          {@const siblings = siblingIndices.map(i => transactions[i]).filter(Boolean)}
          <div class="sidebar-section split-info">
            <div class="sidebar-title">Split Transaction</div>
            <div class="split-siblings">
              {#each siblings as sibling}
                <div class="split-sibling" class:current={sibling.transaction_id === currentTxn.transaction_id}>
                  <span class="sibling-desc">{sibling.description}</span>
                  <span class="sibling-amount" class:negative={sibling.amount < 0} class:positive={sibling.amount >= 0}>
                    {formatUserCurrency(Math.abs(sibling.amount))}
                  </span>
                </div>
              {/each}
              <div class="split-total">
                <span class="total-label">Total:</span>
                <span class="total-amount">{formatUserCurrency(Math.abs(siblings.reduce((sum, s) => sum + s.amount, 0)))}</span>
              </div>
            </div>
          </div>
        {/if}
      {/if}
    </div>
  </div>

  <!-- Command bar / Footer -->
  <div class="command-bar">
    <!-- Keyboard shortcuts footer -->
    <div class="shortcuts-row">
      <span class="shortcut"><kbd>j</kbd><kbd>k</kbd> nav</span>
      <span class="shortcut"><kbd>1-9</kbd> quick tag</span>
      <span class="shortcut"><kbd>t</kbd> type tag</span>
      <span class="shortcut"><kbd>c</kbd> clear tags</span>
      <span class="shortcut"><kbd>a</kbd> toggle all</span>
      <span class="shortcut"><kbd>space</kbd> select</span>
      <span class="shortcut"><kbd>n</kbd> next untagged</span>
      <span class="shortcut"><kbd>{modKey()}Z</kbd> undo</span>
    </div>
  </div>
</div>

<!-- Modals -->
<EditTransactionModal
  open={isTagModalOpen && !!editingTransaction}
  transaction={editingTransaction}
  suggestions={currentSuggestions}
  onclose={closeTagModal}
  onsave={handleEditSave}
/>

<UnsplitConfirmModal
  open={showUnsplitConfirm && !!editingTransaction}
  onclose={() => { showUnsplitConfirm = false; containerEl?.focus(); }}
  onconfirm={unsplitTransaction}
/>

<DeleteConfirmModal
  open={showDeleteConfirm && !!editingTransaction}
  transaction={editingTransaction}
  onclose={() => { showDeleteConfirm = false; containerEl?.focus(); }}
  onconfirm={deleteTransaction}
/>

<!-- Bulk Delete Confirmation -->
<Modal open={showBulkDeleteConfirm} title="Delete Transactions?" onclose={() => { showBulkDeleteConfirm = false; containerEl?.focus(); }} width="400px">
  <div class="confirm-body">
    <p>Are you sure you want to delete <strong>{bulkDeleteCount}</strong> transaction{bulkDeleteCount === 1 ? '' : 's'}?</p>
    <p class="confirm-note">These transactions won't be re-imported during sync.</p>
  </div>

  {#snippet actions()}
    <button class="btn secondary" onclick={() => { showBulkDeleteConfirm = false; containerEl?.focus(); }}>Cancel</button>
    <button class="btn danger" onclick={bulkDeleteTransactions}>Delete {bulkDeleteCount}</button>
  {/snippet}
</Modal>

<SplitTransactionModal
  open={showSplitModal && !!editingTransaction}
  transaction={editingTransaction}
  onclose={closeSplitModal}
  onsplit={handleSplit}
/>

<AddTransactionModal
  open={showAddModal}
  accounts={accountsWithIds}
  onclose={closeAddModal}
  onsave={handleAddTransaction}
/>

<RulesManagementModal
  open={showRulesModal}
  onclose={closeRulesModal}
  initialMode={rulesModalMode}
  prefillTransactions={rulesModalPrefillTransactions}
  prefillTags={rulesModalPrefillTags}
  onsaved={handleRuleSaved}
/>

<PinnedTagsModal
  open={showPinnedTagsModal}
  pinnedTags={pinnedQuickTags}
  {allTags}
  onclose={() => { showPinnedTagsModal = false; containerEl?.focus(); }}
  onsave={handleSavePinnedTags}
/>

<style>
  .tagging-view {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-primary);
    outline: none;
  }

  .header {
    padding: var(--spacing-md) var(--spacing-lg);
    border-bottom: 1px solid var(--border-primary);
    background: var(--bg-secondary);
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
  }

  .title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  /* Overflow menu in header */
  .overflow-menu-container {
    position: relative;
  }

  .overflow-menu-btn {
    width: 32px;
    height: 32px;
    padding: 0;
    background: transparent;
    color: var(--text-secondary);
    border: 1px solid transparent;
    border-radius: 4px;
    font-size: 20px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .overflow-menu-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border-color: var(--border-primary);
  }

  .overflow-menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
    min-width: 160px;
    z-index: 100;
    overflow: hidden;
  }

  .overflow-menu-item {
    width: 100%;
    padding: 10px 14px;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }

  .overflow-menu-item:hover {
    background: var(--bg-tertiary);
  }

  /* Filter bar */
  .filter-bar {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-lg);
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
  }

  /* Untagged badge in header */
  .untagged-badge {
    font-size: 11px;
    font-weight: 500;
    color: var(--accent-warning, #f59e0b);
    padding: 2px 8px;
    background: rgba(245, 158, 11, 0.1);
    border-radius: 10px;
  }

  .complete-badge {
    font-size: 11px;
    font-weight: 500;
    color: var(--accent-success, #22c55e);
    padding: 2px 8px;
    background: rgba(34, 197, 94, 0.1);
    border-radius: 10px;
  }

  /* Search container */
  .search-container {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    height: 32px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
  }

  .search-container:focus-within {
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 2px rgba(99, 102, 241, 0.1);
  }

  .search-icon {
    color: var(--text-muted);
    font-size: 16px;
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 14px;
    outline: none;
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .clear-search {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    padding: 0 4px;
    line-height: 1;
  }

  .clear-search:hover {
    color: var(--text-primary);
  }

  /* Toggle button */
  .filter-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    height: 36px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .filter-toggle:hover {
    border-color: var(--text-muted);
  }

  .filter-toggle.active {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .toggle-track {
    width: 28px;
    height: 16px;
    background: var(--bg-tertiary);
    border-radius: 8px;
    position: relative;
    transition: background 0.2s;
  }

  .filter-toggle.active .toggle-track {
    background: rgba(255, 255, 255, 0.3);
  }

  .toggle-thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    background: var(--text-muted);
    border-radius: 50%;
    transition: left 0.2s, background 0.2s;
  }

  .filter-toggle.active .toggle-thumb {
    left: 14px;
    background: white;
  }

  .toggle-label {
    font-size: 12px;
    color: var(--text-secondary);
    font-weight: 500;
  }

  .filter-toggle.active .toggle-label {
    color: white;
  }

  /* Manage rules button */
  .manage-rules-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    cursor: pointer;
    color: var(--accent-warning);
    flex-shrink: 0;
  }

  .manage-rules-btn:hover {
    border-color: var(--accent-warning);
    background: rgba(245, 158, 11, 0.1);
  }

  /* Account filter dropdown */
  .account-filter-container {
    position: relative;
    margin-left: auto; /* Push to right side of header */
  }

  .account-filter-btn {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: 4px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    cursor: pointer;
    min-width: 140px;
    justify-content: space-between;
  }

  .account-filter-btn:hover {
    border-color: var(--accent-primary);
  }

  .account-filter-btn.active {
    background: var(--accent-primary);
    color: var(--bg-primary);
    border-color: var(--accent-primary);
  }

  .dropdown-arrow {
    font-size: 8px;
    opacity: 0.7;
  }

  .account-dropdown {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
    z-index: 100;
    min-width: 250px;
    display: flex;
    flex-direction: column;
  }

  .dropdown-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-primary);
  }

  .dropdown-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .clear-selection {
    background: none;
    border: none;
    color: var(--accent-primary);
    font-size: 11px;
    cursor: pointer;
    padding: 2px 6px;
  }

  .clear-selection:hover {
    text-decoration: underline;
  }

  .dropdown-hint {
    padding: 6px 12px;
    font-size: 10px;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
  }

  .dropdown-hint kbd {
    display: inline-block;
    padding: 1px 3px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 2px;
    font-family: var(--font-mono);
    font-size: 9px;
    margin-right: 2px;
  }

  .account-list {
    max-height: 300px;
    overflow-y: auto;
  }

  .account-option {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 12px;
    cursor: pointer;
    text-align: left;
  }

  .account-option:hover {
    background: var(--bg-tertiary);
  }

  .account-option.cursor {
    background: var(--bg-tertiary);
    border-left: 3px solid var(--accent-primary);
    padding-left: 9px;
  }

  .account-option.selected {
    font-weight: 600;
  }

  .account-option.selected .checkbox {
    color: var(--accent-primary);
  }

  .checkbox {
    font-size: 14px;
    width: 18px;
    text-align: center;
    flex-shrink: 0;
  }

  .account-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dropdown-footer {
    padding: 8px 12px;
    border-top: 1px solid var(--border-primary);
  }

  .dropdown-apply {
    width: 100%;
    padding: 6px 12px;
    background: var(--accent-primary);
    color: var(--bg-primary);
    border: none;
    border-radius: 4px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .dropdown-apply:hover {
    opacity: 0.9;
  }

  .error-bar {
    padding: var(--spacing-sm) var(--spacing-lg);
    background: var(--accent-danger);
    color: white;
    font-size: 12px;
  }

  .main-content {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .list-container {
    flex: 1;
    overflow-y: auto;
    font-family: var(--font-mono);
    font-size: 13px;
  }

  .sidebar {
    width: 200px;
    flex-shrink: 0;
    border-left: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    padding: var(--spacing-md);
    overflow-y: auto;
  }

  .sidebar-section {
    margin-bottom: var(--spacing-lg);
  }

  .sidebar-title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: var(--spacing-sm);
  }

  .sidebar-edit-btn {
    padding: 2px 8px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 10px;
    font-weight: 500;
    border-radius: 3px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }

  .sidebar-edit-btn:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .sidebar-empty {
    font-size: 12px;
    color: var(--text-muted);
    font-style: italic;
  }

  .sidebar-empty-state {
    padding: var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 6px;
    text-align: center;
  }

  .sidebar-empty-state p {
    margin: 0;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .sidebar-empty-state .empty-hint {
    margin-top: var(--spacing-xs);
    font-size: 12px;
    color: var(--text-muted);
  }

  /* Selection section in sidebar */
  .selection-section {
    background: rgba(var(--accent-primary-rgb, 59, 130, 246), 0.08);
    border: 1px solid rgba(var(--accent-primary-rgb, 59, 130, 246), 0.2);
    border-radius: 6px;
    padding: var(--spacing-md);
  }

  .selection-title {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    text-transform: none;
    letter-spacing: normal;
  }

  .selection-title .selection-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 24px;
    height: 24px;
    padding: 0 6px;
    background: var(--accent-primary);
    color: white;
    font-size: 13px;
    font-weight: 700;
    border-radius: 12px;
  }

  .selection-hint {
    font-size: 12px;
    color: var(--text-secondary);
    margin: var(--spacing-xs) 0 0 0;
  }

  .selection-actions {
    margin-top: var(--spacing-md);
  }

  .selection-actions .btn {
    width: 100%;
    padding: 6px 12px;
    border-radius: 4px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }

  .selection-actions .btn.danger {
    background: var(--accent-danger, #ef4444);
    border: none;
    color: white;
  }

  .selection-actions .btn.danger:hover {
    opacity: 0.9;
  }

  /* Bulk delete confirm modal */
  .confirm-body {
    padding: var(--spacing-md) var(--spacing-lg);
  }

  .confirm-body p {
    margin: 0 0 var(--spacing-md) 0;
    color: var(--text-primary);
    font-size: 14px;
  }

  .confirm-body .confirm-note {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: var(--spacing-md);
  }

  /* Selection stats sidebar styles */
  .selection-stats {
    background: rgba(var(--accent-primary-rgb, 59, 130, 246), 0.06);
    border: 1px solid rgba(var(--accent-primary-rgb, 59, 130, 246), 0.15);
    border-radius: 6px;
    padding: var(--spacing-sm) var(--spacing-md);
  }

  .selection-stats .sidebar-title {
    margin-bottom: var(--spacing-xs);
  }

  .stats-grid {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .stat-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
    padding: 2px 0;
  }

  .stat-label {
    color: var(--text-muted);
    font-size: 11px;
  }

  .stat-value {
    font-family: var(--font-mono, monospace);
    font-weight: 500;
    color: var(--text-primary);
  }

  .stat-value.negative {
    color: var(--accent-danger);
  }

  .stat-value.positive {
    color: var(--accent-success);
  }

  .txn-details-content {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xs);
  }

  .txn-detail-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
  }

  .txn-detail-stacked {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
  }

  .txn-detail-desc,
  .txn-detail-tags {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
    margin-top: var(--spacing-xs);
  }

  .txn-detail-tags .current-tags {
    margin-top: 4px;
  }

  .txn-detail-label {
    color: var(--text-muted);
    font-size: 11px;
  }

  .txn-detail-value {
    color: var(--text-primary);
  }

  .txn-detail-value.desc {
    word-break: break-word;
    line-height: 1.4;
  }

  .txn-detail-value.negative {
    color: var(--accent-danger);
  }

  .txn-detail-value.positive {
    color: var(--accent-success);
  }

  .tag-suggestions {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .tag-suggestion {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: 4px 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    cursor: pointer;
    text-align: left;
    font-size: 12px;
  }

  .tag-suggestion:hover {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
  }

  .tag-shortcut {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: var(--accent-primary);
    color: var(--bg-primary);
    border-radius: 3px;
    font-size: 11px;
    font-weight: 600;
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  .tag-name {
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .pinned-icon {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    color: var(--text-muted);
  }

  .split-info {
    border-top: 1px solid var(--border-primary);
    padding-top: var(--spacing-md);
  }

  .split-siblings {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11px;
  }

  .split-sibling {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 6px;
    background: var(--bg-primary);
    border-radius: 3px;
    gap: 8px;
  }

  .split-sibling.current {
    background: var(--bg-tertiary);
    border-left: 2px solid var(--accent-primary);
    padding-left: 4px;
  }

  .sibling-desc {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-muted);
  }

  .split-sibling.current .sibling-desc {
    color: var(--text-primary);
  }

  .sibling-amount {
    flex-shrink: 0;
    font-family: var(--font-mono);
    color: var(--text-primary);
  }

  .sibling-amount.negative {
    color: var(--accent-danger);
  }

  .sibling-amount.positive {
    color: var(--accent-success);
  }

  .split-total {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 6px;
    border-top: 1px solid var(--border-primary);
    margin-top: 4px;
  }

  .total-label {
    color: var(--text-muted);
    font-weight: 500;
  }

  .total-amount {
    font-family: var(--font-mono);
    font-weight: 600;
    color: var(--text-primary);
  }

  .current-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .current-tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    background: var(--accent-primary);
    color: var(--bg-primary);
    border-radius: 3px;
    font-size: 10px;
    font-weight: 600;
  }

  .tag-remove-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 12px;
    height: 12px;
    padding: 0;
    margin-left: 2px;
    background: rgba(0, 0, 0, 0.2);
    border: none;
    border-radius: 50%;
    color: inherit;
    font-size: 10px;
    line-height: 1;
    cursor: pointer;
    opacity: 0.7;
    transition: opacity 0.15s, background 0.15s;
  }

  .tag-remove-btn:hover {
    opacity: 1;
    background: rgba(0, 0, 0, 0.4);
  }

  .auto-tag-badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 6px;
    background: rgba(245, 158, 11, 0.15);
    color: var(--accent-warning);
    border-radius: 3px;
    font-size: 10px;
    font-weight: 600;
    margin-left: 6px;
  }

  .empty-state {
    padding: var(--spacing-xl);
    text-align: center;
    color: var(--text-muted);
  }

  .empty-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: var(--spacing-md);
  }

  .empty-message {
    font-size: 13px;
    margin-bottom: var(--spacing-lg);
  }

  .empty-actions {
    display: flex;
    gap: var(--spacing-md);
    justify-content: center;
    align-items: center;
  }

  .empty-or {
    font-size: 12px;
    color: var(--text-muted);
  }

  .empty-state .btn {
    padding: 6px 12px;
    border-radius: 4px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .empty-state .btn.primary {
    background: var(--accent-primary);
    border: none;
    color: white;
  }

  .empty-state .btn.primary:hover {
    opacity: 0.9;
  }

  /* Row checkbox */
  .row-checkbox {
    width: 20px;
    height: 20px;
    padding: 0;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 14px;
    cursor: pointer;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0.5;
    transition: opacity 0.15s;
  }

  .row:hover .row-checkbox,
  .row-checkbox:hover {
    opacity: 1;
  }

  .row-checkbox.checked {
    color: var(--accent-primary);
    opacity: 1;
  }

  .row {
    display: flex;
    align-items: center;
    padding: 6px var(--spacing-lg);
    border-bottom: 1px solid var(--border-primary);
    gap: var(--spacing-md);
    cursor: pointer;
  }

  .row:hover {
    background: var(--bg-secondary);
  }

  .row.cursor {
    background: var(--bg-tertiary);
    border-left: 3px solid var(--accent-primary);
    padding-left: calc(var(--spacing-lg) - 3px);
  }

  .row.selected {
    background: rgba(var(--accent-primary-rgb, 99, 102, 241), 0.15);
  }

  .row.cursor.selected {
    background: rgba(var(--accent-primary-rgb, 99, 102, 241), 0.25);
  }

  .row-date {
    width: 90px;
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 12px;
  }

  .row-account {
    width: 140px;
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row-desc {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-primary);
  }

  .row-amount {
    width: 90px;
    text-align: right;
    flex-shrink: 0;
  }

  .row-amount.positive {
    color: var(--accent-success, #22c55e);
  }

  .row-amount.negative {
    color: var(--accent-danger);
  }

  /* Copyable cells (mirrors query plugin) */
  /* Description cell - copy button absolute positioned */
  .row-desc {
    position: relative;
  }

  .row-desc .desc-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding-right: 26px;
  }

  .row-desc :global(.copy-btn-desc) {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
  }

  .row-desc:hover :global(.copy-button) {
    opacity: 1;
  }

  /* Amount cell - flex layout with visibility-based hover */
  .row-amount {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
  }

  .amount-text {
    text-align: right;
  }

  .row-amount :global(.copy-btn-amount) {
    visibility: hidden;
    opacity: 1; /* Override component default, use visibility for show/hide */
  }

  .row-amount:hover :global(.copy-btn-amount) {
    visibility: visible;
  }

  /* Invisible spacer in header to align with copy button */
  .header-copy-spacer {
    display: inline-block;
    width: 26px;
  }

  .row-tags {
    width: 180px;
    flex-shrink: 0;
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    align-items: center;
  }

  /* Add tag placeholder for empty rows */
  .add-tag-placeholder {
    padding: 1px 8px;
    background: transparent;
    border: 1px dashed var(--border-primary);
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 10px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .add-tag-placeholder:hover {
    border-color: var(--accent-primary);
    color: var(--accent-primary);
    background: rgba(99, 102, 241, 0.05);
  }

  /* Add tag button for rows with existing tags */
  .add-tag-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    background: transparent;
    border: 1px dashed var(--border-primary);
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    opacity: 0.5;
    transition: all 0.15s;
  }

  .row:hover .add-tag-btn {
    opacity: 1;
  }

  .add-tag-btn:hover {
    border-color: var(--accent-primary);
    color: var(--accent-primary);
    background: rgba(99, 102, 241, 0.1);
    opacity: 1;
  }

  .tag {
    padding: 1px 6px;
    background: var(--accent-primary);
    color: var(--bg-primary);
    border-radius: 3px;
    font-size: 10px;
    font-weight: 600;
  }

  .tag-more {
    padding: 1px 6px;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    border-radius: 3px;
    font-size: 10px;
  }

  .auto-tag-indicator {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--accent-warning);
    margin-right: 4px;
    opacity: 0.8;
  }

  .auto-tag-indicator:hover {
    opacity: 1;
  }

  /* Fast tooltip for auto-tag indicators */
  [data-tooltip] {
    position: relative;
    cursor: default;
  }

  [data-tooltip]::after {
    content: attr(data-tooltip);
    position: absolute;
    bottom: 100%;
    left: 50%;
    transform: translateX(-50%);
    margin-bottom: 4px;
    background: rgba(0, 0, 0, 0.85);
    color: white;
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 400;
    white-space: nowrap;
    z-index: 1000;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.1s ease-out;
  }

  [data-tooltip]:hover::after {
    opacity: 1;
  }

  /* Inline row tag editor */
  .inline-row-editor {
    position: relative;
    width: 100%;
  }

  /* Sidebar subtitle */
  .sidebar-subtitle {
    font-size: 9px;
    font-weight: 400;
    color: var(--text-muted);
    text-transform: none;
    letter-spacing: normal;
    margin-left: 4px;
  }

  /* Sidebar empty hint */
  .sidebar-empty-hint {
    font-size: 11px;
    color: var(--text-muted);
    padding: var(--spacing-sm);
    background: var(--bg-tertiary);
    border-radius: 4px;
    line-height: 1.4;
  }

  /* Pinned vs computed tag styles */
  .tag-suggestion.pinned {
    border-left: 2px solid var(--accent-primary);
  }

  .tag-suggestion.computed {
    border-left: 2px solid transparent;
    opacity: 0.9;
  }

  .tag-suggestion.computed:hover {
    opacity: 1;
  }

  /* Kbd styling in sidebar hints */
  .selection-hint kbd,
  .empty-hint kbd {
    display: inline-block;
    padding: 1px 4px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 3px;
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-secondary);
  }

  /* Split transaction styles - subtle box grouping */
  .row.split-child {
    position: relative;
    border-left: 1px solid var(--border-primary);
    border-right: 1px solid var(--border-primary);
    margin-left: 8px;
    margin-right: 8px;
    border-bottom: none;
  }

  .row.split-child:not(.split-last) {
    border-bottom: 1px dashed var(--border-primary);
  }

  .row.split-first {
    border-top: 1px solid var(--border-primary);
    border-top-left-radius: 4px;
    border-top-right-radius: 4px;
    margin-top: 4px;
  }

  .row.split-last {
    border-bottom: 1px solid var(--border-primary);
    border-bottom-left-radius: 4px;
    border-bottom-right-radius: 4px;
    margin-bottom: 4px;
  }

  .split-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    color: var(--text-muted);
    margin-right: 4px;
  }

  /* Header row */
  .row.header-row {
    background: var(--bg-secondary);
    border-bottom: 2px solid var(--border-primary);
    position: sticky;
    top: 0;
    z-index: 10;
    cursor: default;
  }

  .row.header-row:hover {
    background: var(--bg-secondary);
  }

  .header-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .header-checkbox {
    opacity: 1 !important;
  }

  .header-checkbox.indeterminate {
    color: var(--accent-primary);
  }

  .row-menu-placeholder {
    width: 24px;
    flex-shrink: 0;
  }

  .command-bar {
    border-top: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    min-height: 36px;
  }

  /* Shortcuts footer */
  .shortcuts-row {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 8px var(--spacing-lg);
    gap: var(--spacing-lg);
    flex-wrap: wrap;
  }

  .shortcut {
    font-size: 11px;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .shortcut kbd {
    display: inline-block;
    padding: 2px 5px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 3px;
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-secondary);
    margin-right: 2px;
  }

  .loading-spinner {
    padding: 20px;
    text-align: center;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    font-size: 12px;
  }

  .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid var(--border-primary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .load-more-hint {
    padding: 12px;
    text-align: center;
    color: var(--text-muted);
    font-size: 11px;
    opacity: 0.7;
  }

  /* Progress Ring */
  .progress-ring-container {
    position: relative;
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .progress-ring-container.complete {
    animation: pulse-complete 0.5s ease-out;
  }

  @keyframes pulse-complete {
    0% { transform: scale(1); }
    50% { transform: scale(1.2); }
    100% { transform: scale(1); }
  }

  .progress-ring {
    width: 36px;
    height: 36px;
    transform: rotate(-90deg);
  }

  .progress-ring-bg {
    fill: none;
    stroke: var(--border-primary);
    stroke-width: 3;
  }

  .progress-ring-fill {
    fill: none;
    stroke: var(--accent-primary);
    stroke-width: 3;
    stroke-linecap: round;
    transition: stroke-dasharray 0.3s ease;
  }

  .progress-ring-container.complete .progress-ring-fill {
    stroke: var(--accent-success, #22c55e);
  }

  .progress-text {
    position: absolute;
    font-size: 8px;
    font-weight: 600;
    color: var(--text-primary);
    font-family: var(--font-mono);
  }

  /* Confetti */
  .confetti-container {
    position: fixed;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 1000;
    overflow: hidden;
  }

  .confetti {
    position: absolute;
    width: 10px;
    height: 10px;
    background: var(--color);
    top: -10px;
    left: var(--x);
    opacity: 0;
    transform: rotate(var(--rotation));
    animation: confetti-fall 3s ease-out var(--delay) forwards;
  }

  .confetti:nth-child(odd) {
    width: 8px;
    height: 12px;
    border-radius: 2px;
  }

  .confetti:nth-child(even) {
    width: 12px;
    height: 8px;
    border-radius: 50%;
  }

  @keyframes confetti-fall {
    0% {
      opacity: 1;
      top: -10px;
      transform: rotate(var(--rotation)) translateX(0);
    }
    100% {
      opacity: 0;
      top: 100vh;
      transform: rotate(calc(var(--rotation) + 720deg)) translateX(calc(var(--x) * 0.2 - 10vw));
    }
  }

</style>
