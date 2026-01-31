/**
 * Plugin Registry - Central state for all plugin registrations
 *
 * This is a Svelte 5 runes-based store that holds all registered
 * sidebar items, views, commands, etc.
 */

import type {
  Plugin,
  PluginContext,
  SidebarSection,
  SidebarItem,
  ViewDefinition,
  Command,
  StatusBarItem,
  Tab,
  DatabaseInterface,
  ThemeInterface,
} from "./types";

// ============================================================================
// State (using Svelte 5 runes via module-level $state)
// ============================================================================

// Since we can't use runes at module level in .ts files,
// we'll use a class with methods that components can call

class PluginRegistry {
  // All registered plugins
  private plugins: Map<string, Plugin> = new Map();

  // Sidebar
  private _sidebarSections: SidebarSection[] = [];
  private _sidebarItems: SidebarItem[] = [];

  // Views
  private _views: Map<string, ViewDefinition> = new Map();

  // View to plugin mapping (for permissions lookup)
  private _viewToPlugin: Map<string, string> = new Map();

  // Plugin permissions (pluginId -> full table permissions)
  private _pluginPermissions: Map<string, { read?: string[]; write?: string[]; schemaName?: string }> = new Map();

  // Commands
  private _commands: Map<string, Command> = new Map();

  // Status bar
  private _statusBarItems: StatusBarItem[] = [];

  // Tabs
  private _tabs: Tab[] = [];
  private _activeTabId: string | null = null;

  // Subscribers for reactivity
  private subscribers: Set<() => void> = new Set();

  // Event subscribers for global events (like data refresh)
  private eventSubscribers: Map<string, Set<() => void>> = new Map();

  // ============================================================================
  // Subscription for reactivity
  // ============================================================================

  subscribe(callback: () => void): () => void {
    this.subscribers.add(callback);
    return () => this.subscribers.delete(callback);
  }

  private notify() {
    this.subscribers.forEach((cb) => cb());
  }

  // ============================================================================
  // Getters
  // ============================================================================

  get sidebarSections(): SidebarSection[] {
    return [...this._sidebarSections].sort((a, b) => a.order - b.order);
  }

  get sidebarItems(): SidebarItem[] {
    return [...this._sidebarItems].sort(
      (a, b) => (a.order ?? 0) - (b.order ?? 0)
    );
  }

  getSidebarItemsForSection(sectionId: string): SidebarItem[] {
    return this.sidebarItems.filter((item) => item.sectionId === sectionId);
  }

  get views(): ViewDefinition[] {
    return Array.from(this._views.values());
  }

  getView(viewId: string): ViewDefinition | undefined {
    return this._views.get(viewId);
  }

  get commands(): Command[] {
    return Array.from(this._commands.values());
  }

  get statusBarItems(): StatusBarItem[] {
    return [...this._statusBarItems].sort((a, b) => a.order - b.order);
  }

  get tabs(): Tab[] {
    return this._tabs;
  }

  get activeTabId(): string | null {
    return this._activeTabId;
  }

  get activeTab(): Tab | null {
    return this._tabs.find((t) => t.id === this._activeTabId) ?? null;
  }

  // ============================================================================
  // Registration methods (called by plugins)
  // ============================================================================

  registerSidebarSection(section: SidebarSection) {
    this._sidebarSections.push(section);
    this.notify();
  }

  registerSidebarItem(item: SidebarItem) {
    this._sidebarItems.push(item);
    this.notify();
  }

  /**
   * Update the badge count for a sidebar item
   * @param itemId The sidebar item ID
   * @param count The badge count (0 or undefined to hide badge)
   */
  updateSidebarBadge(itemId: string, count: number | undefined) {
    const item = this._sidebarItems.find((i) => i.id === itemId);
    if (item) {
      item.badge = count && count > 0 ? count : undefined;
      this.notify();
    }
  }

  registerView(view: ViewDefinition, pluginId?: string) {
    this._views.set(view.id, view);
    if (pluginId) {
      this._viewToPlugin.set(view.id, pluginId);
    }
    this.notify();
  }

  /**
   * Set permissions for a plugin (call before activating)
   */
  setPluginPermissions(pluginId: string, permissions: { read?: string[]; write?: string[]; create?: string[]; schemaName?: string }) {
    this._pluginPermissions.set(pluginId, permissions);
  }

  /**
   * Get the plugin ID for a view
   */
  getPluginIdForView(viewId: string): string | undefined {
    return this._viewToPlugin.get(viewId);
  }

