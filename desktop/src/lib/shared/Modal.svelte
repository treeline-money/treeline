<script lang="ts">
  /**
   * Modal - Shared modal dialog component
   *
   * Usage:
   * <Modal open={isOpen} title="Edit Item" onclose={handleClose}>
   *   <p>Modal content goes here</p>
   *
   *   {#snippet actions()}
   *     <button class="btn secondary" onclick={handleClose}>Cancel</button>
   *     <button class="btn primary" onclick={handleSave}>Save</button>
   *   {/snippet}
   * </Modal>
   */

  import type { Snippet } from "svelte";

  interface Props {
    /** Whether the modal is open */
    open: boolean;
    /** Modal title displayed in header */
    title: string;
    /** Called when modal should close (overlay click, escape, close button) */
    onclose: () => void;
    /** Optional width override (default: 450px) */
    width?: string;
    /** Optional max-height override (default: 80vh) */
    maxHeight?: string;
    /** Optional additional class for the modal container */
    class?: string;
    /** Default slot for modal content */
    children?: Snippet;
    /** Optional actions slot for footer buttons */
    actions?: Snippet;
  }

  let {
    open = false,
    title,
    onclose,
    width = "450px",
    maxHeight = "80vh",
    class: className = "",
    children,
    actions,
  }: Props = $props();

  let overlayEl = $state<HTMLDivElement | null>(null);

  function handleOverlayClick(e: MouseEvent) {
    if (e.target === overlayEl) {
      onclose();
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onclose();
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="modal-overlay"
    bind:this={overlayEl}
    onclick={handleOverlayClick}
    onkeydown={handleKeyDown}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="modal {className}"
      style="width: {width}; max-height: {maxHeight}"
      onclick={(e) => e.stopPropagation()}
      role="document"
    >
      <div class="modal-header">
        <span class="modal-title">{title}</span>
        <button class="close-btn" onclick={onclose} aria-label="Close">×</button>
      </div>

      <div class="modal-body">
        {#if children}
          {@render children()}
        {/if}
      </div>

      {#if actions}
        <div class="modal-actions">
          {@render actions()}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    background: var(--bg-secondary);
    border-radius: 8px;
    max-width: 90vw;
    display: flex;
    flex-direction: column;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.2);
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-md) var(--spacing-lg);
    border-bottom: 1px solid var(--border-primary);
    flex-shrink: 0;
  }

  .modal-title {
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    background: none;
    border: none;
    font-size: 20px;
    color: var(--text-muted);
    cursor: pointer;
    line-height: 1;
    padding: 0;
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.15s;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }

  .modal-body {
    flex: 1;
    overflow-y: auto;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-sm);
    padding: var(--spacing-md) var(--spacing-lg);
    border-top: 1px solid var(--border-primary);
    flex-shrink: 0;
  }

  /* Common button styles used in modals */
  .modal-actions :global(.btn) {
    padding: 8px 16px;
    border-radius: 4px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    border: none;
    transition: opacity 0.15s;
  }

  .modal-actions :global(.btn:hover) {
    opacity: 0.9;
  }

  .modal-actions :global(.btn.primary) {
    background: var(--accent-primary);
    color: var(--bg-primary);
  }

  .modal-actions :global(.btn.secondary) {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border-primary);
  }

  .modal-actions :global(.btn.danger) {
    background: var(--accent-danger, #ef4444);
    color: white;
  }
</style>
