<script lang="ts">
  import { onMount } from "svelte";
  import { registry, getAppSetting, setAppSetting, modKey, pluginUpdatesStore } from "../sdk";
  import { Icon, getIconName } from "../shared";
  import type { SidebarItem } from "../sdk/types";

  // Collapsed state
  let isCollapsed = $state(false);

  onMount(async () => {
    // Load collapsed state from settings
    const collapsed = await getAppSetting("sidebarCollapsed");
    if (collapsed !== undefined) {
      isCollapsed = collapsed;
    }
  });

  async function toggleCollapsed() {
    isCollapsed = !isCollapsed;
    await setAppSetting("sidebarCollapsed", isCollapsed);
  }

  // Reactive state
  let sections = $state(registry.sidebarSections);
  let items = $state(registry.sidebarItems);

  // Subscribe to registry changes
  $effect(() => {
    return registry.subscribe(() => {
      sections = registry.sidebarSections;
      items = registry.sidebarItems;
    });
  });

  function getItemsForSection(sectionId: string): SidebarItem[] {
    // Filter out settings - it's now in the footer as a modal
    return items.filter((item) => item.sectionId === sectionId && item.viewId !== "settings");
  }

  function handleItemClick(viewId: string) {
    registry.openView(viewId);
  }

  // Track active view for highlighting
  let activeViewId = $derived(registry.activeTab?.viewId ?? null);
</script>

