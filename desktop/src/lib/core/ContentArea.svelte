<script lang="ts">
  import { onDestroy } from "svelte";
  import { registry, modKey, logger } from "../sdk";
  import { createPluginSDK } from "../sdk/public";
  import type { Tab, ViewDefinition } from "../sdk";

  let tabs = $state<Tab[]>(registry.tabs);
  let activeTabId = $state<string | null>(registry.activeTab?.id ?? null);
  let views = $state(registry.views);

  // Track mount containers and cleanup functions for external plugins
  let mountContainers = $state<Record<string, HTMLDivElement | null>>({});
  let cleanupFunctions: Record<string, (() => void) | null> = {};

  // Track previous active tab to detect changes
  let prevActiveTabId: string | null = null;

  $effect(() => {
    return registry.subscribe(() => {
      tabs = registry.tabs;
      const newActiveTabId = registry.activeTab?.id ?? null;

      // Emit event when tab changes (not on initial load)
      if (prevActiveTabId !== null && newActiveTabId !== prevActiveTabId && newActiveTabId !== null) {
        // Log page navigation
        const tab = registry.tabs.find((t) => t.id === newActiveTabId);
        if (tab) {
          logger.page(tab.viewId);
        }
        // Small delay to let the tab render before focusing
        setTimeout(() => registry.emit("tab:activated"), 10);
      }

      prevActiveTabId = newActiveTabId;
      activeTabId = newActiveTabId;
      views = registry.views;
    });
  });

  // Get view definition for a tab
  function getView(tab: Tab): ViewDefinition | null {
    return registry.getView(tab.viewId) ?? null;
  }

  // Track which mount function each tab was mounted with (for hot-reload detection)
  let mountedWith: Record<string, Function | null> = {};
  // Track Svelte style tag IDs injected by each plugin tab (for cleanup on hot-reload)
  let pluginStyleIds: Record<string, string[]> = {};

  // Handle mounting external plugins when their container becomes available
  function handleMountContainer(tab: Tab, container: HTMLDivElement | null) {
    const view = getView(tab);
    if (!view?.mount || !container) return;

    // Already mounted with the same mount function — skip
    if (cleanupFunctions[tab.id] && mountedWith[tab.id] === view.mount) return;

    // Store the container ref so the hot-reload effect can find it
    mountContainers[tab.id] = container;

    // Hot-reload: clean up old mount before remounting with new function
    if (cleanupFunctions[tab.id]) {
      cleanupFunctions[tab.id]!();
      container.innerHTML = "";
      // Remove Svelte-injected style tags that this plugin added.
      // append_styles() skips insertion if a style with the same id exists,
      // and the id is filename-based (not content-based), so stale styles block updates.
      for (const id of pluginStyleIds[tab.id] ?? []) {
        document.head.querySelector(`style#${id}`)?.remove();
      }
      delete pluginStyleIds[tab.id];
    }

    // Get plugin ID and permissions for this view
    const pluginId = registry.getPluginIdForView(tab.viewId);
    const permissions = pluginId ? registry.getPluginPermissions(pluginId) : {};

    // Create SDK instance for this plugin
    const sdk = pluginId ? createPluginSDK(pluginId, permissions) : null;

    // Pass SDK and original props to the view
    const props = {
      ...tab.props,
      sdk,
    };

    // Snapshot existing style tags, mount, then diff to find plugin's injected styles
    const stylesBefore = new Set(
      Array.from(document.head.querySelectorAll('style[id^="svelte-"]')).map((el) => el.id)
    );
    cleanupFunctions[tab.id] = view.mount(container, props);
    mountedWith[tab.id] = view.mount;
    const stylesAfter = document.head.querySelectorAll('style[id^="svelte-"]');
    pluginStyleIds[tab.id] = Array.from(stylesAfter)
      .map((el) => el.id)
      .filter((id) => !stylesBefore.has(id));
  }

  // Re-mount plugin tabs when their view definition changes (hot-reload)
  $effect(() => {
    // Read `views` to subscribe to view changes
    const _views = views;
    for (const tab of tabs) {
      const view = getView(tab);
      const container = mountContainers[tab.id];
      if (view?.mount && container && mountedWith[tab.id] && mountedWith[tab.id] !== view.mount) {
        handleMountContainer(tab, container);
      }
    }
  });

  // Clean up closed tabs
  $effect(() => {
    const currentTabIds = new Set(tabs.map(t => t.id));

    // Clean up any tabs that are no longer open
    for (const tabId of Object.keys(cleanupFunctions)) {
      if (!currentTabIds.has(tabId) && cleanupFunctions[tabId]) {
        cleanupFunctions[tabId]!();
        delete cleanupFunctions[tabId];
        delete mountContainers[tabId];
        delete mountedWith[tabId];
        delete pluginStyleIds[tabId];
      }
    }
  });

  // Clean up all on component destroy
  onDestroy(() => {
    for (const cleanup of Object.values(cleanupFunctions)) {
      if (cleanup) cleanup();
    }
  });

  // Svelte action to handle mount container binding
  function mountAction(node: HTMLDivElement, params: { tab: Tab; handler: (tab: Tab, el: HTMLDivElement | null) => void }) {
    params.handler(params.tab, node);
    return {
      destroy() {
        // Cleanup handled by the effect
      }
    };
  }
