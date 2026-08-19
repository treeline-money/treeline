/**
 * Plugin Registration
 *
 * Core plugins are registered here statically.
 * External plugins are loaded dynamically from ~/.treeline/plugins/
 */

import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { registry, themeManager, getDisabledPlugins, getAppSetting, executeQuery, executeQueryWithParams } from "../sdk";
import type { Plugin, PluginContext, PluginMigration } from "../sdk/types";
import { trackActivePlugin, startHotReload } from "./hotReload";
import type { ExternalPluginInfo, LoadedExternalPlugin } from "./types";

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
 *
 * The schema is created even for plugins with no migrations: its existence is
 * how core (tl doctor) tells "installed and loaded once" from "installed but
 * never initialized".
 */
async function runPluginMigrations(
  pluginId: string,
  schemaName: string,
  migrations: PluginMigration[]
): Promise<void> {
  try {
    // 1. Create schema if not exists
    await executeQuery(`CREATE SCHEMA IF NOT EXISTS ${schemaName}`, { readonly: false });

    if (!migrations || migrations.length === 0) {
      // Flush the DDL so the schema is visible to the CLI right away
      await executeQuery("CHECKPOINT", { readonly: false });
      return;
    }

    // Sort migrations by version
    const sortedMigrations = [...migrations].sort((a, b) => a.version - b.version);

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

  // Create a map of discovered manifest ID -> loaded external plugin info
  // The discovered manifest ID (from manifest.json on disk) is the canonical ID for external plugins.
  // This ensures the ID used in registration matches the directory name (which the file watcher uses),
  // even if the JS bundle has a stale/different ID.
  const externalManifestMap = new Map(
    loadedExternalPlugins.map(lep => [lep.discoveredManifest.id, lep])
  );

  // Extract just the plugin objects for the combined list
  const externalPlugins = loadedExternalPlugins.map(lep => lep.plugin);

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
      const isExternal = externalPluginIds.has(plugin.manifest.id);

      // For external plugins, use the discovered manifest ID (from manifest.json on disk)
      // as the canonical ID. This ensures registration matches the directory name
      // (which the file watcher uses for hot-reload), even if the JS bundle has a
      // stale/different ID embedded in it.
      let pluginId: string;
      let permissions: any;
      if (isExternal) {
        const loaded = externalManifestMap.get(plugin.manifest.id)
          // Also try matching by plugin object reference in case JS ID differs from file manifest
          || [...externalManifestMap.values()].find(lep => lep.plugin === plugin);
        if (loaded) {
          pluginId = loaded.discoveredManifest.id;
          permissions = loaded.discoveredManifest.permissions ?? {};
        } else {
          pluginId = plugin.manifest.id;
          permissions = plugin.manifest.permissions ?? {};
        }
      } else {
        pluginId = plugin.manifest.id;
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

      // Ensure the plugin schema exists and run pending migrations (if any)
      // before activation. Always called — the schema's existence is what
      // tells core the plugin has been initialized.
      const schemaName = getPluginSchemaName(pluginId, tablePermissions);
      await runPluginMigrations(pluginId, schemaName, plugin.migrations ?? []);

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

      // Track external plugins for hot-reload deactivation support
      if (isExternal) {
        trackActivePlugin(pluginId, plugin);
      }

      console.log(`✓ Loaded plugin: ${plugin.manifest.name} (${pluginId})`);
    } catch (error) {
      console.error(`✗ Failed to load plugin: ${plugin.manifest.name}`, error);
    }
  }

  // Auto-start hot-reload if the setting is enabled
  try {
    const hotReloadEnabled = await getAppSetting("pluginHotReload");
    if (hotReloadEnabled) {
      await startHotReload();
    }
  } catch (error) {
    console.error("Failed to start plugin hot-reload:", error);
  }
}

/**
 * Get list of all available core plugins (for settings UI)
 */
export function getCorePluginManifests() {
  return corePlugins.map(p => p.manifest);
}
