/**
 * Treeline Plugin SDK - Type Definitions
 *
 * This is the contract between the core shell and plugins.
 * Plugins implement these interfaces to extend the application.
 */

import type { Component } from "svelte";

// ============================================================================
// Plugin Manifest - How plugins describe themselves
// ============================================================================

export interface PluginManifest {
  /** Unique identifier (e.g., "transactions", "fire-calculator") */
  id: string;

  /** Display name */
  name: string;

  /** Version string */
  version: string;

  /** Short description */
  description: string;

  /** Author name or organization */
  author: string;

  /** Optional icon (emoji or icon name) */
  icon?: string;

  /**
   * Permissions this plugin requires.
   * Core plugins can request any tables.
   * Community plugins can only request sys_plugin_{id}_* tables.
   */
  permissions?: PluginPermissions;
}

export interface PluginPermissions {
  /**
   * Tables this plugin can SELECT from (outside its own schema).
   * Use "*" for unrestricted read access (e.g., query plugin).
   *
   * @example ["transactions", "accounts", "sys_balance_snapshots"]
   */
  read?: string[];

  /**
   * Tables this plugin can explicitly write to (outside its own schema).
   * Most plugins don't need this - they only write to their own schema.
   *
   * @example ["sys_transactions"] // tagging plugin modifies transaction tags
   */
  write?: string[];

  /**
   * Tables this plugin creates (for tracking what to clean up on uninstall).
   */
  create?: string[];

  /**
   * Optional schema name override.
   * Default: plugin_<id with hyphens replaced by underscores>
   *
   * @example "plugin_cashflow" for plugin ID "treeline-cashflow"
   */
  schemaName?: string;

  // DEPRECATED: Old format - will be removed in future version
  tables?: {
    read?: string[];
    write?: string[];
    create?: string[];
  };
}

// ============================================================================
// Sidebar - How plugins add navigation items
// ============================================================================

export interface SidebarSection {
  /** Section ID */
  id: string;

  /** Section title (shown as header) */
  title: string;

  /** Sort order (lower = higher in sidebar) */
  order: number;
}

export interface SidebarItem {
  /** Unique ID for this item */
  id: string;

  /** Display label */
  label: string;

  /** Icon (emoji or icon name) */
  icon: string;

  /** Which section this belongs to */
  sectionId: string;

  /** The view to open when clicked */
  viewId: string;

  /** Optional keyboard shortcut hint */
  shortcut?: string;

  /** Sort order within section */
  order?: number;

  /** Optional badge count (e.g., unread count). Set via registry.updateSidebarBadge() */
  badge?: number;
}

// ============================================================================
// Views - The main content areas plugins provide
// ============================================================================

export interface ViewDefinition {
  /** Unique view ID (e.g., "transactions-list", "query-editor") */
  id: string;

  /** Display name (shown in tab) */
  name: string;

  /** Icon for tab */
  icon: string;

  /** The Svelte component to render OR mount/unmount functions for external plugins */
  component?: Component<any>;

  /**
   * For external plugins: mount function that renders into the target element
   * @param target - DOM element to render into
   * @param props - Props to pass to the component
   * @returns cleanup function to call when unmounting
   */
  mount?: (target: HTMLElement, props: Record<string, any>) => () => void;

  /** Can multiple instances be open? */
  allowMultiple?: boolean;

  /** Default view props */
  defaultProps?: Record<string, any>;
}

// ============================================================================
// Commands - Actions that can be triggered via command palette
// ============================================================================

export interface Command {
  /** Unique command ID (e.g., "transactions:search", "sync:run") */
  id: string;

  /** Display name in command palette */
  name: string;

  /** Optional category for grouping */
  category?: string;

  /** Keyboard shortcut (e.g., "mod+shift+t") */
  shortcut?: string;

  /** The function to execute */
  execute: () => void | Promise<void>;
}

// ============================================================================
// Status Bar - Items shown at bottom of window
// ============================================================================

export interface StatusBarItem {
  /** Unique ID */
  id: string;

  /** Position: left or right side */
  position: "left" | "right";

  /** Sort order within position */
  order: number;

  /** The component to render */
  component: Component<any>;
}

// ============================================================================
// Plugin Migrations - Database schema management
// ============================================================================

/**
 * A single database migration for a plugin.
 * Migrations are run in order by version number when the plugin loads.
 */
export interface PluginMigration {
  /** Version number (must be a positive integer, run in order) */
  version: number;

  /** Human-readable name for this migration */
  name: string;

  /** SQL to execute (can be multiple statements separated by semicolons) */
  up: string;
}

// ============================================================================
// Plugin Interface - What plugins must implement
// ============================================================================

export interface Plugin {
  /** Plugin manifest */
  manifest: PluginManifest;

  /**
   * Database migrations for this plugin.
   * Run in order by version number when the plugin loads.
   * The app creates the plugin's schema automatically before running migrations.
   */
  migrations?: PluginMigration[];

  /** Called when plugin is activated */
  activate(ctx: PluginContext): void | Promise<void>;

  /** Called when plugin is deactivated */
  deactivate?(): void | Promise<void>;
}

// ============================================================================
// Plugin Context - What the core provides to plugins
// ============================================================================

export interface PluginContext {
  /** Register sidebar sections */
  registerSidebarSection(section: SidebarSection): void;

  /** Register sidebar items */
  registerSidebarItem(item: SidebarItem): void;

  /** Register views */
  registerView(view: ViewDefinition): void;

  /** Register commands */
  registerCommand(command: Command): void;

  /** Register status bar items */
  registerStatusBarItem(item: StatusBarItem): void;

  /** Open a view in a new tab */
  openView(viewId: string, props?: Record<string, any>): void;

  /** Execute a command by ID */
  executeCommand(commandId: string): void;

  /** Database access */
  db: DatabaseInterface;

  /** Theme access */
  theme: ThemeInterface;
}

// ============================================================================
// Database Interface - How plugins query data
// ============================================================================

export interface DatabaseInterface {
  /** Execute a read query */
  query<T = Record<string, any>>(sql: string): Promise<T[]>;

  /** Execute a write query (for plugin tables only) */
  execute(sql: string): Promise<{ rowsAffected: number }>;

  /** Run plugin migrations */
  migrate(pluginId: string, migrations: Migration[]): Promise<void>;
}

export interface Migration {
  /** Version number (must be sequential) */
  version: number;

  /** Description */
  description: string;

  /** SQL to run */
  sql: string;
}

// ============================================================================
// Theme Interface - How plugins access theme
// ============================================================================

export interface ThemeInterface {
  /** Current theme ID */
  current: string;

  /** Subscribe to theme changes */
  subscribe(callback: (themeId: string) => void): () => void;

  /** Get a CSS variable value */
  getVar(name: string): string;
}

// ============================================================================
// Tab - Internal representation of an open tab
// ============================================================================

export interface Tab {
  /** Unique tab instance ID */
  id: string;

  /** The view this tab shows */
  viewId: string;

  /** Tab title */
  title: string;

  /** Tab icon */
  icon: string;

  /** Props passed to the view component */
  props: Record<string, any>;
}
