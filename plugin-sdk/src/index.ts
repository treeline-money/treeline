/**
 * Treeline Plugin SDK
 *
 * TypeScript types and interfaces for building Treeline plugins.
 * Install: npm install @treeline-money/plugin-sdk
 *
 * @packageDocumentation
 */

// ============================================================================
// Plugin Manifest - How plugins describe themselves
// ============================================================================

/**
 * Plugin manifest describing the plugin's metadata and permissions.
 */
export interface PluginManifest {
  /** Unique identifier (e.g., "subscriptions", "goals") */
  id: string;

  /** Display name */
  name: string;

  /** Version string (semver) */
  version: string;

  /** Short description */
  description: string;

  /** Author name or organization */
  author: string;

  /** Optional icon (emoji or icon name) */
  icon?: string;

  /** Permissions this plugin requires */
  permissions?: PluginPermissions;
}

/**
 * Permissions a plugin can request.
 */
export interface PluginPermissions {
  /** Table permissions for this plugin */
  tables?: {
    /** Tables this plugin can SELECT from */
    read?: string[];
    /** Tables this plugin can INSERT/UPDATE/DELETE */
    write?: string[];
    /** Tables this plugin can CREATE/DROP (must match sys_plugin_{id}_* pattern) */
    create?: string[];
  };
}

// ============================================================================
// Plugin SDK - The API available to plugin views
// ============================================================================

/**
 * The SDK object passed to plugin views via props.
 *
 * @example
 * ```svelte
 * <script lang="ts">
 *   import type { PluginSDK } from '@treeline-money/plugin-sdk';
 *
 *   interface Props {
 *     sdk: PluginSDK;
 *   }
 *   const { sdk }: Props = $props();
 *
 *   // Query transactions
 *   const transactions = await sdk.query('SELECT * FROM transactions LIMIT 10');
 *
 *   // Show a toast
 *   sdk.toast.success('Data loaded!');
 * </script>
 * ```
 */
export interface PluginSDK {
  /**
   * Execute a read-only SQL query against the database.
   * @param sql - SQL SELECT query
   * @returns Array of row objects
   */
  query: <T = Record<string, unknown>>(sql: string) => Promise<T[]>;

  /**
   * Execute a write SQL query (INSERT/UPDATE/DELETE).
   * Restricted to tables allowed in plugin permissions.
   * @param sql - SQL write query
   * @returns Object with rowsAffected count
   */
  execute: (sql: string) => Promise<{ rowsAffected: number }>;

  /**
   * Toast notification methods.
   */
  toast: {
    /** Show an info toast */
    show: (message: string, description?: string) => void;
    /** Show a success toast */
    success: (message: string, description?: string) => void;
    /** Show an error toast */
    error: (message: string, description?: string) => void;
    /** Show a warning toast */
    warning: (message: string, description?: string) => void;
    /** Show an info toast */
    info: (message: string, description?: string) => void;
  };

  /**
   * Navigate to another view.
   * @param viewId - The view ID to open
   * @param props - Optional props to pass to the view
   */
  openView: (viewId: string, props?: Record<string, unknown>) => void;

  /**
   * Subscribe to data refresh events (called after sync/import).
   * @param callback - Function to call when data is refreshed
   * @returns Unsubscribe function
   */
  onDataRefresh: (callback: () => void) => () => void;

  /**
   * Emit a data refresh event. Call this after modifying data
   * so other views can update.
   */
  emitDataRefresh: () => void;

  /**
   * Update the badge count shown on this plugin's sidebar item.
   * @param count - Badge count (0 or undefined to hide)
   */
  updateBadge: (count: number | undefined) => void;

  /**
   * Theme utilities.
   */
  theme: {
    /** Get current theme ("light" or "dark") */
    current: () => "light" | "dark";
    /** Subscribe to theme changes */
    subscribe: (callback: (theme: string) => void) => () => void;
  };

  /**
   * Platform-aware modifier key display string.
   * Returns "Cmd" on Mac, "Ctrl" on Windows/Linux.
   */
  modKey: "Cmd" | "Ctrl";