  /**
   * Get full permissions for a plugin
   */
  getPluginPermissions(pluginId: string): { read?: string[]; write?: string[]; create?: string[]; schemaName?: string } {
    return this._pluginPermissions.get(pluginId) ?? {};
  }

  /**
   * Get all installed plugin permissions (for dependency checking)
   */
  getAllPluginPermissions(): Map<string, { read?: string[]; write?: string[]; create?: string[]; schemaName?: string }> {
    return new Map(this._pluginPermissions);
  }

  registerCommand(command: Command) {
    this._commands.set(command.id, command);
    this.notify();
  }

  registerStatusBarItem(item: StatusBarItem) {
    this._statusBarItems.push(item);
    this.notify();
  }

  // ============================================================================
  // Tab management
  // ============================================================================

  openView(viewId: string, props: Record<string, any> = {}) {
    const view = this._views.get(viewId);
    if (!view) {
      console.warn(`View not found: ${viewId}`);
      return;
    }

    // Check if view is already open and doesn't allow multiple
    if (!view.allowMultiple) {
      const existing = this._tabs.find((t) => t.viewId === viewId);
      if (existing) {
        // Update props on existing tab if new props were passed
        if (Object.keys(props).length > 0) {
          existing.props = { ...existing.props, ...props };
        }
        this._activeTabId = existing.id;
        this.notify();
        return;
      }
    }

    // Create new tab
    const tab: Tab = {
      id: `tab-${Date.now()}-${Math.random().toString(36).slice(2)}`,
      viewId,
      title: view.name,
      icon: view.icon,
      props: { ...view.defaultProps, ...props },
    };

    this._tabs.push(tab);
    this._activeTabId = tab.id;
    this.notify();
  }

  closeTab(tabId: string) {
    const index = this._tabs.findIndex((t) => t.id === tabId);
    if (index === -1) return;

    this._tabs.splice(index, 1);

    // If we closed the active tab, activate another
    if (this._activeTabId === tabId) {
      if (this._tabs.length > 0) {
        // Activate the tab to the left, or the first tab
        this._activeTabId = this._tabs[Math.max(0, index - 1)]?.id ?? null;
      } else {
        this._activeTabId = null;
      }
    }

    this.notify();
  }

  setActiveTab(tabId: string) {
    if (this._tabs.find((t) => t.id === tabId)) {
      this._activeTabId = tabId;
      this.notify();
    }
  }

  // ============================================================================
  // Command execution
  // ============================================================================

  executeCommand(commandId: string) {
    const command = this._commands.get(commandId);
    if (command) {
      command.execute();
    } else {
      console.warn(`Command not found: ${commandId}`);
    }
  }

  // ============================================================================
  // Plugin loading
  // ============================================================================

  async loadPlugin(plugin: Plugin, db: DatabaseInterface, theme: ThemeInterface) {
    if (this.plugins.has(plugin.manifest.id)) {
      console.warn(`Plugin already loaded: ${plugin.manifest.id}`);
      return;
    }

    // Create context for this plugin
    const ctx: PluginContext = {
      registerSidebarSection: (s) => this.registerSidebarSection(s),
      registerSidebarItem: (i) => this.registerSidebarItem(i),
      registerView: (v) => this.registerView(v),
      registerCommand: (c) => this.registerCommand(c),
      registerStatusBarItem: (i) => this.registerStatusBarItem(i),
      openView: (id, props) => this.openView(id, props),
      executeCommand: (id) => this.executeCommand(id),
      db,
      theme,
    };

    // Activate the plugin
    await plugin.activate(ctx);
    this.plugins.set(plugin.manifest.id, plugin);

    console.log(`Loaded plugin: ${plugin.manifest.name}`);
  }

  // ============================================================================
  // Global events (for cross-component communication)
  // ============================================================================

  /**
   * Subscribe to a global event
   * @param event Event name (e.g., "data:refresh")
   * @param callback Function to call when event is emitted
   * @returns Unsubscribe function
   */
  on(event: string, callback: () => void): () => void {
    if (!this.eventSubscribers.has(event)) {
      this.eventSubscribers.set(event, new Set());
    }
    this.eventSubscribers.get(event)!.add(callback);
    return () => this.eventSubscribers.get(event)?.delete(callback);
  }

  /**
   * Emit a global event to all subscribers
   * @param event Event name
   */
  emit(event: string): void {
    const callbacks = this.eventSubscribers.get(event);
    if (callbacks) {
      callbacks.forEach((cb) => cb());
    }
  }
}

// Singleton instance
export const registry = new PluginRegistry();
