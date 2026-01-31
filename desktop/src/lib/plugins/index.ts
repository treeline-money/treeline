/**
 * Plugin Registration
 *
 * Core plugins are registered here statically.
 * External plugins are loaded dynamically from ~/.treeline/plugins/
 */

import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { registry, themeManager, getDisabledPlugins, executeQuery, executeQueryWithParams } from "../sdk";
import type { Plugin, PluginContext, PluginMigration } from "../sdk/types";

// Import core plugins
import { plugin as queryPlugin } from "./query";
import { plugin as transactionsPlugin } from "./transactions";
import { plugin as accountsPlugin } from "./accounts";

// List of core plugins (built into the app)
// Budget is now an external plugin - only 3 core plugins remain
const corePlugins: Plugin[] = [accountsPlugin, transactionsPlugin, queryPlugin];

// ============================================================================
// Plugin Migration Runner
// ============================================================================

/**
 * Get the schema name for a plugin (from permissions or derived from ID)
 */
function getPluginSchemaName(pluginId: string, permissions?: { schemaName?: string }): string {
  return permissions?.schemaName || `plugin_${pluginId.replace(/-/g, "_")}`;
}

/**
 * Run pending migrations for a plugin.
 * Creates the schema and schema_migrations table if needed.
 */
async function runPluginMigrations(
  pluginId: string,
  schemaName: string,
  migrations: PluginMigration[]
): Promise<void> {
  if (!migrations || migrations.length === 0) {
    return;
  }

  // Sort migrations by version
  const sortedMigrations = [...migrations].sort((a, b) => a.version - b.version);

  try {
    // 1. Create schema if not exists
    await executeQuery(`CREATE SCHEMA IF NOT EXISTS ${schemaName}`, { readonly: false });

    // 2. Create schema_migrations table if not exists
    // Note: No DEFAULT on executed_at to avoid WAL replay issues with function defaults
    await executeQuery(`
      CREATE TABLE IF NOT EXISTS ${schemaName}.schema_migrations (
        version INTEGER PRIMARY KEY,
        name VARCHAR NOT NULL,
        executed_at TIMESTAMP
      )
    `, { readonly: false });

    // 3. Get current max version
    const result = await executeQuery(
      `SELECT COALESCE(MAX(version), 0) FROM ${schemaName}.schema_migrations`
    );
    const currentVersion = (result.rows[0]?.[0] as number) || 0;

    // 4. Run pending migrations
    const pendingMigrations = sortedMigrations.filter(m => m.version > currentVersion);

    if (pendingMigrations.length === 0) {
      return;
    }

    console.log(`Running ${pendingMigrations.length} migration(s) for plugin ${pluginId}...`);

    for (const migration of pendingMigrations) {
      try {
        // Execute the migration SQL
        // Split by semicolon to handle multiple statements
        const statements = migration.up
          .split(';')
          .map((s: string) => s.trim())
          .filter((s: string) => s.length > 0);

        for (const statement of statements) {
          await executeQuery(statement, { readonly: false });
        }

        // Record the migration (use JS timestamp to avoid ICU dependency)
        const now = new Date().toISOString();
        await executeQueryWithParams(
          `INSERT INTO ${schemaName}.schema_migrations (version, name, executed_at) VALUES (?, ?, ?::TIMESTAMP)`,
          [migration.version, migration.name, now],
          { readonly: false }
        );

        console.log(`  ✓ Migration ${migration.version}: ${migration.name}`);
      } catch (error) {
        console.error(`  ✗ Migration ${migration.version} failed:`, error);
        throw new Error(`Migration ${migration.version} (${migration.name}) failed: ${error}`);
      }
    }

    // Force checkpoint after migrations to flush DDL to main database file.
    // This prevents WAL replay issues with CREATE/ALTER TABLE that have function defaults
    // (DuckDB bug: WAL replay can't resolve functions like CURRENT_TIMESTAMP, uuid()).
    await executeQuery("CHECKPOINT", { readonly: false });
  } catch (error) {
    console.error(`Failed to run migrations for plugin ${pluginId}:`, error);
    throw error;
  }
}

interface ExternalPluginInfo {
  manifest: {
    id: string;
    name: string;
    version: string;
    description: string;
    author: string;
    main: string;
    permissions?: {
      tables?: {
        read?: string[];
        write?: string[];
        create?: string[];
      };
      // Direct format (alternative to tables.read/write)
      read?: string[];
      write?: string[];
      create?: string[];
      schemaName?: string;
    };
  };
  path: string;
}

interface LoadedExternalPlugin {
  plugin: Plugin;
  discoveredManifest: ExternalPluginInfo["manifest"];
}

/**
 * Load external plugins from ~/.treeline/plugins/
 * Returns both the plugin module and the discovered manifest (from manifest.json file)
 * so we can use the file-based permissions rather than bundled ones.
 */
