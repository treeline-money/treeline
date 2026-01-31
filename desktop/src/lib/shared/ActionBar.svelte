<script lang="ts">
  /**
   * ActionBar - Shared component for displaying keyboard shortcuts as clickable buttons
   *
   * Usage:
   * <ActionBar actions={[
   *   { keys: ["j", "k"], label: "nav", action: () => {} },
   *   { keys: ["Enter"], label: "view", action: () => openView() },
   *   { keys: ["e"], label: "edit", action: () => startEdit() },
   * ]} />
   */

  export interface ActionItem {
    /** Keyboard key(s) that trigger this action */
    keys: string[];
    /** Short label describing the action */
    label: string;
    /** Function to execute when clicked */
    action?: () => void;
    /** Whether this action is currently disabled */
    disabled?: boolean;
  }

  interface Props {
    actions: ActionItem[];
  }

  let { actions }: Props = $props();

  function handleClick(item: ActionItem) {
    if (item.action && !item.disabled) {
      item.action();
    }
  }

  function handleKeyDown(e: KeyboardEvent, item: ActionItem) {
    if ((e.key === "Enter" || e.key === " ") && item.action && !item.disabled) {
      e.preventDefault();
      item.action();
    }
  }
</script>

<div class="action-bar">
  {#each actions as item}
    <button
      class="action-item"
      class:disabled={item.disabled}
      class:clickable={!!item.action && !item.disabled}
      onclick={() => handleClick(item)}
      onkeydown={(e) => handleKeyDown(e, item)}
      disabled={item.disabled}
      tabindex={item.action && !item.disabled ? 0 : -1}
    >
      {#each item.keys as key, i}
        <kbd>{key}</kbd>
      {/each}
      <span class="label">{item.label}</span>
    </button>
  {/each}
</div>

<style>
  .action-bar {
    padding: 6px var(--spacing-lg);
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border-primary);
    display: flex;
    gap: var(--spacing-lg);
    font-size: 11px;
    color: var(--text-muted);
    flex-wrap: wrap;
  }

  .action-item {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: none;
    border: none;
    padding: 2px 4px;
    margin: -2px -4px;
    border-radius: 4px;
    color: inherit;
    font: inherit;
    cursor: default;
    transition: background-color 0.1s, color 0.1s;
  }

  .action-item.clickable {
    cursor: pointer;
  }

  .action-item.clickable:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .action-item.clickable:active {
    background: var(--bg-primary);
  }

  .action-item.disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .action-item kbd {
    display: inline-block;
    padding: 1px 4px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 3px;
    font-family: var(--font-mono);
    font-size: 10px;
    margin-right: 2px;
    transition: border-color 0.1s, background-color 0.1s;
  }

  .action-item.clickable:hover kbd {
    border-color: var(--text-muted);
    background: var(--bg-tertiary);
  }

  .label {
    margin-left: 2px;
  }
</style>
