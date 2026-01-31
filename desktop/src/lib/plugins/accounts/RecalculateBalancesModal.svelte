<script lang="ts">
  /**
   * RecalculateBalancesModal - Modal for previewing and applying balance backfill
   *
   * Flow:
   * 1. Enter a known balance and date (with optional date range)
   * 2. Preview calculated end-of-day balances
   * 3. Apply - replaces all existing snapshots in range
   */
  import { Modal, formatUserCurrency, getUserCurrencySymbol } from "../../shared";
  import { backfillPreview, backfillExecute, type BalanceSnapshotPreview } from "../../sdk";
  import { showToast } from "../../sdk";

  interface Props {
    open: boolean;
    accountId: string;
    accountName: string;
    onclose: () => void;
    onsuccess: () => void;
  }

  let {
    open,
    accountId,
    accountName,
    onclose,
    onsuccess,
  }: Props = $props();

  // Steps: "input" | "preview" | "applying"
  type Step = "input" | "preview" | "applying";
  let step = $state<Step>("input");

  // Form state
  let balanceInput = $state("");
  let knownDate = $state("");
  let useDateRange = $state(false);
  let startDate = $state("");
  let endDate = $state("");
  let error = $state<string | null>(null);

  // Preview state
  let previews = $state<BalanceSnapshotPreview[]>([]);
  let isLoadingPreview = $state(false);

  // Tooltip state
  let hoveredSnapshot = $state<BalanceSnapshotPreview | null>(null);
  let tooltipPosition = $state({ x: 0, y: 0 });

  function handleMouseEnter(e: MouseEvent, snapshot: BalanceSnapshotPreview) {
    if (snapshot.transactions && snapshot.transactions.length > 0) {
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      tooltipPosition = { x: rect.left, y: rect.bottom + 4 };
      hoveredSnapshot = snapshot;
    }
  }

  function handleMouseLeave() {
    hoveredSnapshot = null;
  }

  // Summary counts
  let newCount = $derived(previews.filter(p => p.is_new).length);
  let replaceCount = $derived(previews.filter(p => !p.is_new).length);

  // Group replaced by source for summary
  let replaceBySources = $derived(() => {
    const sources: Record<string, number> = {};
    for (const p of previews) {
      if (!p.is_new && p.existing_source) {
        sources[p.existing_source] = (sources[p.existing_source] || 0) + 1;
      }
    }
    return sources;
  });

  // Reset form when modal opens
  $effect(() => {
    if (open) {
      step = "input";
      balanceInput = "";
      knownDate = new Date().toISOString().split("T")[0];
      useDateRange = false;
      startDate = "";
      endDate = "";
      error = null;
      previews = [];
      isLoadingPreview = false;
      hoveredSnapshot = null;
    }
  });

  // Derived: is the form valid?
  let isValid = $derived(() => {
    const balance = parseFloat(balanceInput || "");
    if (isNaN(balance) || knownDate.length !== 10) return false;
    if (useDateRange) {
      if (startDate.length !== 10 || endDate.length !== 10) return false;
      if (startDate > endDate) return false;
    }
    return true;
  });

  // Sanitize balance input to only allow numbers and decimal point
  function sanitizeBalanceInput(e: Event) {
    const input = e.target as HTMLInputElement;
    input.value = input.value.replace(/[^\d.-]/g, "");
    balanceInput = input.value;
  }

  async function handlePreview() {
    const balance = parseFloat(balanceInput || "");
    if (isNaN(balance)) {
      error = "Please enter a valid number (e.g., 1234.56)";
      return;
    }

    isLoadingPreview = true;
    error = null;

    try {
      const start = useDateRange ? startDate : undefined;
      const end = useDateRange ? endDate : undefined;

      previews = await backfillPreview(accountId, balance, knownDate, start, end);

      if (previews.length === 0) {
        error = "No transactions or existing snapshots found in the selected date range";
        return;
      }

      step = "preview";
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to preview balances";
    } finally {
      isLoadingPreview = false;
    }
  }

  async function handleApply() {
    const balance = parseFloat(balanceInput);
    if (isNaN(balance)) return;

    step = "applying";
    error = null;

    try {
      const start = useDateRange ? startDate : undefined;
      const end = useDateRange ? endDate : undefined;

      const result = await backfillExecute(accountId, balance, knownDate, start, end);

      // Show success toast
      const messages: string[] = [];
      if (result.snapshots_created > 0) {
        messages.push(`${result.snapshots_created} created`);
      }
      if (result.snapshots_updated > 0) {
        messages.push(`${result.snapshots_updated} replaced`);
      }

      showToast({
        type: "success",
        title: `Balance history updated: ${messages.join(", ") || "no changes"}`,
      });

      onsuccess();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to apply balance backfill";
      step = "preview";
    }
  }

  function handleBack() {
    step = "input";
    error = null;
  }

  function formatSource(source: string | null): string {
    if (!source) return "";
    const labels: Record<string, string> = {
      sync: "sync",
      manual: "manual",
      backfill: "calculated",
      import: "imported",
    };
    return labels[source] || source;
  }
