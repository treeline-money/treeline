<script lang="ts">
  import { hubWatch, featureFlags, FEATURE_HUB } from "../sdk";

  interface Props {
    /** When set, the pill becomes clickable in the conflict state and this
     *  fires — the status bar opens the resolution modal. */
    onConflictClick?: () => void;
  }
  let { onConflictClick }: Props = $props();

  // Tick every 15s so "Up to date · 30s ago" stays current.
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => {
      now = Date.now();
    }, 15_000);
    return () => clearInterval(id);
  });

  let label = $derived.by(() => {
    if (!featureFlags.isEnabled(FEATURE_HUB)) return null;
    if (!hubWatch.running) return null;
    switch (hubWatch.status) {
      case "pushing":
        return { icon: "↻", text: "Pushing…", className: "busy" };
      case "pulling":
        return { icon: "↻", text: "Pulling…", className: "busy" };
      case "conflict":
        return {
          icon: "⚠",
          text: `Conflict (${hubWatch.conflictCount})`,
          className: "conflict",
        };
      case "error":
        return { icon: "⚠", text: "Hub error", className: "error" };
      case "up_to_date":
      case "watching": {
        if (hubWatch.lastUpdatedAt) {
          const ago = ageLabel(now - hubWatch.lastUpdatedAt);
          return { icon: "✓", text: `Up to date · ${ago}`, className: "ok" };
        }
        return { icon: "⟳", text: "Watching", className: "watching" };
      }
      default:
        return null;
    }
  });

  function ageLabel(ms: number): string {
    // `now` is sampled every 15s but lastUpdatedAt updates immediately, so the
    // diff can transiently be negative. Treat anything <5s as "just now".
    const seconds = Math.max(0, Math.floor(ms / 1000));
    if (seconds < 5) return "just now";
    if (seconds < 60) return `${seconds}s ago`;
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    return `${hours}h ago`;
  }

  let title = $derived.by(() => {
    const parts: string[] = [];
    if (hubWatch.hubUrl) parts.push(`Hub: ${hubWatch.hubUrl}`);
    if (hubWatch.errorMessage) parts.push(`Error: ${hubWatch.errorMessage}`);
    return parts.join("\n") || "Hub watch";
  });
</script>

{#if label}
  {#if hubWatch.status === "conflict" && onConflictClick}
    <button
      class="statusbar-item hub-watch hub-watch-{label.className} hub-watch-clickable"
      title="Click to review and resolve"
      onclick={onConflictClick}
    >
      <span class="item-icon">{label.icon}</span>
      <span class="item-text">{label.text} — click to resolve</span>
    </button>
  {:else}
    <span class="statusbar-item hub-watch hub-watch-{label.className}" {title}>
      <span class="item-icon">{label.icon}</span>
      <span class="item-text">{label.text}</span>
    </span>
  {/if}
{/if}

<style>
  .hub-watch {
    cursor: default;
  }
  .hub-watch-clickable {
    cursor: pointer;
  }
  .hub-watch-busy .item-icon {
    animation: spin 1s linear infinite;
    display: inline-block;
  }
  .hub-watch-conflict {
    color: var(--color-warning, #d97706);
  }
  .hub-watch-error {
    color: var(--color-danger, #dc2626);
  }
  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
</style>