</script>

<main class="content-area">
  {#if tabs.length > 0}
    {#each tabs as tab (tab.id)}
      {@const view = getView(tab)}
      {@const isActive = tab.id === activeTabId}

      {#if view}
        <div class="tab-content" class:active={isActive} class:hidden={!isActive}>
          {#if view.component}
            <!-- Core plugin with Svelte component -->
            {@const Component = view.component}
            <Component {...tab.props} />
          {:else if view.mount}
            <!-- External plugin with mount function -->
            <div
              class="plugin-mount-container"
              use:mountAction={{ tab, handler: handleMountContainer }}
            ></div>
          {/if}
        </div>
      {/if}
    {/each}
  {:else}
    <div class="empty-state">
      <div class="empty-content">
        <div class="empty-logo">
          <svg viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
            <path d="M32 12 L20 35 L35 40 L44 35 Z" fill="var(--logo-snow)"/>
            <path d="M20 35 L35 40 L44 35 L54 52 L10 52 Z" fill="var(--accent-primary)"/>
            <path d="M32 12 L54 52 L10 52 Z" stroke="var(--accent-primary)" stroke-width="2.5" fill="none"/>
          </svg>
        </div>
        <h2 class="empty-title">treeline</h2>
        <p class="empty-subtitle">Select a view from the sidebar or press <kbd>{modKey()}P</kbd> to open the command palette</p>

        <div class="keyboard-hints">
          <div class="hint-group">
            <kbd>{modKey()}P</kbd>
            <span>Command palette</span>
          </div>
          <div class="hint-group">
            <kbd>{modKey()}1-9</kbd>
            <span>Switch tabs</span>
          </div>
          <div class="hint-group">
            <kbd>{modKey()}W</kbd>
            <span>Close tab</span>
          </div>
        </div>
      </div>
    </div>
  {/if}
</main>

<style>
  .content-area {
    flex: 1;
    background: var(--bg-primary);
    overflow: hidden;
    position: relative;
  }

  .tab-content {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    overflow: auto;
  }

  .tab-content.hidden {
    visibility: hidden;
    pointer-events: none;
  }

  .tab-content.active {
    visibility: visible;
    pointer-events: auto;
  }

  .empty-state {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .empty-content {
    text-align: center;
    max-width: 400px;
  }

  .empty-logo {
    width: 80px;
    height: 80px;
    margin: 0 auto var(--spacing-lg);
  }

  .empty-logo svg {
    width: 100%;
    height: 100%;
  }

  .empty-title {
    font-family: 'Outfit', var(--font-sans);
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 var(--spacing-sm);
    letter-spacing: -0.5px;
  }

  .empty-subtitle {
    color: var(--text-secondary);
    font-size: 14px;
    margin: 0 0 var(--spacing-xl);
    line-height: 1.5;
  }

  .keyboard-hints {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
    text-align: left;
    background: var(--bg-secondary);
    padding: var(--spacing-lg);
    border-radius: var(--radius-md);
    border: 1px solid var(--border-primary);
  }

  .hint-group {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
  }

  kbd {
    font-family: var(--font-mono);
    font-size: 11px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    padding: 2px 8px;
    min-width: 60px;
    text-align: center;
    color: var(--text-secondary);
  }

  .hint-group span {
    color: var(--text-secondary);
    font-size: 13px;
  }

  .plugin-mount-container {
    width: 100%;
    height: 100%;
  }
</style>
