<script lang="ts">
  /**
   * EditTransactionModal - Modal for editing transaction details and tags
   */
  import { Modal } from "../../shared";
  import type { Transaction, TagSuggestion } from "./types";

  interface Props {
    open: boolean;
    transaction: Transaction | null;
    suggestions: TagSuggestion[];
    onclose: () => void;
    onsave: (data: {
      description: string;
      amount: number;
      date: string;
      tags: string[];
    }) => void;
  }

  let { open, transaction, suggestions, onclose, onsave }: Props = $props();

  // Form state
  let tagInput = $state("");
  let descInput = $state("");
  let amountInput = $state("");
  let dateInput = $state("");
  let error = $state<string | null>(null);
  let inputEl: HTMLInputElement | null = null;

  // Reset form when modal opens with a transaction
  $effect(() => {
    if (open && transaction) {
      tagInput = transaction.tags.join(", ");
      descInput = transaction.description;
      amountInput = transaction.amount.toString();
      dateInput = transaction.transaction_date;
      error = null;
      // Focus the description input
      setTimeout(() => inputEl?.focus(), 10);
    }
  });

  function handleSave() {
    error = null;

    const newTags = tagInput
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t);
    const newDesc = descInput.trim();
    const newAmount = parseFloat(amountInput);
    const newDate = dateInput.trim();

    // Validate amount
    if (isNaN(newAmount)) {
      error = "Invalid amount";
      return;
    }

    // Validate date format
    if (!/^\d{4}-\d{2}-\d{2}$/.test(newDate)) {
      error = "Date must be in YYYY-MM-DD format";
      return;
    }

    onsave({
      description: newDesc,
      amount: newAmount,
      date: newDate,
      tags: newTags,
    });
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSave();
    }
  }

  function addSuggestedTag(tag: string) {
    const current = tagInput.trim();
    tagInput = current ? `${current}, ${tag}` : tag;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div onkeydown={handleKeyDown}>
  <Modal {open} title="Edit Transaction" {onclose}>
    {#if transaction?.parent_transaction_id}
      <div class="split-notice">
        <span class="split-badge">⑂</span> Part of a split transaction
      </div>
    {/if}

    <div class="modal-form">
      {#if error}
        <div class="form-error">{error}</div>
      {/if}

      <div class="form-row">
        <div class="form-group flex-2">
          <label for="modal-desc">Description</label>
          <input
            id="modal-desc"
            type="text"
            bind:this={inputEl}
            bind:value={descInput}
            placeholder="Transaction description"
          />
        </div>
      </div>

      <div class="form-row">
        <div class="form-group">
          <label for="modal-date">Date</label>
          <input id="modal-date" type="date" bind:value={dateInput} />
        </div>
        <div class="form-group">
          <label for="modal-amount">Amount</label>
          <input
            id="modal-amount"
            type="text"
            bind:value={amountInput}
            placeholder="0.00"
            class="amount-input"
          />
        </div>
      </div>

      <div class="form-group">
        <label for="modal-tags">Tags (comma-separated)</label>
        <input
          id="modal-tags"
          type="text"
          bind:value={tagInput}
          placeholder="e.g., groceries, food, weekly"
        />
      </div>

      {#if suggestions.length > 0}
        <div class="suggested-tags">
          <span class="suggested-label">Suggested:</span>
          {#each suggestions.slice(0, 5) as suggestion}
            <button
              class="suggested-tag-btn"
              onclick={() => addSuggestedTag(suggestion.tag)}
            >
              {suggestion.tag}
            </button>
          {/each}
        </div>
      {/if}

      <div class="account-info">
        <span class="account-label">Account:</span>
        <span class="account-value">
          {transaction?.account_nickname ||
            transaction?.account_name ||
            "Unknown"}
        </span>
      </div>
    </div>

    {#snippet actions()}
      <button class="btn secondary" onclick={onclose}>Cancel</button>
      <button class="btn primary" onclick={handleSave}>Save</button>
    {/snippet}
  </Modal>
</div>

<style>
  .split-notice {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-lg);
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border-primary);
    font-size: 12px;
    color: var(--text-muted);
  }

  .split-badge {
    font-size: 14px;
  }

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

  .form-group input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .amount-input {
    font-family: var(--font-mono);
    text-align: right;
  }

  .suggested-tags {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    flex-wrap: wrap;
  }

  .suggested-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .suggested-tag-btn {
    padding: 4px 8px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    cursor: pointer;
  }

  .suggested-tag-btn:hover {
    background: var(--accent-primary);
    color: var(--bg-primary);
    border-color: var(--accent-primary);
  }

  .account-info {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding-top: var(--spacing-sm);
    border-top: 1px solid var(--border-primary);
  }

  .account-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .account-value {
    font-size: 12px;
    color: var(--text-primary);
  }
</style>