</script>

<Modal
  {open}
  title={step === "input" ? "Recalculate Balance History" : step === "preview" ? "Preview Balance History" : "Applying..."}
  onclose={onclose}
  width="720px"
>
  <div class="modal-body">
    {#if error}
      <div class="error-message">{error}</div>
    {/if}

    {#if step === "input"}
      <p class="intro">
        Enter a known balance (e.g., from your bank statement) and Treeline will calculate
        historical end-of-day balances based on your transaction history.
      </p>

      <div class="form-group">
        <label for="balance-input">Known Balance</label>
        <div class="input-with-prefix">
          <span class="input-prefix">{getUserCurrencySymbol()}</span>
          <input
            id="balance-input"
            type="text"
            inputmode="decimal"
            placeholder="0.00"
            bind:value={balanceInput}
            oninput={sanitizeBalanceInput}
          />
        </div>
        <span class="form-hint">The balance at end of day on the selected date</span>
      </div>

      <div class="form-group">
        <label for="known-date">Balance Date (Anchor Point)</label>
        <input
          id="known-date"
          type="date"
          bind:value={knownDate}
        />
        <span class="form-hint">The date when you know the balance was this amount</span>
      </div>

      <div class="options-section">
        <label class="checkbox-label">
          <input type="checkbox" bind:checked={useDateRange} />
          <span class="checkbox-text">
            <strong>Limit to date range</strong>
            <span class="checkbox-hint">Only calculate balances within a specific range</span>
          </span>
        </label>
      </div>

      {#if useDateRange}
        <div class="date-range-row">
          <div class="form-group">
            <label for="start-date">From</label>
            <input
              id="start-date"
              type="date"
              bind:value={startDate}
            />
          </div>
          <div class="form-group">
            <label for="end-date">To</label>
            <input
              id="end-date"
              type="date"
              bind:value={endDate}
            />
          </div>
        </div>
      {/if}

    {:else if step === "preview"}
      <div class="preview-summary">
        <div class="summary-item">
          <span class="summary-value new">{newCount}</span>
          <span class="summary-label">new</span>
        </div>
        {#if replaceCount > 0}
          <div class="summary-item">
            <span class="summary-value replace">{replaceCount}</span>
            <span class="summary-label">
              replaced
              {#if Object.keys(replaceBySources()).length > 0}
                <span class="source-breakdown">
                  ({Object.entries(replaceBySources()).map(([s, n]) => `${n} ${formatSource(s)}`).join(", ")})
                </span>
              {/if}
            </span>
          </div>
        {/if}
      </div>

      <div class="preview-table-container">
        <table class="preview-table">
          <thead>
            <tr>
              <th>Date</th>
              <th class="change-col">Change</th>
              <th class="balance-col">Calculated</th>
              <th class="balance-col">Existing</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {#each previews as snapshot}
              <tr class:new={snapshot.is_new} class:replace={!snapshot.is_new}>
                <td class="date-cell">{snapshot.date}</td>
                <td
                  class="change-cell"
                  class:positive={snapshot.daily_change > 0}
                  class:negative={snapshot.daily_change < 0}
                  onmouseenter={(e) => handleMouseEnter(e, snapshot)}
                  onmouseleave={handleMouseLeave}
                >
                  {#if snapshot.daily_change !== 0}
                    <span class="change-value" class:has-transactions={snapshot.transactions && snapshot.transactions.length > 0}>
                      {snapshot.daily_change > 0 ? "+" : ""}{formatUserCurrency(snapshot.daily_change)}
                      {#if snapshot.transactions && snapshot.transactions.length > 0}
                        <span class="tx-count">({snapshot.transactions.length})</span>
                      {/if}
                    </span>
                  {:else}
                    <span class="no-change">—</span>
                  {/if}
                </td>
                <td class="balance-cell">{formatUserCurrency(snapshot.balance)}</td>
                <td class="balance-cell existing">
                  {#if snapshot.existing_balance !== null}
                    {formatUserCurrency(snapshot.existing_balance)}
                  {:else}
                    <span class="no-existing">—</span>
                  {/if}
                </td>
                <td class="status-cell">
                  {#if snapshot.is_new}
                    <span class="badge new">New</span>
                  {:else}
                    <span class="badge replace">Replace</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      {#if hoveredSnapshot && hoveredSnapshot.transactions && hoveredSnapshot.transactions.length > 0}
        <div class="tx-tooltip" style="left: {tooltipPosition.x}px; top: {tooltipPosition.y}px;">
          <div class="tx-tooltip-header">Transactions</div>
          {#each hoveredSnapshot.transactions as tx}
            <div class="tx-tooltip-row">
              <span class="tx-desc">{tx.description || "(no description)"}</span>
              <span class="tx-amount" class:positive={tx.amount > 0} class:negative={tx.amount < 0}>
                {tx.amount > 0 ? "+" : ""}{formatUserCurrency(tx.amount)}
              </span>
            </div>
          {/each}
        </div>
      {/if}

    {:else if step === "applying"}
      <div class="applying-state">
        <div class="spinner"></div>
        <p>Applying balance history...</p>
      </div>
    {/if}
  </div>

  {#snippet actions()}
    {#if step === "input"}
      <button class="btn secondary" onclick={onclose}>Cancel</button>
      <button class="btn primary" onclick={handlePreview} disabled={!isValid() || isLoadingPreview}>
        {isLoadingPreview ? "Loading..." : "Preview"}
      </button>
    {:else if step === "preview"}
      <button class="btn secondary" onclick={handleBack}>Back</button>
      <button class="btn primary" onclick={handleApply}>
        Apply {newCount + replaceCount} Changes
      </button>
    {:else}
      <button class="btn secondary" disabled>Cancel</button>
      <button class="btn primary" disabled>Applying...</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .modal-body {
    padding: var(--spacing-lg);
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
  }

  .error-message {
    padding: var(--spacing-sm) var(--spacing-md);
    background: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.4);
    border-radius: 4px;
    color: var(--accent-danger, #ef4444);
    font-size: 13px;
  }

  .intro {
    margin: 0;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.5;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xs);
  }

  .form-group label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .input-with-prefix {
    display: flex;
    align-items: center;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    overflow: hidden;
  }

  .input-with-prefix:focus-within {
    border-color: var(--accent-primary);
  }

  .input-prefix {
    padding: 8px 12px;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    font-size: 14px;
    border-right: 1px solid var(--border-primary);
  }

  .input-with-prefix input {
    flex: 1;
    padding: 8px 12px;
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 14px;
    font-family: var(--font-mono);
  }

  .input-with-prefix input:focus {
    outline: none;
  }

  .form-group input[type="date"] {
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 14px;
  }

  .form-group input[type="date"]:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .form-hint {
    font-size: 11px;
    color: var(--text-muted);
  }

  .date-range-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--spacing-md);
  }

  /* Preview section */
  .preview-summary {
    display: flex;
    gap: var(--spacing-lg);
    padding: var(--spacing-md);
    background: var(--bg-secondary);
    border-radius: 6px;
    flex-wrap: wrap;
  }

  .summary-item {
    display: flex;
    align-items: baseline;
    gap: var(--spacing-xs);
  }

  .summary-value {
    font-size: 20px;
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .summary-value.new {
    color: var(--accent-success);
  }

  .summary-value.replace {
    color: var(--accent-warning);
  }

  .summary-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .source-breakdown {
    font-size: 11px;
    color: var(--text-muted);
  }

  .preview-table-container {
    max-height: 300px;
    overflow-y: auto;
    border: 1px solid var(--border-primary);
    border-radius: 4px;
  }

  .preview-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }

  .preview-table th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--bg-tertiary);
    text-align: left;
    padding: var(--spacing-sm) var(--spacing-md);
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    border-bottom: 1px solid var(--border-primary);
  }

  .preview-table th.balance-col,
  .preview-table th.change-col {
    text-align: right;
  }

  .preview-table td {
    padding: var(--spacing-xs) var(--spacing-md);
    border-bottom: 1px solid var(--border-primary);
  }

  .preview-table tbody tr:last-child td {
    border-bottom: none;
  }

  .preview-table tbody tr.new {
    background: color-mix(in srgb, var(--accent-success) 5%, transparent);
  }

  .preview-table tbody tr.replace {
    background: color-mix(in srgb, var(--accent-warning) 8%, transparent);
  }

  .date-cell {
    font-family: var(--font-mono);
    color: var(--text-secondary);
  }

  .change-cell {
    text-align: right;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .change-cell.positive {
    color: var(--accent-success);
  }

  .change-cell.negative {
    color: var(--accent-danger);
  }

  .no-change {
    color: var(--text-muted);
    opacity: 0.4;
  }

  .change-value {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .change-value.has-transactions {
    cursor: help;
  }

  .tx-count {
    font-size: 10px;
    color: var(--text-muted);
  }

  .tx-tooltip {
    position: fixed;
    z-index: 1000;
    min-width: 280px;
    max-width: 400px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    padding: var(--spacing-sm);
    pointer-events: none;
  }

  .tx-tooltip-header {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    padding-bottom: var(--spacing-xs);
    margin-bottom: var(--spacing-xs);
    border-bottom: 1px solid var(--border-primary);
  }

  .tx-tooltip-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--spacing-sm);
    padding: 2px 0;
    font-size: 12px;
  }

  .tx-desc {
    color: var(--text-secondary);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }

  .tx-amount {
    font-family: var(--font-mono);
    font-size: 11px;
    white-space: nowrap;
  }

  .tx-amount.positive {
    color: var(--accent-success);
  }

  .tx-amount.negative {
    color: var(--accent-danger);
  }

  .balance-cell {
    text-align: right;
    font-family: var(--font-mono);
    font-weight: 500;
  }

  .balance-cell.existing {
    color: var(--text-muted);
    font-weight: 400;
  }

  .no-existing {
    color: var(--text-muted);
    opacity: 0.5;
  }

  .status-cell {
    text-align: right;
  }

  .badge {
    display: inline-block;
    padding: 2px 6px;
    border-radius: 3px;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
  }

  .badge.new {
    background: color-mix(in srgb, var(--accent-success) 15%, transparent);
    color: var(--accent-success);
  }

  .badge.replace {
    background: color-mix(in srgb, var(--accent-warning) 15%, transparent);
    color: var(--accent-warning);
  }

  .options-section {
    padding-top: var(--spacing-sm);
    border-top: 1px solid var(--border-primary);
  }

  .checkbox-label {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-sm);
    cursor: pointer;
  }

  .checkbox-label input {
    margin-top: 3px;
  }

  .checkbox-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .checkbox-text strong {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .checkbox-hint {
    font-size: 11px;
    color: var(--text-muted);
  }

  /* Applying state */
  .applying-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--spacing-md);
    padding: var(--spacing-xl);
  }

  .applying-state p {
    margin: 0;
    color: var(--text-muted);
    font-size: 14px;
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--border-primary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
