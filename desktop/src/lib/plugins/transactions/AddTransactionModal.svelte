<script lang="ts">
  /**
   * AddTransactionModal - Modal for manually adding a new transaction
   */
  import { Modal } from "../../shared";
  import type { AccountInfo } from "./types";

  type TransactionType = "out" | "in";

  interface Props {
    open: boolean;
    accounts: AccountInfo[];
    onclose: () => void;
    onsave: (data: {
      description: string;
      amount: number;
      date: string;
      accountId: string;
      tags: string[];
    }) => void;
  }

  let { open, accounts, onclose, onsave }: Props = $props();

  // Form state
  let descInput = $state("");
  let amountInput = $state("");
  let transactionType = $state<TransactionType>("out"); // Default to money out (most common)
  let dateInput = $state(new Date().toISOString().split("T")[0]);
  let accountId = $state("");
  let tagsInput = $state("");
  let error = $state<string | null>(null);

  // Reset form when modal opens
  $effect(() => {
    if (open) {
      descInput = "";
      amountInput = "";
      transactionType = "out"; // Always default to money out
      dateInput = new Date().toISOString().split("T")[0];
      accountId = "";
      tagsInput = "";
      error = null;
    }
  });

  function handleSave() {
    error = null;

    // Validate inputs
    const desc = descInput.trim();
    if (!desc) {
      error = "Description is required";
      return;
    }

    const rawAmount = parseFloat(amountInput);
    if (isNaN(rawAmount) || rawAmount < 0) {
      error = "Invalid amount (enter a positive number)";
      return;
    }

    if (rawAmount === 0) {
      error = "Amount cannot be zero";
      return;
    }

    if (!accountId) {
      error = "Please select an account";
      return;
    }

    if (!/^\d{4}-\d{2}-\d{2}$/.test(dateInput)) {
      error = "Date must be in YYYY-MM-DD format";
      return;
    }

    const tags = tagsInput
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t);

    // Apply sign based on transaction type: expenses are negative, income is positive
    const amount = transactionType === "out" ? -Math.abs(rawAmount) : Math.abs(rawAmount);

    onsave({
      description: desc,
      amount,
      date: dateInput,
      accountId,
      tags,
    });
  }
</script>

<Modal {open} title="Add Transaction" {onclose}>
  <div class="modal-form">
    {#if error}
      <div class="form-error">{error}</div>
    {/if}

    <div class="form-row">
      <div class="form-group flex-2">
        <label for="add-desc">Description</label>
        <input
          id="add-desc"
          type="text"
          bind:value={descInput}
          placeholder="Transaction description"
        />
      </div>
    </div>

    <div class="form-row">
      <div class="form-group">
        <label for="add-date">Date</label>
        <input id="add-date" type="date" bind:value={dateInput} />
      </div>
      <div class="form-group">
        <label for="add-type">Type</label>
        <div class="type-toggle">
          <button
            type="button"
            class="toggle-btn"
            class:active={transactionType === "out"}
            onclick={() => transactionType = "out"}
          >
            Out
          </button>
          <button
            type="button"
            class="toggle-btn"
            class:active={transactionType === "in"}
            onclick={() => transactionType = "in"}
          >
            In
          </button>
        </div>
      </div>
      <div class="form-group">
        <label for="add-amount">Amount</label>
        <div class="amount-wrapper">
          <span class="amount-sign">{transactionType === "out" ? "−" : "+"}</span>
          <input
            id="add-amount"
            type="text"
            bind:value={amountInput}
            placeholder="0.00"
            class="amount-input"
          />
        </div>
      </div>
    </div>

    <div class="form-group">
      <label for="add-account">Account</label>
      <select id="add-account" bind:value={accountId}>
        <option value="">Select an account...</option>
        {#each accounts as account}
          <option value={account.id}>{account.name}</option>
        {/each}
      </select>
    </div>

    <div class="form-group">
      <label for="add-tags">Tags (comma-separated)</label>
      <input
        id="add-tags"
        type="text"
        bind:value={tagsInput}
        placeholder="e.g., groceries, food"
      />
    </div>
  </div>

  {#snippet actions()}
    <button class="btn secondary" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={handleSave}>Add Transaction</button>
  {/snippet}
</Modal>

<style>
  .modal-form {
    padding: var(--spacing-md) var(--spacing-lg);
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
  }

  .form-error {
    padding: var(--spacing-sm) var(--spacing-md);
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid var(--accent-danger, #ef4444);
    border-radius: 4px;
    color: var(--accent-danger, #ef4444);
    font-size: 13px;
  }

  .form-row {
    display: flex;
    gap: var(--spacing-md);
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
  }

  .form-group.flex-2 {
    flex: 2;
  }

  .form-group label {
    font-size: 12px;
    color: var(--text-muted);
    font-weight: 500;
  }

  .form-group input {
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
  }

  .form-group select {
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    appearance: none;
    -webkit-appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%239ca3af' d='M2 4l4 4 4-4'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 10px center;
    padding-right: 32px;
    cursor: pointer;
  }

  .form-group input:focus,
  .form-group select:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .type-toggle {
    display: flex;
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    overflow: hidden;
  }

  .toggle-btn {
    flex: 1;
    padding: 8px 12px;
    background: var(--bg-primary);
    border: none;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .toggle-btn:first-child {
    border-right: 1px solid var(--border-primary);
  }

  .toggle-btn:hover:not(.active) {
    background: var(--bg-secondary);
  }

  .toggle-btn.active {
    background: var(--accent-primary);
    color: white;
    font-weight: 500;
  }

  .amount-wrapper {
    display: flex;
    align-items: center;
    padding: 0 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
  }

  .amount-wrapper:focus-within {
    border-color: var(--accent-primary);
  }

  .amount-sign {
    font-family: var(--font-mono);
    font-size: 14px;
    font-weight: 600;
    color: var(--text-muted);
    margin-right: 4px;
  }

  .amount-input {
    flex: 1;
    font-family: var(--font-mono);
    text-align: right;
    border: none !important;
    padding: 8px 0 !important;
    background: transparent !important;
  }

  .amount-input:focus {
    outline: none;
  }

</style>