  /**
   * Format a keyboard shortcut for display.
   * Converts "mod+p" to "⌘P" on Mac or "Ctrl+P" on Windows.
   * @param shortcut - Shortcut string (e.g., "mod+shift+p")
   */
  formatShortcut: (shortcut: string) => string;

  /**
   * Plugin settings (persisted, scoped to plugin ID).
   */
  settings: {
    /** Get all settings for this plugin */
    get: <T extends Record<string, unknown>>() => Promise<T>;
    /** Save settings for this plugin */
    set: <T extends Record<string, unknown>>(settings: T) => Promise<void>;
  };

  /**
   * Plugin state (ephemeral, scoped to plugin ID).
   * Use for runtime state that doesn't need to persist.
   */
  state: {
    /** Read plugin state */
    read: <T>() => Promise<T | null>;
    /** Write plugin state */
    write: <T>(state: T) => Promise<void>;
  };

  /**
   * Currency formatting utilities.
   */
  currency: {
    /** Format amount with currency symbol (e.g., "$1,234.56") */
    format: (amount: number, currency?: string) => string;
    /** Format compactly for large amounts (e.g., "$1.2M") */
    formatCompact: (amount: number, currency?: string) => string;
    /** Format just the number without symbol (e.g., "1,234.56") */
    formatAmount: (amount: number) => string;
    /** Get symbol for a currency code (e.g., "USD" -> "$") */
    getSymbol: (currency: string) => string;
    /** Get the user's configured currency code */
    getUserCurrency: () => string;
    /** List of supported currency codes */
    supportedCurrencies: string[];
  };
}

// ============================================================================
// Plugin Registration Types
// ============================================================================

/**
 * Sidebar section definition.
 */
export interface SidebarSection {
  /** Section ID */
  id: string;
  /** Section title (shown as header) */
  title: string;
  /** Sort order (lower = higher) */
  order: number;
}

/**
 * Sidebar item definition.
 */
export interface SidebarItem {
  /** Unique ID */
  id: string;
  /** Display label */
  label: string;
  /** Icon (emoji or icon name) */
  icon: string;
  /** Section this belongs to */
  sectionId: string;
  /** View to open when clicked */
  viewId: string;
  /** Keyboard shortcut hint */
  shortcut?: string;
  /** Sort order within section */
  order?: number;
}

/**
 * View definition for plugin views.
 */
export interface ViewDefinition {
  /** Unique view ID */
  id: string;
  /** Display name (shown in tab) */
  name: string;
  /** Icon for tab */
  icon: string;
  /**
   * Mount function that renders into the target element.
   * @param target - DOM element to render into
   * @param props - Props including the SDK
   * @returns Cleanup function to call when unmounting
   */
  mount: (target: HTMLElement, props: { sdk: PluginSDK }) => () => void;
  /** Can multiple instances be open? */
  allowMultiple?: boolean;
}

/**
 * Command definition for the command palette.
 */
export interface Command {
  /** Unique command ID */
  id: string;
  /** Display name */
  name: string;
  /** Optional description */
  description?: string;
  /** Category for grouping */
  category?: string;
  /** Keyboard shortcut */
  shortcut?: string;
  /** Function to execute */
  execute: () => void | Promise<void>;
}

/**
 * Plugin context provided during activation.
 */
export interface PluginContext {
  /** Register a sidebar section */
  registerSidebarSection: (section: SidebarSection) => void;
  /** Register a sidebar item */
  registerSidebarItem: (item: SidebarItem) => void;
  /** Register a view */
  registerView: (view: ViewDefinition) => void;
  /** Register a command */
  registerCommand: (command: Command) => void;
  /** Open a view */
  openView: (viewId: string, props?: Record<string, unknown>) => void;
}

/**
 * Plugin interface that all plugins must implement.
 */
export interface Plugin {
  /** Plugin manifest */
  manifest: PluginManifest;
  /** Called when plugin is activated */
  activate: (ctx: PluginContext) => void | Promise<void>;
  /** Called when plugin is deactivated */
  deactivate?: () => void | Promise<void>;
}
