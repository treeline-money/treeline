<script lang="ts">
  import { registry, modKey, formatShortcut } from "../sdk";
  import type { Command } from "../sdk";

  let { isOpen = $bindable(false) } = $props();

  let searchQuery = $state("");
  let selectedIndex = $state(0);
  let inputEl = $state<HTMLInputElement | null>(null);

  let commands = $state<Command[]>(registry.commands);

  $effect(() => {
    return registry.subscribe(() => {
      commands = registry.commands;
    });
  });

  // Filter commands based on search
  let filteredCommands = $derived.by(() => {
    if (!searchQuery.trim()) return commands;

    const query = searchQuery.toLowerCase();
    return commands.filter(
      (cmd) =>
        cmd.name.toLowerCase().includes(query) ||
        cmd.id.toLowerCase().includes(query) ||
        cmd.category?.toLowerCase().includes(query)
    );
  });

  // Group by category
  let groupedCommands = $derived.by(() => {
    const groups: Record<string, Command[]> = {};
    for (const cmd of filteredCommands) {
      const category = cmd.category ?? "Commands";
      if (!groups[category]) groups[category] = [];
      groups[category].push(cmd);
    }
    return groups;
  });

  // Reset selection when search changes
  $effect(() => {
    searchQuery;
    selectedIndex = 0;
  });

  // Focus input when opened
  $effect(() => {
    if (isOpen && inputEl) {
      inputEl.focus();
      searchQuery = "";
      selectedIndex = 0;
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      isOpen = false;
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, filteredCommands.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
    } else if (e.key === "Enter" && filteredCommands.length > 0) {
      e.preventDefault();
      executeSelected();
    }
  }

  function executeSelected() {
    const cmd = filteredCommands[selectedIndex];
    if (cmd) {
      isOpen = false;
      cmd.execute();
    }
  }

  function handleItemClick(cmd: Command) {
    isOpen = false;
    cmd.execute();
  }

  function handleBackdropClick() {
    isOpen = false;
  }

  // Get flat index for highlighting
  function getFlatIndex(cmd: Command): number {
    return filteredCommands.indexOf(cmd);
  }
</script>

{#if isOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="palette-backdrop" onclick={handleBackdropClick}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="palette" onclick={(e) => e.stopPropagation()}>
      <div class="palette-input-container">
        <span class="palette-icon">{modKey()}</span>
        <input
          bind:this={inputEl}
          bind:value={searchQuery}
          onkeydown={handleKeydown}
          type="text"
          class="palette-input"
          placeholder="Type a command..."
        />
      </div>

      <div class="palette-results">
        {#if filteredCommands.length === 0}
          <div class="palette-empty">No commands found</div>
        {:else}
          {#each Object.entries(groupedCommands) as [category, cmds]}
            <div class="palette-category">
              <div class="category-title">{category}</div>
              {#each cmds as cmd}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="palette-item"
                  class:selected={getFlatIndex(cmd) === selectedIndex}
                  onclick={() => handleItemClick(cmd)}
                >
                  <span class="item-name">{cmd.name}</span>
                  {#if cmd.shortcut}
                    <span class="item-shortcut">{formatShortcut(cmd.shortcut)}</span>
                  {/if}
                </div>
              {/each}
            </div>
          {/each}
        {/if}
      </div>

      <div class="palette-footer">
        <span class="hint">↑↓ navigate</span>
        <span class="hint">↵ select</span>
        <span class="hint">esc close</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .palette-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    justify-content: center;
    padding-top: 15vh;
    z-index: 1000;
  }

  .palette {
    width: 500px;
    max-height: 400px;
    background: var(--palette-bg);
    border: 1px solid var(--palette-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .palette-input-container {
    display: flex;
    align-items: center;
    padding: var(--spacing-md);
    border-bottom: 1px solid var(--palette-border);
    gap: var(--spacing-sm);
  }

  .palette-icon {
    color: var(--text-muted);
    font-size: 14px;
  }

  .palette-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 14px;
    font-family: var(--font-sans);
  }

  .palette-input::placeholder {
    color: var(--text-muted);
  }

  .palette-results {
    flex: 1;
    overflow-y: auto;
  }

  .palette-empty {
    padding: var(--spacing-xl);
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }

  .palette-category {
    padding: var(--spacing-xs) 0;
  }

  .category-title {
    padding: var(--spacing-xs) var(--spacing-md);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
  }

  .palette-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-sm) var(--spacing-md);
    cursor: pointer;
    transition: background 0.1s;
  }

  .palette-item:hover,
  .palette-item.selected {
    background: var(--palette-item-hover);
  }

  .item-name {
    color: var(--text-primary);
    font-size: 13px;
  }

  .item-shortcut {
    font-size: 11px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    background: var(--bg-tertiary);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
  }

  .palette-footer {
    display: flex;
    gap: var(--spacing-lg);
    padding: var(--spacing-sm) var(--spacing-md);
    border-top: 1px solid var(--palette-border);
    background: var(--bg-tertiary);
  }

  .hint {
    font-size: 11px;
    color: var(--text-muted);
    font-family: var(--font-mono);
  }
</style>
