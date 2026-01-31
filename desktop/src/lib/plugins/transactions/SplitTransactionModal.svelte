<script lang="ts">
  /**
   * SplitTransactionModal - Modal for splitting a transaction into multiple parts
   */
  import { Modal, formatUserCurrency } from "../../shared";
  import type { Transaction, SplitAmount } from "./types";

  interface Props {
    open: boolean;
    transaction: Transaction | null;
    onclose: () => void;
    onsplit: (amounts: SplitAmount[]) => void;
  }

  let { open, transaction, onclose, onsplit }: Props = $props();

  // Split amounts state
  let splitAmounts = $state<SplitAmount[]>([
    { description: "", amount: "" },
    { description: "", amount: "" },
  ]);

  // Reset when modal opens with a new transaction
  $effect(() => {
    if (open && transaction) {
      splitAmounts = [
        { description: transaction.description || "", amount: "" },
        { description: "", amount: "" },
      ];
    }
  });

  function addSplitRow() {
    splitAmounts = [...splitAmounts, { description: "", amount: "" }];
  }

  function removeSplitRow(index: number) {
    if (splitAmounts.length > 2) {
      splitAmounts = splitAmounts.filter((_, i) => i !== index);
    }
  }

  function handleSplit() {
    onsplit(splitAmounts);
  }

  // Computed validation
  let splitTotal = $derived(
    splitAmounts.reduce((sum, s) => sum + (parseFloat(s.amount) || 0), 0)
  );
  let remaining = $derived((transaction?.amount ?? 0) - splitTotal);
  let isValid = $derived(Math.abs(remaining) <= 0.01);
</script>

<Modal {open} title="Split Transaction" {onclose} width="500px">
  {#if transaction}
    <div class="split-body">
      <div class="txn-preview">
        <div class="txn-preview-desc">{transaction.description}</div>
        <div class="txn-preview-amount" class:negative={transaction.amount < 0}>
          Original: {formatUserCurrency(transaction.amount)}
        </div>
      </div>

      <div class="split-rows">
        {#each splitAmounts as split, i}
          <div class="split-row">
            <input
              type="text"
              class="split-desc"
              bind:value={split.description}
              placeholder="Description"
            />
            <input
              type="text"
              class="split-amount"
              bind:value={split.amount}
              placeholder="0.00"
            />
            {#if splitAmounts.length > 2}
              <button class="btn-icon" onclick={() => removeSplitRow(i)}>×</button
              >
            {/if}
          </div>
        {/each}
      </div>

      <button class="btn secondary add-split-btn" onclick={addSplitRow}>
        + Add Row
      </button>

      <div class="split-summary" class:error={!isValid}>
        Total: {formatUserCurrency(splitTotal)} | Remaining: {formatUserCurrency(remaining)}
      </div>
    </div>
  {/if}

  {#snippet actions()}
    <button class="btn secondary" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={handleSplit} disabled={!isValid}>
      Split
    </button>
  {/snippet}
</Modal>

<style>
  .split-body {
    padding: var(--spacing-md) var(--spacing-lg);
  }

  .txn-preview {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 4px;
    margin-bottom: var(--spacing-md);
  }

  .txn-preview-desc {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-primary);
    font-size: 13px;
  }

  .txn-preview-amount {
    flex-shrink: 0;
    font-family: var(--font-mono);
    font-size: 13px;
    font-weight: 600;
    margin-left: var(--spacing-md);
  }

  .txn-preview-amount.negative {
    color: var(--accent-danger, #ef4444);
  }

  .split-rows {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
    margin-bottom: var(--spacing-md);
  }

  .split-row {
    display: flex;
    gap: var(--spacing-sm);
    align-items: center;
  }

  .split-desc {
    flex: 2;
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
  }

  .split-desc:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .split-amount {
    width: 100px;
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: var(--font-mono);
    text-align: right;
  }

  .split-amount:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .btn-icon {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-muted);
    font-size: 16px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .btn-icon:hover {
    color: var(--accent-danger, #ef4444);
    border-color: var(--accent-danger, #ef4444);
  }

  .add-split-btn {
    margin-bottom: var(--spacing-md);
  }

  .split-summary {
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 4px;
    font-size: 13px;
    font-family: var(--font-mono);
    color: var(--text-muted);
  }

  .split-summary.error {
    background: rgba(239, 68, 68, 0.1);
    color: var(--accent-danger, #ef4444);
  }
</style>
