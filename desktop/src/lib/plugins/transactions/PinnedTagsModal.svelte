<script lang="ts">
  /**
   * PinnedTagsModal - Manage pinned quick tags
   *
   * Pinned tags always appear first in the Quick Tags list (slots 1-N),
   * with remaining slots filled by frequency-based suggestions.
   */
  import { Modal, Icon } from "../../shared";

  interface Props {
    open: boolean;
    pinnedTags: string[];
    allTags: string[]; // All known tags sorted by frequency
    onclose: () => void;
    onsave: (tags: string[]) => void;
  }

  let { open, pinnedTags, allTags, onclose, onsave }: Props = $props();

  // Local state for editing
  let editingTags = $state<string[]>([]);
  let newTagInput = $state("");
  let inputEl = $state<HTMLInputElement | null>(null);

  // Reset when modal opens
  $effect(() => {
    if (open) {
      editingTags = [...pinnedTags];
      newTagInput = "";
    }
  });

  // Available tags to quick-select (not already pinned)
  let availableQuickTags = $derived.by(() => {
    return allTags
      .filter(tag => !editingTags.includes(tag))
      .slice(0, 12); // Show top 12 most frequent
  });

  // Filter suggestions for autocomplete
  let filteredSuggestions = $derived.by(() => {
    if (!newTagInput.trim()) return [];
    const input = newTagInput.toLowerCase();
    return allTags
      .filter(tag =>
        tag.toLowerCase().includes(input) &&
        !editingTags.includes(tag)
      )
      .slice(0, 5);
  });

  function addTag(tag: string) {
    const trimmed = tag.trim().toLowerCase();
    if (trimmed && !editingTags.includes(trimmed) && editingTags.length < 9) {
      editingTags = [...editingTags, trimmed];
      newTagInput = "";
      inputEl?.focus();
    }
  }

  function removeTag(index: number) {
    editingTags = editingTags.filter((_, i) => i !== index);
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && newTagInput.trim()) {
      e.preventDefault();
      addCurrentInput();
    }
  }

  function handleBlur() {
    // Auto-add tag on blur if there's text
    if (newTagInput.trim()) {
      addCurrentInput();
    }
  }

  function addCurrentInput() {
    // If there's a matching suggestion, use it
    if (filteredSuggestions.length > 0) {
      addTag(filteredSuggestions[0]);
    } else if (newTagInput.trim()) {
      addTag(newTagInput);
    }
  }

  function handleSave() {
    onsave(editingTags);
    onclose();
  }
</script>

<Modal {open} title="Edit Pinned Tags" {onclose} width="480px">
  <div class="pinned-body">
    <p class="intro">
      Pinned tags always appear first in Quick Tags (slots 1-{editingTags.length || "N"}).
      Remaining slots are filled by suggestions based on your tagging history.
    </p>

    <div class="section-label">Pinned Tags</div>
    {#if editingTags.length > 0}
      <div class="pinned-list">
        {#each editingTags as tag, i}
          <div class="pinned-item">
            <span class="pinned-slot">{i + 1}</span>
            <span class="pinned-tag">{tag}</span>
            <button class="remove-btn" onclick={() => removeTag(i)} title="Remove">×</button>
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty-state">No pinned tags yet</div>
    {/if}

    {#if editingTags.length < 9}
      <div class="section-label">Add a Tag</div>
      <div class="add-tag">
        <div class="input-row">
          <input
            type="text"
            bind:this={inputEl}
            bind:value={newTagInput}
            onkeydown={handleKeyDown}
            onblur={handleBlur}
            placeholder="Type a tag name..."
          />
          <button
            class="add-btn"
            onclick={addCurrentInput}
            disabled={!newTagInput.trim()}
            title="Add tag"
          >
            <Icon name="plus" size={16} />
          </button>
        </div>
        {#if filteredSuggestions.length > 0}
          <div class="suggestions-dropdown">
            {#each filteredSuggestions as suggestion}
              <button class="suggestion-item" onclick={() => addTag(suggestion)}>
                {suggestion}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      {#if availableQuickTags.length > 0}
        <div class="section-label">Or Select from Existing</div>
        <div class="quick-select">
          {#each availableQuickTags as tag}
            <button class="quick-tag" onclick={() => addTag(tag)}>
              {tag}
            </button>
          {/each}
        </div>
      {/if}
    {:else}
      <p class="max-hint">Maximum 9 pinned tags reached</p>
    {/if}
  </div>

  {#snippet actions()}
    <button class="btn secondary" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={handleSave}>Save</button>
  {/snippet}
</Modal>

<style>
  .pinned-body {
    padding: var(--spacing-md) var(--spacing-lg);
    min-height: 300px;
  }

  .intro {
    margin: 0 0 var(--spacing-lg) 0;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.4;
  }

  .section-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: var(--spacing-sm);
    margin-top: var(--spacing-md);
  }

  .section-label:first-of-type {
    margin-top: 0;
  }

  .pinned-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xs);
  }

  .pinned-item {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-xs) var(--spacing-sm);
    background: var(--bg-tertiary);
    border-radius: 4px;
  }

  .pinned-slot {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent-primary);
    color: white;
    border-radius: 3px;
    font-size: 11px;
    font-weight: 600;
  }

  .pinned-tag {
    flex: 1;
    font-size: 13px;
    color: var(--text-primary);
  }

  .remove-btn {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    border-radius: 3px;
  }

  .remove-btn:hover {
    background: var(--bg-secondary);
    color: var(--accent-danger);
  }

  .empty-state {
    padding: var(--spacing-md);
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
    background: var(--bg-tertiary);
    border-radius: 4px;
  }

  .add-tag {
    position: relative;
  }

  .input-row {
    display: flex;
    gap: var(--spacing-xs);
  }

  .input-row input {
    flex: 1;
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
  }

  .input-row input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .add-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    background: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }

  .add-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .add-btn:disabled {
    background: var(--bg-tertiary);
    color: var(--text-muted);
    cursor: not-allowed;
  }

  .suggestions-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-top: none;
    border-radius: 0 0 4px 4px;
    z-index: 10;
  }

  .suggestion-item {
    display: block;
    width: 100%;
    padding: 8px 12px;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }

  .suggestion-item:hover {
    background: var(--bg-tertiary);
  }

  .quick-select {
    display: flex;
    flex-wrap: wrap;
    gap: var(--spacing-xs);
  }

  .quick-tag {
    padding: 4px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
  }

  .quick-tag:hover {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
    color: var(--text-primary);
  }

  .max-hint {
    margin: var(--spacing-md) 0 0 0;
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
  }
</style>
