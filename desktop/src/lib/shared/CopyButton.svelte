<script lang="ts">
  /**
   * CopyButton - Small copy button that shows on hover
   *
   * Usage:
   * <CopyButton value="text to copy" />
   */
  import Icon from "./Icon.svelte";

  interface Props {
    /** The value to copy to clipboard */
    value: string;
    /** Size of the icon (default 12) */
    size?: number;
    /** Additional CSS class for positioning */
    class?: string;
  }

  let { value, size = 12, class: className = "" }: Props = $props();

  let copied = $state(false);
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  async function handleCopy(e: MouseEvent) {
    e.stopPropagation();

    try {
      await navigator.clipboard.writeText(value);
      copied = true;

      // Clear any existing timeout
      if (timeoutId) clearTimeout(timeoutId);

      timeoutId = setTimeout(() => {
        copied = false;
        timeoutId = null;
      }, 1200);
    } catch (err) {
      console.error("Failed to copy:", err);
    }
  }
</script>

<button
  class="copy-button {className}"
  class:copied
  onclick={handleCopy}
  title="Copy to clipboard"
>
  <Icon name={copied ? 'check' : 'copy'} {size} />
</button>

<style>
  .copy-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    opacity: 0;
    transition: color 0.15s, background 0.15s, opacity 0.15s;
  }

  .copy-button:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .copy-button.copied {
    color: var(--accent-success);
    animation: copy-pop 0.3s ease-out;
  }

  @keyframes copy-pop {
    0% { transform: scale(1); }
    50% { transform: scale(1.2); }
    100% { transform: scale(1); }
  }
</style>
