<script lang="ts">
  /**
   * RulesManagementModal - Full rules management interface
   *
   * Simple pattern matching by default with match type options,
   * SQL editing available for power users.
   * List, create, edit, delete, and test auto-tag rules.
   */
  import { onMount } from "svelte";
  import { Modal, formatUserCurrency, TagInput } from "../../shared";
  import { executeQuery } from "../../sdk";
  import type { Transaction } from "./types";
  import type { TagRule, RuleTestResult } from "./rules";
  import {
    loadRules,
    saveRule,
    updateRule,
    deleteRule,
    toggleRuleEnabled,
    testSqlCondition,
    generateRuleId,
    getRuleWhereClause,
    generateSqlFromTransactions,
    applyTagsToMatching,
  } from "./rules";

  interface Props {
    open: boolean;
    onclose: () => void;
    /** Start in "list" (default) or "create" mode */
    initialMode?: "list" | "create";
    /** Pre-fill pattern from these transactions (used with initialMode="create") */
    prefillTransactions?: Transaction[];
    /** Pre-fill these tags (used with initialMode="create") */
    prefillTags?: string[];
    /** Callback when a rule is saved. Receives info about what happened. */
    onsaved?: (info: { appliedToExisting: boolean; appliedCount?: number }) => void;
  }

  let { open, onclose, initialMode = "list", prefillTransactions, prefillTags, onsaved }: Props = $props();

  // Match type options for simple mode
  type MatchType = "contains" | "starts_with" | "ends_with" | "exact" | "regex";

  const matchTypeLabels: Record<MatchType, string> = {
    contains: "Contains",
    starts_with: "Starts with",
    ends_with: "Ends with",
    exact: "Exact match",
    regex: "Regex",
  };

  // State
  let rules = $state<TagRule[]>([]);
  let isLoading = $state(true);
  let error = $state<string | null>(null);
  let searchQuery = $state("");

  // Filtered rules based on search query
  let filteredRules = $derived.by(() => {
    if (!searchQuery.trim()) return rules;
    const query = searchQuery.toLowerCase();
    return rules.filter(rule =>
      rule.name.toLowerCase().includes(query) ||
      getRuleWhereClause(rule)?.toLowerCase().includes(query) ||
      rule.tags.some(tag => tag.toLowerCase().includes(query))
    );
  });

  // All known tags for autocomplete (sorted by frequency)
  let allTags = $state<string[]>([]);

  async function loadAllTags() {
    try {
      const result = await executeQuery(`
        WITH unnested AS (
          SELECT UNNEST(tags) as tag
          FROM transactions
          WHERE tags IS NOT NULL AND len(tags) > 0
        )
        SELECT tag, COUNT(*) as cnt
        FROM unnested
        GROUP BY tag
        ORDER BY cnt DESC
      `);
      allTags = result.rows.map(row => row[0] as string);
    } catch (e) {
      // Database might not be ready
      allTags = [];
    }
  }

  // Edit mode
  let editingRule = $state<TagRule | null>(null);
  let isCreating = $state(false);

  // Form state for editing
  let formName = $state("");
  let formPattern = $state("");
  let formMatchType = $state<MatchType>("contains");
  let formSqlCondition = $state("");
  let formShowSql = $state(false);
  let formTags = $state("");
  let formEnabled = $state(true);
  let formApplyToExisting = $state(true); // Apply tags to existing matching transactions when creating

  // Test state
  let isTesting = $state(false);
  let testResult = $state<RuleTestResult | null>(null);
  let testError = $state<string | null>(null);
  let lastTestedCondition = $state<string | null>(null);

  // Debounce timer for auto-preview
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // Build SQL from pattern and match type
  function buildSqlFromPattern(pat: string, type: MatchType): string {
    if (!pat.trim()) return "";
    const escaped = pat.trim().replace(/'/g, "''");
    const lower = escaped.toLowerCase();

    switch (type) {
      case "contains":
        return `description ILIKE '%${lower}%'`;
      case "starts_with":
        return `description ILIKE '${lower}%'`;
      case "ends_with":
        return `description ILIKE '%${lower}'`;
      case "exact":
        return `description ILIKE '${lower}'`;
      case "regex":
        // Use 'i' option for case-insensitive regex matching
        return `regexp_matches(description, '${escaped}', 'i')`;
      default:
        return `description ILIKE '%${lower}%'`;
    }
  }

  // Computed effective SQL from pattern or raw SQL
  let formEffectiveSql = $derived.by(() => {
    if (formShowSql) {
      return formSqlCondition;
    }
    return buildSqlFromPattern(formPattern, formMatchType);
  });

  // Available columns for reference
  const columns = [
    { name: "description", type: "VARCHAR", example: "description ILIKE '%starbucks%'" },
    { name: "amount", type: "DECIMAL", example: "amount < -50" },
    { name: "account_name", type: "VARCHAR", example: "account_name = 'Checking'" },
    { name: "transaction_date", type: "DATE", example: "transaction_date >= '2024-01-01'" },
  ];

  // Common filters that users can click to insert
  const commonFilters = [
    { label: "Contains", sql: "description ILIKE '%keyword%'" },
    { label: "Starts with", sql: "description ILIKE 'keyword%'" },
    { label: "Amount >", sql: "amount > 100" },
    { label: "Amount <", sql: "amount < -50" },
    { label: "Regex", sql: "regexp_matches(description, 'pattern', 'i')" },
    { label: "Date after", sql: "transaction_date >= '2024-01-01'" },
  ];

  function insertFilter(sql: string) {
    formSqlCondition = sql;
    // Immediately test the inserted filter
    lastTestedCondition = null;
    runTest(sql);
  }

  // Load rules and tag data on mount
  onMount(async () => {
    await refreshRules();
    await loadAllTags();
  });

  // Track if we've initialized for the current open state
  let lastInitKey = $state<string | null>(null);

  // Handle modal open - either list or create mode
  $effect(() => {
    if (!open) {
      // Reset all state when modal closes
      lastInitKey = null;
      searchQuery = "";
      isCreating = false;
      editingRule = null;
      testResult = null;
      testError = null;
      return;
    }

    // Build a key to track if we've already initialized
    const txnIds = prefillTransactions?.map(t => t.transaction_id).join(",") || "";
    const initKey = `${initialMode}-${txnIds}`;

    if (initKey === lastInitKey) return;
    lastInitKey = initKey;

    if (initialMode === "create") {
      // Start in create mode with prefilled data
      startCreateWithPrefill();
    } else {
      // Normal list mode
      refreshRules();
    }
  });

  async function refreshRules() {
    isLoading = true;
    error = null;
    try {
      rules = await loadRules();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to load rules";
    } finally {
      isLoading = false;
    }
  }

  function startCreate() {
    isCreating = true;
    editingRule = null;
    formName = "";
    formPattern = "";
    formMatchType = "contains";
    formSqlCondition = "";
    formShowSql = false;
    formTags = "";
    formEnabled = true;
    formApplyToExisting = true;
    testResult = null;
    testError = null;
    lastTestedCondition = null;
  }

  function startCreateWithPrefill() {
    isCreating = true;
    editingRule = null;
    formEnabled = true;
    formApplyToExisting = true;
    formShowSql = false;
    testResult = null;
    testError = null;
    lastTestedCondition = null;

    // Pre-fill tags if provided
    formTags = prefillTags?.join(", ") || "";

    // Generate pattern suggestion from transactions
    if (prefillTransactions && prefillTransactions.length > 0) {
      const descriptions = prefillTransactions.map(t => t.description);
      const suggestion = generateSqlFromTransactions(descriptions);

      formPattern = suggestion.pattern || "";
      formMatchType = "contains";
      formSqlCondition = suggestion.sql;

      // Generate rule name
      if (suggestion.pattern && prefillTags && prefillTags.length > 0) {
        formName = `Tag "${suggestion.pattern}" as ${prefillTags.join(", ")}`;
      } else if (suggestion.pattern) {
        formName = `Match "${suggestion.pattern}"`;
      } else if (prefillTags && prefillTags.length > 0) {
        formName = `Tag as ${prefillTags.join(", ")}`;
      } else {
        formName = "";
      }

      // Auto-test the initial condition
      if (suggestion.sql) {
        runTest(suggestion.sql);
      }
    } else {
      formName = "";
      formPattern = "";
      formMatchType = "contains";
      formSqlCondition = "";
    }
  }

  // Parse SQL to detect match type and pattern
  function parseSqlToPattern(sql: string): { pattern: string; matchType: MatchType } | null {
    // Try contains: description ILIKE '%xxx%' or LOWER(description) LIKE '%xxx%'
    let match = sql.match(/description ILIKE '%(.+)%'$/i) || sql.match(/LOWER\(description\) LIKE '%(.+)%'$/i);
    if (match) return { pattern: match[1], matchType: "contains" };

    // Try starts_with: description ILIKE 'xxx%' or LOWER(description) LIKE 'xxx%'
    match = sql.match(/description ILIKE '([^%]+)%'$/i) || sql.match(/LOWER\(description\) LIKE '([^%]+)%'$/i);
    if (match) return { pattern: match[1], matchType: "starts_with" };

    // Try ends_with: description ILIKE '%xxx' or LOWER(description) LIKE '%xxx'
    match = sql.match(/description ILIKE '%([^%]+)'$/i) || sql.match(/LOWER\(description\) LIKE '%([^%]+)'$/i);
    if (match) return { pattern: match[1], matchType: "ends_with" };

    // Try exact: description ILIKE 'xxx' or LOWER(description) = 'xxx'
    match = sql.match(/description ILIKE '([^%]+)'$/i) || sql.match(/LOWER\(description\) = '(.+)'$/i);
    if (match) return { pattern: match[1], matchType: "exact" };

    // Try regex: regexp_matches(description, 'xxx', 'i') or regexp_matches(LOWER(description), 'xxx')
    match = sql.match(/regexp_matches\(description, '(.+)', 'i'\)$/i) || sql.match(/regexp_matches\(LOWER\(description\), '(.+)'\)$/i);
    if (match) return { pattern: match[1], matchType: "regex" };

    return null;
  }

  function startEdit(rule: TagRule) {
    isCreating = false;
    editingRule = rule;
    formName = rule.name;

    // Get the SQL condition
    const sql = getRuleWhereClause(rule) || "";
    formSqlCondition = sql;

    // Try to parse the SQL into a pattern and match type
    const parsed = parseSqlToPattern(sql);
    if (parsed) {
      formPattern = parsed.pattern;
      formMatchType = parsed.matchType;
      formShowSql = false;
    } else {
      formPattern = "";
      formMatchType = "contains";
      formShowSql = true; // Complex SQL, show SQL mode
    }

    formTags = rule.tags.join(", ");
    formEnabled = rule.enabled;
    formApplyToExisting = false; // Default to false when editing
    testResult = null;
    testError = null;
    lastTestedCondition = null;

    // Auto-preview when entering edit mode
    const effectiveSql = formShowSql ? sql : buildSqlFromPattern(formPattern, formMatchType);
    if (effectiveSql) {
      runTest(effectiveSql);
    }
  }

  function cancelEdit() {
    isCreating = false;
    editingRule = null;
    testResult = null;
    testError = null;
    lastTestedCondition = null;
  }

  function toggleSqlMode() {
    if (!formShowSql) {
      // Switching to SQL mode - initialize with current effective SQL
      formSqlCondition = formEffectiveSql;
    }
    formShowSql = !formShowSql;

    // Immediately run test when switching modes
    lastTestedCondition = null;
    const sql = formShowSql ? formSqlCondition : buildSqlFromPattern(formPattern, formMatchType);
    if (sql) {
      runTest(sql);
    }
  }

  async function runTest(sql: string, force = false) {
    if (!sql.trim()) {
      testResult = null;
      testError = null;
      return;
    }

    if (!force && sql === lastTestedCondition) {
      return;
    }

    testError = null;
    isTesting = true;
    lastTestedCondition = sql;

    try {
      testResult = await testSqlCondition(sql, 200);
    } catch (e) {
      testError = e instanceof Error ? e.message : "Invalid condition";
      // Keep previous results visible so the UI doesn't flash
    } finally {
      isTesting = false;
    }
  }

  function scheduleTest() {
    // Clear any pending debounce
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }

    const sql = formEffectiveSql;
    if (!sql.trim()) {
      testResult = null;
      return;
    }

    // Debounce: wait 300ms before running test
    debounceTimer = setTimeout(() => {
      runTest(sql);
    }, 300);
  }

  function runTestNow() {
    // Clear any pending debounce and run immediately
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    const sql = formEffectiveSql;
    if (sql) {
      runTest(sql, true);
    }
  }

  function handleInput() {
    // Schedule debounced test on any input change
    scheduleTest();
  }

  function handleInputKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      runTestNow();
    }
  }

  function handleMatchTypeChange() {
    // Immediately run test when match type changes (no debounce needed)
    lastTestedCondition = null;
    const sql = formEffectiveSql;
    if (sql) {
      runTest(sql);
    }
  }

  async function handleSave() {
    const tags = formTags
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t);

    const sql = formEffectiveSql;

    if (!formName.trim()) {
      error = "Rule name is required";
      return;
    }

    if (!sql.trim()) {
      error = "Pattern or SQL condition is required";
      return;
    }

    if (tags.length === 0) {
      error = "At least one tag is required";
      return;
    }

    error = null;

    try {
      const now = new Date().toISOString();

      if (editingRule) {
        // Update existing rule
        const updated: TagRule = {
          ...editingRule,
          name: formName.trim(),
          sqlCondition: sql.trim(),
          // Keep existing displayConditions if any, otherwise clear
          displayConditions: editingRule.displayConditions,
          tags,
          enabled: formEnabled,
          updatedAt: now,
        };
        await updateRule(updated);
      } else {
        // Create new rule
        const newRule: TagRule = {
          id: generateRuleId(),
          name: formName.trim(),
          sqlCondition: sql.trim(),
          // No displayConditions for raw SQL input
          tags,
          enabled: formEnabled,
          createdAt: now,
          updatedAt: now,
        };
        await saveRule(newRule);
      }

      // Apply tags to existing matching transactions if requested
      let appliedCount = 0;
      if (formApplyToExisting && tags.length > 0) {
        appliedCount = await applyTagsToMatching(sql.trim(), tags);
      }

      await refreshRules();
      onsaved?.({ appliedToExisting: formApplyToExisting && tags.length > 0, appliedCount });
      cancelEdit();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to save rule";
    }
  }

  async function handleDelete(rule: TagRule) {
    try {
      await deleteRule(rule.id);
      await refreshRules();
      if (editingRule?.id === rule.id) {
        cancelEdit();
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to delete rule";
    }
  }

  async function handleToggleEnabled(rule: TagRule) {
    try {
      await toggleRuleEnabled(rule.id);
      await refreshRules();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to toggle rule";
    }
  }

  async function handleQuickTest(rule: TagRule) {
    const whereClause = getRuleWhereClause(rule);
    if (!whereClause) return;

    startEdit(rule);
    runTest(whereClause);
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (isCreating || editingRule) {
        cancelEdit();
      } else {
        onclose();
      }
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div onkeydown={handleKeyDown}>
  <Modal {open} title={isCreating ? "New Rule" : editingRule ? "Edit Rule" : "Auto-Tag Rules"} {onclose} width={isCreating || editingRule ? "680px" : "500px"} maxHeight="85vh">
    <div class="modal-content">
      {#if error}
        <div class="error-bar">{error}</div>
      {/if}

      {#if isCreating || editingRule}
        <!-- Edit/Create Form - Full featured -->
        <div class="edit-form">
          <div class="form-row">
            <div class="form-group flex-1">
              <label for="rule-name">Rule Name</label>
              <input
                id="rule-name"
                type="text"
                bind:value={formName}
                placeholder="e.g., Tag Starbucks as coffee"
              />
            </div>
            <div class="form-group">
              <label for="rule-enabled">Enabled</label>
              <label class="toggle">
                <input id="rule-enabled" type="checkbox" bind:checked={formEnabled} />
                <span class="toggle-slider"></span>
              </label>
            </div>
          </div>

          <div class="form-group">
            <label for="rule-tags">Tags (comma-separated)</label>
            <TagInput
              id="rule-tags"
              bind:value={formTags}
              {allTags}
              placeholder="coffee, food, dining"
            />
          </div>

          <div class="form-group checkbox-group">
            <label class="checkbox-label">
              <input type="checkbox" bind:checked={formApplyToExisting} />
              <span>Apply to existing transactions</span>
              {#if testResult && formApplyToExisting}
                <span class="match-hint">({testResult.matchingCount} will be tagged)</span>
              {/if}
            </label>
            {#if formApplyToExisting}
              <p class="checkbox-note">Adds tags to matching transactions. Existing tags are kept.</p>
            {/if}
          </div>

          <!-- Pattern or SQL Condition -->
          <div class="form-group">
            <div class="condition-header">
              <label for="condition-input">
                {formShowSql ? "SQL WHERE Clause" : "Match description"}
              </label>
              <button class="toggle-sql" onclick={toggleSqlMode}>
                {formShowSql ? "Simple mode" : "Edit SQL"}
              </button>
            </div>

            {#if formShowSql}
              <div class="sql-editor-block">
                <div class="sql-prefix">
                  <span class="sql-keyword">SELECT</span> * <span class="sql-keyword">FROM</span> transactions <span class="sql-keyword">WHERE</span>
                </div>
                <div class="sql-input-wrapper" class:has-error={testError}>
                  <textarea
                    id="condition-input"
                    bind:value={formSqlCondition}
                    oninput={handleInput}
                    onkeydown={handleInputKeyDown}
                    placeholder="description ILIKE '%merchant%'"
                    rows="3"
                    spellcheck="false"
                    class:error={testError}
                  ></textarea>
                  {#if testError}
                    <div class="sql-error-tooltip" title={testError}>
                      <span class="error-icon">!</span>
                    </div>
                  {/if}
                </div>
              </div>

              <!-- Common filters -->
              <div class="common-filters">
                <span class="filters-label">Insert:</span>
                {#each commonFilters as filter}
                  <button class="filter-chip" onclick={() => insertFilter(filter.sql)}>
                    {filter.label}
                  </button>
                {/each}
              </div>

              <div class="sql-help">
                <details>
                  <summary>Available columns</summary>
                  <div class="columns-list">
                    {#each columns as col}
                      <div class="column-item">
                        <code>{col.name}</code>
                        <span class="column-type">{col.type}</span>
                        <span class="column-example">{col.example}</span>
                      </div>
                    {/each}
                  </div>
                  <p class="sql-tip">
                    Use <code>ILIKE</code> for case-insensitive matching.
                    Use <code>regexp_matches(str, pattern, 'i')</code> for regex.
                  </p>
                </details>
              </div>
            {:else}
              <div class="pattern-input-row">
                <div class="match-type-buttons">
                  {#each Object.entries(matchTypeLabels) as [value, label]}
                    <button
                      class="match-type-btn"
                      class:active={formMatchType === value}
                      onclick={() => { formMatchType = value as MatchType; handleMatchTypeChange(); }}
                    >
                      {label}
                    </button>
                  {/each}
                </div>
                <input
                  id="condition-input"
                  type="text"
                  bind:value={formPattern}
                  oninput={handleInput}
                  onkeydown={handleInputKeyDown}
                  placeholder={formMatchType === "regex" ? "e.g., starbucks|coffee" : "e.g., STARBUCKS"}
                />
              </div>
              <p class="pattern-hint">
                {#if formMatchType === "contains"}
                  Matches any transaction with this text anywhere in the description.
                {:else if formMatchType === "starts_with"}
                  Matches transactions where the description begins with this text.
                {:else if formMatchType === "ends_with"}
                  Matches transactions where the description ends with this text.
                {:else if formMatchType === "exact"}
                  Matches transactions with exactly this description.
                {:else if formMatchType === "regex"}
                  Matches using a regular expression pattern. Use <code>|</code> for OR, <code>.*</code> for wildcards.
                {/if}
                <span class="case-note">Case-insensitive.</span>
              </p>
            {/if}
          </div>

          <!-- Test Results -->
          {#if testResult || isTesting}
            <div class="test-results" class:has-matches={testResult && testResult.matchingCount > 0} class:loading={isTesting}>
              <div class="test-header">
                <span class="test-count">
                  {#if isTesting && !testResult}
                    Testing...
                  {:else if testResult}
                    {testResult.matchingCount} matching transaction{testResult.matchingCount !== 1 ? "s" : ""}
                    {#if isTesting}<span class="updating">updating...</span>{/if}
                  {/if}
                </span>
              </div>
              {#if testResult && testResult.sampleMatches.length > 0}
                <div class="test-samples">
                  {#each testResult.sampleMatches as match}
                    <div class="test-sample">
                      <div class="sample-row-1">
                        <span class="sample-date">{new Date(match.transaction_date).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}</span>
                        <span class="sample-desc">{match.description}</span>
                        <span class="sample-amount" class:negative={match.amount < 0} class:positive={match.amount >= 0}>
                          {formatUserCurrency(match.amount)}
                        </span>
                      </div>
                      <div class="sample-row-2">
                        <span class="sample-account">{match.account_name}</span>
                        {#if match.tags && match.tags.length > 0}
                          <span class="sample-tags">
                            {#each match.tags as tag}
                              <span class="sample-tag">{tag}</span>
                            {/each}
                          </span>
                        {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}

          <div class="form-actions">
            <button class="btn secondary" onclick={cancelEdit}>Cancel</button>
            <button
              class="btn primary"
              onclick={handleSave}
              disabled={!formEffectiveSql.trim()}
            >
              {isCreating ? "Create Rule" : "Save Changes"}
            </button>
          </div>
        </div>
      {:else}
        <!-- Rules List -->
        <div class="rules-header">
          <p class="rules-description">
            Rules automatically tag matching transactions during sync.
          </p>
          {#if rules.length > 0}
            <button class="btn primary small" onclick={startCreate}>+ New Rule</button>
          {/if}
        </div>

        {#if rules.length > 3}
          <div class="search-box">
            <input
              type="text"
              bind:value={searchQuery}
              placeholder="Search rules..."
              class="search-input"
            />
            {#if searchQuery}
              <button class="search-clear" onclick={() => searchQuery = ""}>×</button>
            {/if}
          </div>
        {/if}

        {#if isLoading}
          <div class="loading">Loading...</div>
        {:else if rules.length === 0}
          <div class="empty-state">
            <p>No rules yet</p>
            <button class="btn primary" onclick={startCreate}>Create Rule</button>
          </div>
        {:else if filteredRules.length === 0 && searchQuery}
          <div class="empty-state">
            <p>No rules match "{searchQuery}"</p>
          </div>
        {:else}
          <div class="rules-list">
            {#each filteredRules as rule}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="rule-card"
                class:disabled={!rule.enabled}
                onclick={() => startEdit(rule)}
              >
                <div class="rule-card-header">
                  <span class="rule-name">{rule.name}</span>
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                  <label class="toggle-switch" onclick={(e) => e.stopPropagation()}>
                    <input
                      type="checkbox"
                      checked={rule.enabled}
                      onchange={() => handleToggleEnabled(rule)}
                    />
                    <span class="toggle-track"></span>
                  </label>
                </div>
                <div class="rule-condition">
                  <code>{getRuleWhereClause(rule) || "No condition"}</code>
                </div>
                <div class="rule-card-footer">
                  <div class="rule-tags-list">
                    {#each rule.tags as tag}
                      <span class="rule-tag">{tag}</span>
                    {/each}
                  </div>
                  <button
                    class="delete-btn"
                    onclick={(e) => { e.stopPropagation(); handleDelete(rule); }}
                    title="Delete rule"
                  >
                    Delete
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}

        {#snippet actions()}
          <button class="btn secondary" onclick={onclose}>Close</button>
          <button class="btn primary" onclick={startCreate}>+ New Rule</button>
        {/snippet}
      {/if}
    </div>
  </Modal>
</div>

<style>
  .modal-content {
    padding: var(--spacing-md) var(--spacing-lg);
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
    min-height: 300px;
  }

  .error-bar {
    padding: var(--spacing-sm) var(--spacing-md);
    background: rgba(239, 68, 68, 0.1);
    border-radius: 4px;
    color: var(--accent-danger, #ef4444);
    font-size: 12px;
  }

  /* Loading & Empty states */
  .loading {
    text-align: center;
    padding: var(--spacing-xl);
    color: var(--text-muted);
    font-size: 13px;
  }

  .empty-state {
    text-align: center;
    padding: var(--spacing-xl) var(--spacing-lg);
    color: var(--text-muted);
  }

  .empty-state p {
    margin: 0;
    font-size: 14px;
  }

  .empty-state .btn {
    margin-top: var(--spacing-md);
  }

  /* Rules Header */
  .rules-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--spacing-sm);
    gap: var(--spacing-md);
  }

  .rules-description {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
  }

  .btn.small {
    padding: 6px 12px;
    font-size: 12px;
  }

  /* Search Box */
  .search-box {
    position: relative;
    margin-bottom: var(--spacing-sm);
  }

  .search-input {
    width: 100%;
    padding: 8px 32px 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
  }

  .search-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-clear {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0;
    line-height: 1;
  }

  .search-clear:hover {
    color: var(--text-primary);
  }

  /* Rules List */
  .rules-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .rule-card {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
    padding: var(--spacing-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    width: 100%;
    transition: border-color 0.15s ease, background-color 0.15s ease;
  }

  .rule-card:hover {
    border-color: var(--accent-primary);
    background: var(--bg-tertiary);
  }

  .rule-card.disabled {
    opacity: 0.6;
  }

  .rule-card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--spacing-md);
  }

  .rule-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .rule-condition {
    padding: var(--spacing-xs) var(--spacing-sm);
    background: var(--bg-primary);
    border-radius: 4px;
    overflow: hidden;
  }

  .rule-condition code {
    font-size: 11px;
    font-family: var(--font-mono, monospace);
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    display: block;
  }

  .rule-card-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--spacing-sm);
  }

  .rule-tags-list {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    flex: 1;
    min-width: 0;
  }

  .rule-tag {
    padding: 2px 8px;
    background: var(--accent-primary);
    color: white;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 500;
  }

  .delete-btn {
    padding: 4px 8px;
    background: transparent;
    border: none;
    border-radius: 4px;
    font-size: 11px;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
  }

  .delete-btn:hover {
    background: rgba(239, 68, 68, 0.1);
    color: var(--accent-danger, #ef4444);
  }

  /* Toggle Switch */
  .toggle-switch {
    position: relative;
    display: inline-block;
    width: 36px;
    height: 20px;
    flex-shrink: 0;
    cursor: pointer;
  }

  .toggle-switch input {
    opacity: 0;
    width: 0;
    height: 0;
  }

  .toggle-track {
    position: absolute;
    inset: 0;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 10px;
    transition: background-color 0.2s, border-color 0.2s;
  }

  .toggle-track::before {
    content: "";
    position: absolute;
    height: 14px;
    width: 14px;
    left: 2px;
    bottom: 2px;
    background: var(--text-muted);
    border-radius: 50%;
    transition: transform 0.2s, background-color 0.2s;
  }

  .toggle-switch input:checked + .toggle-track {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .toggle-switch input:checked + .toggle-track::before {
    transform: translateX(16px);
    background: white;
  }

  /* Edit Form */
  .edit-form {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
  }

  .form-row {
    display: flex;
    gap: var(--spacing-md);
    align-items: flex-end;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .form-group.flex-1 {
    flex: 1;
  }

  .form-group label {
    font-size: 12px;
    color: var(--text-muted);
    font-weight: 500;
  }

  .form-group.checkbox-group {
    flex-direction: column;
    align-items: flex-start;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-size: 13px;
    color: var(--text-primary);
  }

  .checkbox-label input[type="checkbox"] {
    width: 16px;
    height: 16px;
    cursor: pointer;
  }

  .match-hint {
    font-size: 12px;
    color: var(--text-muted);
  }

  .checkbox-note {
    margin: 4px 0 0 24px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .condition-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .condition-header label {
    font-size: 12px;
    color: var(--text-muted);
    font-weight: 500;
  }

  .toggle-sql {
    background: none;
    border: none;
    color: var(--accent-primary);
    font-size: 11px;
    cursor: pointer;
    text-decoration: underline;
    padding: 0;
  }

  .toggle-sql:hover {
    color: var(--text-primary);
  }

  .form-group input[type="text"] {
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
  }

  .form-group input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .form-group textarea {
    padding: 10px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: var(--font-mono, monospace);
    resize: vertical;
    min-height: 60px;
  }

  .form-group textarea:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .sql-editor-block {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    overflow: hidden;
  }

  .sql-prefix {
    padding: 10px 12px 6px;
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
  }

  .sql-keyword {
    color: var(--accent-primary);
    font-weight: 600;
  }

  .sql-input-wrapper {
    position: relative;
  }

  .sql-input-wrapper textarea {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-primary);
    border: none;
    border-radius: 0;
    color: var(--text-primary);
    font-size: 12px;
    font-family: var(--font-mono, monospace);
    resize: none;
  }

  .sql-input-wrapper textarea:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .sql-input-wrapper textarea.error {
    border-color: var(--accent-danger, #ef4444);
  }

  .sql-input-wrapper.has-error textarea {
    border-color: var(--accent-danger, #ef4444);
  }

  .sql-error-tooltip {
    position: absolute;
    top: 8px;
    right: 8px;
    cursor: help;
  }

  .error-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: var(--accent-danger, #ef4444);
    color: white;
    font-size: 12px;
    font-weight: 700;
    border-radius: 50%;
  }

  .pattern-input-row {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .match-type-buttons {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }

  .match-type-btn {
    padding: 4px 10px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    font-size: 11px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .match-type-btn:hover {
    background: var(--bg-secondary);
    border-color: var(--text-muted);
  }

  .match-type-btn.active {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: var(--bg-primary);
  }

  .pattern-hint {
    margin: 0;
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .pattern-hint code {
    background: var(--bg-tertiary);
    padding: 1px 4px;
    border-radius: 2px;
    font-family: var(--font-mono, monospace);
    font-size: 10px;
  }

  .case-note {
    opacity: 0.7;
  }

  /* Toggle switch */
  .toggle {
    position: relative;
    display: inline-block;
    width: 40px;
    height: 22px;
    cursor: pointer;
  }

  .toggle input {
    opacity: 0;
    width: 0;
    height: 0;
  }

  .toggle-slider {
    position: absolute;
    inset: 0;
    background: var(--bg-tertiary);
    border-radius: 11px;
    transition: 0.2s;
  }

  .toggle-slider::before {
    content: "";
    position: absolute;
    height: 16px;
    width: 16px;
    left: 3px;
    bottom: 3px;
    background: var(--text-muted);
    border-radius: 50%;
    transition: 0.2s;
  }

  .toggle input:checked + .toggle-slider {
    background: var(--accent-primary);
  }

  .toggle input:checked + .toggle-slider::before {
    transform: translateX(18px);
    background: white;
  }

  .sql-help {
    margin-top: 4px;
  }

  .sql-help details {
    font-size: 12px;
    color: var(--text-muted);
  }

  .sql-help summary {
    cursor: pointer;
    user-select: none;
  }

  .sql-help summary:hover {
    color: var(--text-primary);
  }

  .columns-list {
    margin-top: var(--spacing-sm);
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--spacing-sm);
    background: var(--bg-tertiary);
    border-radius: 4px;
  }

  .column-item {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    font-size: 11px;
  }

  .column-item code {
    background: var(--bg-primary);
    padding: 2px 6px;
    border-radius: 3px;
    font-family: var(--font-mono, monospace);
    color: var(--accent-primary);
  }

  .column-type {
    color: var(--text-muted);
    opacity: 0.7;
    min-width: 60px;
  }

  .column-example {
    color: var(--text-muted);
    font-family: var(--font-mono, monospace);
    font-size: 10px;
  }

  .sql-tip {
    margin: var(--spacing-sm) 0 0;
    font-size: 11px;
    color: var(--text-muted);
  }

  .sql-tip code {
    background: var(--bg-tertiary);
    padding: 1px 4px;
    border-radius: 2px;
    font-family: var(--font-mono, monospace);
  }

  .common-filters {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: var(--spacing-sm);
  }

  .filters-label {
    font-size: 11px;
    color: var(--text-muted);
  }

  .filter-chip {
    padding: 3px 8px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    font-size: 10px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .filter-chip:hover {
    background: var(--bg-secondary);
    border-color: var(--accent-primary);
    color: var(--accent-primary);
  }

  .test-results {
    padding: var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 6px;
    border: 1px solid var(--border-primary);
  }

  .test-results.loading {
    opacity: 0.8;
  }

  .updating {
    font-size: 11px;
    font-weight: 400;
    color: var(--text-muted);
    margin-left: var(--spacing-sm);
  }

  .test-results.has-matches {
    border-color: var(--accent-success, #22c55e);
  }

  .test-header {
    margin-bottom: var(--spacing-sm);
  }

  .test-count {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .test-samples {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 300px;
    overflow-y: auto;
  }

  .test-sample {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 8px;
    background: var(--bg-primary);
    border-radius: 4px;
    font-size: 12px;
  }

  .sample-row-1 {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
  }

  .sample-row-2 {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding-left: 52px; /* Align with description after date */
  }

  .sample-date {
    color: var(--text-muted);
    font-size: 11px;
    min-width: 44px;
    flex-shrink: 0;
  }

  .sample-desc {
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .sample-amount {
    color: var(--text-muted);
    font-family: var(--font-mono, monospace);
    flex-shrink: 0;
  }

  .sample-amount.negative {
    color: var(--accent-danger);
  }

  .sample-amount.positive {
    color: var(--accent-success, #22c55e);
  }

  .sample-account {
    color: var(--text-muted);
    font-size: 11px;
  }

  .sample-tags {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }

  .sample-tag {
    padding: 1px 6px;
    background: var(--bg-tertiary);
    border-radius: 3px;
    font-size: 10px;
    color: var(--text-secondary);
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-sm);
    padding-top: var(--spacing-md);
    border-top: 1px solid var(--border-primary);
  }

  /* Buttons */
  .btn {
    padding: 8px 16px;
    border-radius: 4px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    border: none;
  }

  .btn.primary {
    background: var(--accent-primary);
    color: var(--bg-primary);
  }

  .btn.primary:hover {
    opacity: 0.9;
  }

  .btn.primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn.secondary {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border-primary);
  }


</style>
