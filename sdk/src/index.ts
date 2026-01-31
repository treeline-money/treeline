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
 *
 * Plugins automatically have full read/write access to their own schema (plugin_<id>).
 * These permissions are for accessing tables OUTSIDE the plugin's own schema.
 */
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
// Plugin SDK - The API available to plugin views
// ============================================================================

/**
 * Query parameter type - supports primitives and arrays.
 * Use with parameterized queries to prevent SQL injection.
 */
export type QueryParam = string | number | boolean | null | string[] | number[];

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
 *   // Query with parameterized values (SAFE - recommended)
 *   const transactions = await sdk.query(
 *     'SELECT * FROM transactions WHERE amount > ? AND description LIKE ?',
 *     [100, '%coffee%']
 *   );
 *
 *   // Show a toast
 *   sdk.toast.success('Data loaded!');
 * </script>
 * ```
 */
export interface PluginSDK {
  /**
   * Execute a read-only SQL query against the database.
   * Use parameterized queries (?) for user-provided values to prevent SQL injection.
   *
   * @param sql - SQL SELECT query with ? placeholders
   * @param params - Optional array of values to bind to ? placeholders
   * @returns Array of row objects
   *
   * @example
   * // Parameterized query (SAFE)
   * const results = await sdk.query(
   *   'SELECT * FROM transactions WHERE amount > ?',
   *   [100]
   * );
   */
  query: <T = Record<string, unknown>>(sql: string, params?: QueryParam[]) => Promise<T[]>;

  /**
   * Execute a write SQL query (INSERT/UPDATE/DELETE/CREATE/DROP).
   * Plugins have full write access to their own schema (plugin_<id>).
   * Use parameterized queries (?) for user-provided values to prevent SQL injection.
   *
   * @param sql - SQL write query with ? placeholders
   * @param params - Optional array of values to bind to ? placeholders
   * @returns Object with rowsAffected count
   *
   * @example
   * // Parameterized insert to own schema (SAFE)
   * const schema = sdk.getSchemaName();
   * await sdk.execute(
   *   `INSERT INTO ${schema}.goals (id, name) VALUES (?, ?)`,
   *   [crypto.randomUUID(), 'Emergency Fund']
   * );
   */
  execute: (sql: string, params?: QueryParam[]) => Promise<{ rowsAffected: number }>;

  /**
   * Get the schema name for this plugin.
   * Tables should be created in this schema: `${sdk.getSchemaName()}.table_name`
   *
   * @returns The plugin's schema name (e.g., "plugin_goals", "plugin_budget")
   *
   * @example
   * const schema = sdk.getSchemaName(); // "plugin_goals"
   * await sdk.execute(`CREATE TABLE IF NOT EXISTS ${schema}.goals (...)`);
   */
  getSchemaName: () => string;

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

// ============================================================================
// Plugin Migrations
// ============================================================================

/**
 * A single database migration for a plugin.
 * Migrations are run in order by version number when the plugin loads.
 *
 * The app automatically creates `plugin_<id>.schema_migrations` to track
 * which migrations have been run.
 *
 * @example
 * ```typescript
 * const migrations: PluginMigration[] = [
 *   {
 *     version: 1,
 *     name: "create_goals_table",
 *     up: `
 *       CREATE TABLE plugin_goals.goals (
 *         id VARCHAR PRIMARY KEY,
 *         name VARCHAR NOT NULL
 *       )
 *     `
 *   },
 *   {
 *     version: 2,
 *     name: "add_priority_column",
 *     up: `ALTER TABLE plugin_goals.goals ADD COLUMN priority INTEGER DEFAULT 0`
 *   }
 * ];
 * ```
 */
export interface PluginMigration {
  /**
   * Unique version number. Must be a positive integer.
   * Migrations run in order of version number.
   */
  version: number;

  /**
   * Human-readable name for this migration.
   * Used in logs and the schema_migrations table.
   */
  name: string;

  /**
   * SQL to execute for this migration.
   * Can be a single statement or multiple statements separated by semicolons.
   * The plugin's schema is created automatically before migrations run.
   */
  up: string;
}

/**
 * Plugin interface that all plugins must implement.
 */
export interface Plugin {
  /** Plugin manifest */
  manifest: PluginManifest;

  /**
   * Database migrations for this plugin.
   * Run in order by version number when the plugin loads.
   * The app creates the plugin's schema automatically before running migrations.
   *
   * @example
   * ```typescript
   * export const plugin: Plugin = {
   *   manifest: { ... },
   *   migrations: [
   *     { version: 1, name: "initial", up: "CREATE TABLE plugin_goals.goals (...)" },
   *     { version: 2, name: "add_index", up: "CREATE INDEX ..." },
   *   ],
   *   activate(ctx) { ... }
   * }
   * ```
   */
  migrations?: PluginMigration[];

  /** Called when plugin is activated (after migrations complete) */
  activate: (ctx: PluginContext) => void | Promise<void>;
  /** Called when plugin is deactivated */
  deactivate?: () => void | Promise<void>;
}
