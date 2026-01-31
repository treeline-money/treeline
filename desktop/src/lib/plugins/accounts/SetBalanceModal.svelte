<script lang="ts">
  /**
   * SetBalanceModal - Modal for setting/updating account balance
   * Used both standalone (from account menu) and embedded in post-import flow
   */
  import { Modal, formatUserCurrency, getUserCurrencySymbol } from "../../shared";
  import { executeQuery } from "../../sdk";

  interface Props {
    open: boolean;
    accountId: string;
    accountName: string;
    currentBalance?: number | null;
    currentBalanceDate?: string | null;
    onclose: () => void;
    onsave: (balance: number, date: string) => void;
  }

  let {
    open,
    accountId,
    accountName,
    currentBalance = null,
    currentBalanceDate = null,
    onclose,
    onsave,
  }: Props = $props();

  // Form state
  let balanceInput = $state("");
  let dateInput = $state("");
  let isSaving = $state(false);
  let error = $state<string | null>(null);

  // Reset form when modal opens
  $effect(() => {
    if (open) {
      // Pre-fill with current balance if available
      if (currentBalance !== null && currentBalance !== undefined) {
        balanceInput = Math.abs(currentBalance).toFixed(2);
      } else {
        balanceInput = "";
      }
      // Default to today
      dateInput = new Date().toISOString().split("T")[0];
      error = null;
      isSaving = false;
    }
  });

  // Derived: is the form valid?
  let isValid = $derived(() => {
    const balance = parseFloat(balanceInput);
    return !isNaN(balance) && dateInput.length === 10;
  });

  // Derived: has existing balance?
  let hasExistingBalance = $derived(currentBalance !== null && currentBalance !== undefined);

  async function handleSave() {
    const balance = parseFloat(balanceInput);
    if (isNaN(balance)) {
      error = "Please enter a valid balance";
      return;
    }

    isSaving = true;
    error = null;

    try {
      // Insert balance snapshot
      const snapshotId = crypto.randomUUID();
      const snapshotTime = `${dateInput}T23:59:59`;
      const now = new Date().toISOString();

      await executeQuery(
        `INSERT INTO sys_balance_snapshots (snapshot_id, account_id, balance, snapshot_time, created_at, updated_at, source)
         VALUES ('${snapshotId}', '${accountId}', ${balance}, '${snapshotTime}', '${now}', '${now}', 'manual')`,
        { readonly: false }
      );

      onsave(balance, dateInput);
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to save balance";
      isSaving = false;
    }
  }
</script>

<Modal {open} title="Add Balance" onclose={onclose} width="400px">
  <div class="balance-body">
    {#if error}
      <div class="error-message">{error}</div>
    {/if}

    <p class="balance-intro">
      Enter a balance from your bank statement or account summary.
    </p>

    {#if hasExistingBalance}
      <div class="current-balance">
        <span class="current-label">Current balance:</span>
        <span class="current-value">{formatUserCurrency(Math.abs(currentBalance!))}</span>
        {#if currentBalanceDate}
          <span class="current-date">as of {currentBalanceDate}</span>
        {/if}
      </div>
    {/if}

    <div class="form-group">
      <label for="balance-input">Balance</label>
      <div class="input-with-prefix">
        <span class="input-prefix">{getUserCurrencySymbol()}</span>
        <input
          id="balance-input"
          type="number"
          step="0.01"
          placeholder="0.00"
          bind:value={balanceInput}
        />
      </div>
    </div>

    <div class="form-group">
      <label for="date-input">As of date</label>
      <input
        id="date-input"
        type="date"
        bind:value={dateInput}
      />
      <span class="form-hint">Usually today, or the statement date</span>
    </div>
  </div>

  {#snippet actions()}
    <button class="btn secondary" onclick={onclose} disabled={isSaving}>Cancel</button>
    <button class="btn primary" onclick={handleSave} disabled={!isValid() || isSaving}>
      {isSaving ? "Saving..." : "Save Balance"}
    </button>
  {/snippet}
</Modal>

<style>
  .balance-body {
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

  .balance-intro {
    margin: 0;
    color: var(--text-muted);
    font-size: 13px;
  }

  .current-balance {
    display: flex;
    align-items: baseline;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 4px;
    font-size: 13px;
  }

  .current-label {
    color: var(--text-muted);
  }

  .current-value {
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--text-primary);
  }

  .current-date {
    color: var(--text-muted);
    font-size: 12px;
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
</style>