async function loadExternalPlugins(): Promise<LoadedExternalPlugin[]> {
  try {
    // Get the plugins directory path
    const pluginsDir = await invoke<string>("get_plugins_dir");

    // Discover all available plugins (reads manifest.json files)
    const discovered = await invoke<ExternalPluginInfo[]>("discover_plugins");
    const plugins: LoadedExternalPlugin[] = [];

    for (const pluginInfo of discovered) {
      try {
        // Construct the full path to the plugin file
        const pluginPath = `${pluginsDir}/${pluginInfo.manifest.id}/${pluginInfo.manifest.main}`;

        // Convert to asset URL that Tauri can load
        const assetUrl = convertFileSrc(pluginPath);

        console.log(`Loading external plugin from: ${assetUrl}`);

        // Dynamically import the plugin module
        const module = await import(/* @vite-ignore */ assetUrl);

        if (module.plugin) {
          plugins.push({
            plugin: module.plugin,
            discoveredManifest: pluginInfo.manifest,
          });
          console.log(`✓ Discovered external plugin: ${pluginInfo.manifest.name}`);
        } else {
          console.error(`✗ External plugin ${pluginInfo.manifest.id} does not export 'plugin'`);
        }
      } catch (error) {
        console.error(`✗ Failed to load external plugin ${pluginInfo.manifest.id}:`, error);
      }
    }

    return plugins;
  } catch (error) {
    console.error("Failed to discover external plugins:", error);
    return [];
  }
}

/**
 * Initialize all plugins (core + external)
 */
export async function initializePlugins(): Promise<void> {
  // Load external plugins (with discovered manifest info)
  const loadedExternalPlugins = await loadExternalPlugins();

  // Get list of disabled plugins
  const disabledPlugins = await getDisabledPlugins();

  // Extract just the plugin objects for the combined list
  const externalPlugins = loadedExternalPlugins.map(lep => lep.plugin);

  // Create a map of external plugin ID -> discovered manifest (from manifest.json file)
  // This is used to get permissions from the file rather than the bundled JS
  const externalManifestMap = new Map(
    loadedExternalPlugins.map(lep => [lep.plugin.manifest.id, lep.discoveredManifest])
  );

  // Combine core and external plugins
  const allPlugins = [...corePlugins, ...externalPlugins];

  console.log(`Initializing ${allPlugins.length} plugin(s) (${corePlugins.length} core, ${externalPlugins.length} external)...`);
  if (disabledPlugins.length > 0) {
    console.log(`Disabled plugins: ${disabledPlugins.join(", ")}`);
  }

  // Register core sidebar sections
  registry.registerSidebarSection({
    id: "main",
    title: "Core",
    order: 1,
  });

  // Register plugins section (only if we have external plugins)
  if (externalPlugins.length > 0) {
    registry.registerSidebarSection({
      id: "plugins",
      title: "Plugins",
      order: 10,
    });
  }

  // Track which plugins are external
  const externalPluginIds = new Set(externalPlugins.map(p => p.manifest.id));

  for (const plugin of allPlugins) {
    // Skip disabled plugins
    if (disabledPlugins.includes(plugin.manifest.id)) {
      console.log(`⊘ Skipped disabled plugin: ${plugin.manifest.name} (${plugin.manifest.id})`);
      continue;
    }

    try {
      const pluginId = plugin.manifest.id;
      const isExternal = externalPluginIds.has(pluginId);

      // Register plugin permissions (read/write/schemaName)
      // For external plugins, use permissions from the manifest.json file (not bundled JS)
      // This ensures locally-installed plugins get their updated permissions
      let permissions;
      if (isExternal) {
        const discoveredManifest = externalManifestMap.get(pluginId);
        permissions = discoveredManifest?.permissions ?? {};
      } else {
        permissions = plugin.manifest.permissions ?? {};
      }
      // Extract read/write/create arrays (new format has them directly, old format had tables.read/write)
      const tablePermissions = {
        read: permissions.read ?? permissions.tables?.read,
        write: permissions.write ?? permissions.tables?.write,
        create: permissions.create ?? permissions.tables?.create,
        schemaName: permissions.schemaName,
      };
      registry.setPluginPermissions(pluginId, tablePermissions);

      // Run plugin migrations (if any) before activation
      const schemaName = getPluginSchemaName(pluginId, tablePermissions);
      if (plugin.migrations && plugin.migrations.length > 0) {
        await runPluginMigrations(pluginId, schemaName, plugin.migrations);
      }

      // Create context with plugin API
      const context: PluginContext = {
        registerSidebarSection: registry.registerSidebarSection.bind(registry),
        // External plugins get their sidebar items redirected to "plugins" section
        registerSidebarItem: isExternal
          ? (item) => registry.registerSidebarItem({ ...item, sectionId: "plugins" })
          : registry.registerSidebarItem.bind(registry),
        // Pass pluginId to registerView for permission tracking
        registerView: (view) => registry.registerView(view, pluginId),
        registerCommand: registry.registerCommand.bind(registry),
        registerStatusBarItem: registry.registerStatusBarItem.bind(registry),
        openView: registry.openView.bind(registry),
        executeCommand: registry.executeCommand.bind(registry),
        db: {} as any, // Database access is provided via SDK props
        theme: themeManager,
      };

      // Activate plugin
      await plugin.activate(context);

      console.log(`✓ Loaded plugin: ${plugin.manifest.name} (${plugin.manifest.id})`);
    } catch (error) {
      console.error(`✗ Failed to load plugin: ${plugin.manifest.name}`, error);
    }
  }
}

/**
 * Get list of all available core plugins (for settings UI)
 */
export function getCorePluginManifests() {
  return corePlugins.map(p => p.manifest);
}
