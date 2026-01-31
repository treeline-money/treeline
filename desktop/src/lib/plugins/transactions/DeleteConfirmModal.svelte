<script lang="ts">
  /**
   * DeleteConfirmModal - Confirmation dialog for deleting a transaction
   */
  import { Modal, formatUserCurrency } from "../../shared";
  import type { Transaction } from "./types";

  interface Props {
    open: boolean;
    transaction: Transaction | null;
    onclose: () => void;
    onconfirm: () => void;
  }

  let { open, transaction, onclose, onconfirm }: Props = $props();
</script>

<Modal {open} title="Delete Transaction?" {onclose} width="400px">
  <div class="confirm-body">
    <p>Are you sure you want to delete this transaction?</p>
    {#if transaction}
      <div class="txn-preview">
        <div class="txn-preview-desc">{transaction.description}</div>
        <div class="txn-preview-amount" class:negative={transaction.amount < 0}>
          {formatUserCurrency(transaction.amount)}
        </div>
      </div>
    {/if}
    <p class="confirm-note">This transaction won't be re-imported during sync.</p>
  </div>

  {#snippet actions()}
    <button class="btn secondary" onclick={onclose}>Cancel</button>
    <button class="btn danger" onclick={onconfirm}>Delete</button>
  {/snippet}
</Modal>

<style>
  .confirm-body {
    padding: var(--spacing-md) var(--spacing-lg);
  }

  .confirm-body p {
    margin: 0 0 var(--spacing-md) 0;
    color: var(--text-primary);
    font-size: 14px;
  }

  .confirm-note {
    font-size: 12px !important;
    color: var(--text-muted) !important;
    margin-top: var(--spacing-md) !important;
  }

  .txn-preview {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 4px;
    margin: var(--spacing-md) 0;
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
</style>
