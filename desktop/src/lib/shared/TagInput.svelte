<script lang="ts">
  /**
   * TagInput - Reusable tag input with autocomplete
   *
   * Usage:
   * <TagInput
   *   bind:value={tags}
   *   allTags={availableTags}
   *   placeholder="Add tags..."
   *   onsubmit={() => saveTags()}
   *   oncancel={() => cancelEdit()}
   * />
   */

  interface Props {
    /** Current comma-separated tags value */
    value: string;
    /** All available tags for autocomplete (sorted by preference) */
    allTags: string[];
    /** Placeholder text */
    placeholder?: string;
    /** Input ID for label association */
    id?: string;
    /** CSS class for the input */
    class?: string;
    /** Called when Enter is pressed (without autocomplete selection) */
    onsubmit?: () => void;
    /** Called when Escape is pressed */
    oncancel?: () => void;
    /** Called on any click (for stopPropagation in inline editors) */
    onclick?: (e: MouseEvent) => void;
    /** If true, also calls onsubmit when selecting from autocomplete with Enter */
    submitOnSelect?: boolean;
    /** If true, uses smaller padding for inline/compact contexts */
    compact?: boolean;
  }

  let {
    value = $bindable(""),
    allTags,
    placeholder = "Add tags...",
    id,
    class: className,
    onsubmit,
    oncancel,
    onclick,
    submitOnSelect = false,
    compact = false,
  }: Props = $props();

  let inputEl = $state<HTMLInputElement | null>(null);
  let autocompleteIndex = $state(-1);

  /** Focus the input element */
  export function focus() {
    inputEl?.focus();
  }

  /** Get the input element */
  export function getInput() {
    return inputEl;
  }

  // Autocomplete suggestions based on current input
  let suggestions = $derived.by(() => {
    if (!value || allTags.length === 0) return [];

    // Get the partial tag being typed (after last comma)
    const parts = value.split(",");
    const partial = parts[parts.length - 1].trim().toLowerCase();
    if (!partial || partial.length < 1) return [];

    // Don't show suggestions if partial exactly matches a tag (it's already complete)
    if (allTags.some(tag => tag.toLowerCase() === partial)) return [];

    // Get already-entered tags to exclude
    const enteredTags = parts.slice(0, -1).map(t => t.trim().toLowerCase());

    // Find matching tags (prefix match only - this is autocomplete, not search)
    return allTags
      .filter(tag => {
        const tagLower = tag.toLowerCase();
        // Don't suggest tags already entered
        if (enteredTags.includes(tagLower)) return false;
        // Must start with partial
        return tagLower.startsWith(partial);
      })
      .slice(0, 8);
  });

  function selectTag(tag: string, viaEnter = false) {
    const parts = value.split(",").map(t => t.trim());
    parts[parts.length - 1] = tag; // Replace partial with selected tag

    if (submitOnSelect && viaEnter) {
      // For inline editing: update value and submit immediately
      value = parts.join(", ");
      autocompleteIndex = -1;
      onsubmit?.();
    } else {
      // Normal mode: add comma for next tag
      value = parts.join(", ") + ", ";
      autocompleteIndex = -1;
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      oncancel?.();
      return;
    }

    if (suggestions.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        autocompleteIndex = Math.min(autocompleteIndex + 1, suggestions.length - 1);
        return;
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        autocompleteIndex = Math.max(autocompleteIndex - 1, -1);
        return;
      } else if (e.key === "Enter" && autocompleteIndex >= 0) {
        e.preventDefault();
        e.stopPropagation();
        selectTag(suggestions[autocompleteIndex], true);
        return;
      } else if (e.key === "Tab" && suggestions.length > 0) {
        e.preventDefault();
        selectTag(suggestions[0]);
        return;
      }
    }

    // Enter without autocomplete selection = submit
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      onsubmit?.();
    }
  }

  // Reset autocomplete index when input changes
  $effect(() => {
    value; // dependency
    autocompleteIndex = -1;
  });
</script>

<div class="tag-input-wrapper" class:compact>
  <input
    bind:this={inputEl}
    {id}
    type="text"
    class={className}
    bind:value
    onkeydown={handleKeyDown}
    {onclick}
    {placeholder}
    autocomplete="off"
  />
  {#if suggestions.length > 0}
    <div class="tag-autocomplete-dropdown">
      {#each suggestions as tag, i}
        <button
          type="button"
          class="autocomplete-item"
          class:selected={i === autocompleteIndex}
          onclick={() => selectTag(tag)}
        >
          {tag}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .tag-input-wrapper {
    position: relative;
  }

  .tag-input-wrapper input {
    width: 100%;
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
  }

  .tag-input-wrapper.compact input {
    padding: 4px 8px;
    font-size: 12px;
  }

  .tag-input-wrapper input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .tag-autocomplete-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-top: none;
    border-radius: 0 0 4px 4px;
    max-height: 200px;
    overflow-y: auto;
    z-index: 10;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  }

  .autocomplete-item {
    display: block;
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    text-align: left;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
  }

  .compact .autocomplete-item {
    padding: 6px 8px;
    font-size: 12px;
  }

  .autocomplete-item:hover,
  .autocomplete-item.selected {
    background: var(--accent-primary);
    color: white;
  }
</style>
