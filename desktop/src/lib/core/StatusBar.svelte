<script lang="ts">
  import { registry, themeManager } from "../sdk";
  import type { StatusBarItem } from "../sdk";
  import StatusBarActivity from "./StatusBarActivity.svelte";

  let statusBarItems = $state<StatusBarItem[]>(registry.statusBarItems);
  let currentTheme = $state(themeManager.current);
  let availableThemes = $state(themeManager.getAvailableThemes());

  $effect(() => {
    return registry.subscribe(() => {
      statusBarItems = registry.statusBarItems;
    });
  });

  $effect(() => {
    return themeManager.subscribe((themeId) => {
      currentTheme = themeId;
      // Refresh available themes in case they changed
      availableThemes = themeManager.getAvailableThemes();
    });
  });

  let leftItems = $derived(statusBarItems.filter((i) => i.position === "left"));
  let rightItems = $derived(statusBarItems.filter((i) => i.position === "right"));

  let currentThemeName = $derived(
    availableThemes.find((t) => t.id === currentTheme)?.name ?? currentTheme
  );

  function cycleTheme() {
    const themeIds = availableThemes.map((t) => t.id);
    const currentIndex = themeIds.indexOf(currentTheme);
    const nextIndex = (currentIndex + 1) % themeIds.length;
    themeManager.setTheme(themeIds[nextIndex]);
  }
</script>

<footer class="statusbar">
  <div class="statusbar-left">
    <StatusBarActivity />
    {#each leftItems as item (item.id)}
      <item.component />
    {/each}
  </div>

  <div class="statusbar-right">
    {#each rightItems as item (item.id)}
      <item.component />
    {/each}

    <!-- Built-in theme toggle -->
    <button class="statusbar-item" onclick={cycleTheme} title="Change theme">
      <span class="item-icon">◐</span>
      <span class="item-text">{currentThemeName}</span>
    </button>
  </div>
</footer>

<style>
  .statusbar {
    height: 24px;
    background: var(--statusbar-bg);
    border-top: 1px solid var(--statusbar-border);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 var(--spacing-sm);
    font-size: 11px;
    user-select: none;
  }

  .statusbar-left,
  .statusbar-right {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
  }

  .statusbar-item {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px var(--spacing-sm);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font-size: 11px;
    font-family: var(--font-sans);
    cursor: pointer;
    transition: background 0.1s, color 0.1s;
  }

  .statusbar-item:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .item-icon {
    font-size: 12px;
  }

  :global(.statusbar-item) {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px var(--spacing-sm);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font-size: 11px;
    font-family: var(--font-sans);
    cursor: pointer;
    transition: background 0.1s, color 0.1s;
  }

  :global(.statusbar-item:hover) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
</style>
