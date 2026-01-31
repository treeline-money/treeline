<script lang="ts">
  import { registry } from "../sdk";
  import type { Tab } from "../sdk";
  import { Icon, getIconName } from "../shared";

  let tabs = $state<Tab[]>(registry.tabs);
  let activeTabId = $state<string | null>(registry.activeTabId);

  $effect(() => {
    return registry.subscribe(() => {
      tabs = registry.tabs;
      activeTabId = registry.activeTabId;
    });
  });

  function handleTabClick(tabId: string) {
    registry.setActiveTab(tabId);
  }

  function handleTabClose(e: MouseEvent, tabId: string) {
    e.stopPropagation();
    registry.closeTab(tabId);
  }

  function handleMiddleClick(e: MouseEvent, tabId: string) {
    if (e.button === 1) {
      e.preventDefault();
      registry.closeTab(tabId);
    }
  }
</script>

<div class="tabbar">
  <div class="tabs-container">
    {#each tabs as tab (tab.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="tab"
        class:active={tab.id === activeTabId}
        onclick={() => handleTabClick(tab.id)}
        onauxclick={(e) => handleMiddleClick(e, tab.id)}
      >
        <span class="tab-icon"><Icon name={getIconName(tab.icon)} size={14} /></span>
        <span class="tab-title">{tab.title}</span>
        <button
          class="tab-close"
          onclick={(e) => handleTabClose(e, tab.id)}
          aria-label="Close tab"
        >
          ×
        </button>
      </div>
    {/each}
  </div>

  {#if tabs.length === 0}
    <div class="empty-tabs">
      <span class="empty-hint">No tabs open</span>
    </div>
  {/if}
</div>

<style>
  .tabbar {
    height: 36px;
    background: var(--tab-bg);
    border-bottom: 1px solid var(--tab-border);
    display: flex;
    align-items: stretch;
  }

  .tabs-container {
    display: flex;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tabs-container::-webkit-scrollbar {
    display: none;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    padding: 0 var(--spacing-md);
    min-width: 120px;
    max-width: 200px;
    background: transparent;
    border: none;
    border-right: 1px solid var(--tab-border);
    color: var(--text-secondary);
    font-size: 12px;
    font-family: var(--font-sans);
    cursor: pointer;
    transition: background 0.1s, color 0.1s;
  }

  .tab:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .tab.active {
    background: var(--tab-active-bg);
    color: var(--text-primary);
    border-bottom: 2px solid var(--accent-primary);
    margin-bottom: -1px;
  }

  .tab-icon {
    font-size: 14px;
    flex-shrink: 0;
  }

  .tab-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }

  .tab-close {
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    font-size: 16px;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.1s, background 0.1s;
  }

  .tab:hover .tab-close {
    opacity: 1;
  }

  .tab-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .empty-tabs {
    flex: 1;
    display: flex;
    align-items: center;
    padding: 0 var(--spacing-lg);
  }

  .empty-hint {
    color: var(--text-muted);
    font-size: 12px;
  }
</style>