<aside class="sidebar" class:collapsed={isCollapsed}>
  <div class="sidebar-header">
    {#if isCollapsed}
      <button class="expand-btn" onclick={toggleCollapsed} title="Expand sidebar">
        <svg class="logo" viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
          <path d="M32 12 L20 35 L35 40 L44 35 Z" fill="var(--logo-snow)"/>
          <path d="M20 35 L35 40 L44 35 L54 52 L10 52 Z" fill="var(--accent-primary)"/>
          <path d="M32 12 L54 52 L10 52 Z" stroke="var(--accent-primary)" stroke-width="2.5" fill="none"/>
        </svg>
      </button>
    {:else}
      <svg class="logo" viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
        <path d="M32 12 L20 35 L35 40 L44 35 Z" fill="var(--logo-snow)"/>
        <path d="M20 35 L35 40 L44 35 L54 52 L10 52 Z" fill="var(--accent-primary)"/>
        <path d="M32 12 L54 52 L10 52 Z" stroke="var(--accent-primary)" stroke-width="2.5" fill="none"/>
      </svg>
      <span class="title">treeline</span>
      <button class="collapse-btn" onclick={toggleCollapsed} title="Collapse sidebar">
        <Icon name="chevron-down" size={14} />
      </button>
    {/if}
  </div>

  <nav class="sidebar-nav">
    {#each sections as section}
      <div class="sidebar-section">
        {#if !isCollapsed}
          <div class="section-title">{section.title}</div>
        {/if}
        <ul class="section-items">
          {#each getItemsForSection(section.id) as item (item.id)}
            <li>
              <button
                class="sidebar-item"
                class:active={activeViewId === item.viewId}
                onclick={() => handleItemClick(item.viewId)}
                title={isCollapsed ? item.label : undefined}
              >
                <span class="item-icon">
                  <Icon name={getIconName(item.icon)} size={16} />
                  {#if item.badge && isCollapsed}
                    <span class="badge badge-collapsed">{item.badge > 99 ? '99+' : item.badge}</span>
                  {/if}
                </span>
                {#if !isCollapsed}
                  <span class="item-label">{item.label}</span>
                  {#if item.badge}
                    <span class="badge">{item.badge > 99 ? '99+' : item.badge}</span>
                  {:else if item.shortcut}
                    <span class="item-shortcut">{item.shortcut}</span>
                  {/if}
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/each}
  </nav>

  <div class="sidebar-footer">
    <button class="sidebar-item" onclick={() => registry.executeCommand("core:settings:plugins")} title={isCollapsed ? "Plugins" : undefined}>
      <span class="item-icon">
        <Icon name="package" size={16} />
        {#if pluginUpdatesStore.hasUpdates && isCollapsed}
          <span class="badge badge-collapsed">{pluginUpdatesStore.count}</span>
        {/if}
      </span>
      {#if !isCollapsed}
        <span class="item-label">Plugins</span>
        {#if pluginUpdatesStore.hasUpdates}
          <span class="badge">{pluginUpdatesStore.count}</span>
        {/if}
      {/if}
    </button>
    <button class="sidebar-item" onclick={() => registry.executeCommand("core:settings")} title={isCollapsed ? "Settings" : undefined}>
      <span class="item-icon">
        <Icon name="settings" size={16} />
      </span>
      {#if !isCollapsed}
        <span class="item-label">Settings</span>
        <span class="item-shortcut">{modKey()},</span>
      {/if}
    </button>
    <button class="sidebar-item" onclick={() => registry.executeCommand("core:command-palette")} title={isCollapsed ? "Commands" : undefined}>
      <span class="item-icon">
        <Icon name="command" size={16} />
      </span>
      {#if !isCollapsed}
        <span class="item-label">Commands</span>
        <span class="item-shortcut">{modKey()}P</span>
      {/if}
    </button>
  </div>
</aside>

<style>
  .sidebar {
    width: 220px;
    height: 100%;
    background: var(--sidebar-bg);
    border-right: 1px solid var(--sidebar-border);
    display: flex;
    flex-direction: column;
    user-select: none;
    transition: width 0.15s ease;
  }

  .sidebar.collapsed {
    width: 52px;
  }

  .sidebar-header {
    padding: var(--spacing-md) var(--spacing-lg);
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    border-bottom: 1px solid var(--sidebar-border);
  }

  .sidebar.collapsed .sidebar-header {
    padding: var(--spacing-md) var(--spacing-sm);
    justify-content: center;
  }

  .logo {
    width: 22px;
    height: 22px;
    flex-shrink: 0;
  }

  .title {
    font-family: 'Outfit', var(--font-sans);
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: -0.5px;
    flex: 1;
  }

  .collapse-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.1s, background 0.1s;
  }

  .collapse-btn:hover {
    color: var(--text-primary);
    background: var(--sidebar-item-hover);
  }

  .expand-btn {
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: transform 0.15s ease;
  }

  .expand-btn:hover {
    transform: scale(1.1);
  }

  .expand-btn .logo {
    width: 22px;
    height: 22px;
  }

  .sidebar-nav {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-sm) 0;
  }

  .sidebar-section {
    margin-bottom: var(--spacing-md);
  }

  .section-title {
    padding: var(--spacing-xs) var(--spacing-lg);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
  }

  .section-items {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .sidebar-item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-lg);
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 13px;
    font-family: var(--font-sans);
    cursor: pointer;
    text-align: left;
    transition: background 0.1s, color 0.1s;
  }

  .sidebar.collapsed .sidebar-item {
    padding: var(--spacing-sm);
    justify-content: center;
  }

  .sidebar-item:hover {
    background: var(--sidebar-item-hover);
    color: var(--text-primary);
  }

  .sidebar-item.active {
    background: var(--sidebar-item-active);
    color: var(--text-primary);
    border-left: 2px solid var(--accent-primary);
    padding-left: calc(var(--spacing-lg) - 2px);
  }

  .sidebar.collapsed .sidebar-item.active {
    padding-left: calc(var(--spacing-sm) - 2px);
  }

  .item-icon {
    position: relative;
    width: 18px;
    text-align: center;
    flex-shrink: 0;
  }

  .item-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-shortcut {
    font-size: 12px;
    color: var(--text-muted);
    font-family: var(--font-mono);
  }

  .badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    font-size: 10px;
    font-weight: 600;
    color: white;
    background: var(--accent-primary);
    border-radius: 9px;
    font-family: var(--font-mono);
  }

  .badge-collapsed {
    position: absolute;
    top: -4px;
    right: -4px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    font-size: 9px;
  }

  .sidebar-footer {
    border-top: 1px solid var(--sidebar-border);
    padding: var(--spacing-sm) 0;
  }
</style>
