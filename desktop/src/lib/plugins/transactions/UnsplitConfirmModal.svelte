<script lang="ts">
  /**
   * UnsplitConfirmModal - Confirmation dialog for unsplitting a transaction
   */
  import { Modal } from "../../shared";

  interface Props {
    open: boolean;
    onclose: () => void;
    onconfirm: () => void;
  }

  let { open, onclose, onconfirm }: Props = $props();
</script>

<Modal {open} title="Unsplit Transaction?" {onclose} width="400px">
  <div class="confirm-body">
    <p>This will restore the original transaction and remove all split parts.</p>
    <p class="confirm-note">
      The original transaction will be restored with its original amount.
    </p>
  </div>

  {#snippet actions()}
    <button class="btn secondary" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={onconfirm}>Unsplit</button>
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
  }
</style>
