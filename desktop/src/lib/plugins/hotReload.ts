/**
 * Plugin Hot Reload
 *
 * Listens for `plugin-file-changed` events from the Rust file watcher
 * and reloads the affected external plugin without restarting the app.
 *
 * Flow:
 * 1. Rust watches ~/.treeline/plugins/ for changes to index.js / manifest.json
 * 2. On change, Rust emits `plugin-file-changed` with the plugin ID
 * 3. This module receives the event and:
 *    a. Calls plugin.deactivate() if it exists
 *    b. Unregisters all plugin items from the registry
 *    c. Re-discovers the plugin via Tauri (fresh manifest)
 *    d. Re-imports the JS module (with cache-bust query param)
 *    e. Re-runs activation with a fresh PluginContext
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { registry, themeManager, toast, getDisabledPlugins } from "../sdk";
import type { Plugin, PluginContext } from "../sdk/types";

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
      read?: string[];
      write?: string[];
      create?: string[];
      schemaName?: string;
    };
  };
  path: string;
}

// Track active plugins for deactivation during reload
const activePlugins = new Map<string, Plugin>();

let unlisten: UnlistenFn | null = null;

/**
 * Register a plugin as active (called during initial plugin loading).
 * This allows hot-reload to call deactivate() on it before reloading.
 */
export function trackActivePlugin(pluginId: string, plugin: Plugin) {
  activePlugins.set(pluginId, plugin);
}

/**
 * Start listening for plugin file change events from the backend.
 */
export async function startHotReload(): Promise<void> {
  // Don't double-register
  if (unlisten) return;

  unlisten = await listen<string>("plugin-file-changed", async (event) => {
    const pluginId = event.payload;
    console.log(`[hot-reload] Detected change in plugin: ${pluginId}`);

    try {
      await reloadPlugin(pluginId);
      toast.success("Plugin reloaded", `${pluginId} was hot-reloaded`);
    } catch (error) {
      console.error(`[hot-reload] Failed to reload plugin ${pluginId}:`, error);
      toast.error("Plugin reload failed", `${pluginId}: ${error}`);
    }
  });

  console.log("[hot-reload] Listening for plugin file changes");
}

/**
 * Stop listening for plugin file change events.
 */
export function stopHotReload(): void {
  if (unlisten) {
    unlisten();
    unlisten = null;
    console.log("[hot-reload] Stopped listening for plugin file changes");
  }
}

/**
 * Reload a single external plugin by ID.
 */
async function reloadPlugin(pluginId: string): Promise<void> {
  // Check if plugin is disabled - skip reload if so
  const disabledPlugins = await getDisabledPlugins();
  if (disabledPlugins.includes(pluginId)) {
    console.log(`[hot-reload] Plugin ${pluginId} is disabled, skipping reload`);
    return;
  }

  // 1. Deactivate the old plugin instance if it has a deactivate hook
  const oldPlugin = activePlugins.get(pluginId);
  if (oldPlugin?.deactivate) {
    try {
      await oldPlugin.deactivate();
    } catch (e) {
      console.warn(`[hot-reload] deactivate() failed for ${pluginId}:`, e);
    }
  }

  // 2. Unregister all items from the registry
  registry.unregisterPlugin(pluginId);

  // 3. Re-discover the plugin to get a fresh manifest from disk
  const discovered = await invoke<ExternalPluginInfo[]>("discover_plugins");
  const pluginInfo = discovered.find((p) => p.manifest.id === pluginId);

  if (!pluginInfo) {
    // Plugin was deleted - just clean up
    activePlugins.delete(pluginId);
    console.log(`[hot-reload] Plugin ${pluginId} was removed`);
    return;
  }

  // 4. Re-import the JS module with a cache-busting query param
  const pluginsDir = await invoke<string>("get_plugins_dir");
  const pluginPath = `${pluginsDir}/${pluginInfo.manifest.id}/${pluginInfo.manifest.main}`;
  const assetUrl = convertFileSrc(pluginPath);
  const cacheBustedUrl = `${assetUrl}?t=${Date.now()}`;

  console.log(`[hot-reload] Re-importing plugin from: ${cacheBustedUrl}`);
  const module = await import(/* @vite-ignore */ cacheBustedUrl);

  if (!module.plugin) {
    throw new Error(`Plugin ${pluginId} does not export 'plugin'`);
  }

  const plugin: Plugin = module.plugin;

  // 5. Re-register permissions from the fresh manifest
  const permissions = pluginInfo.manifest.permissions ?? {};
  const tablePermissions = {
    read: permissions.read ?? permissions.tables?.read,
    write: permissions.write ?? permissions.tables?.write,
    create: permissions.create ?? permissions.tables?.create,
    schemaName: permissions.schemaName,
  };
  registry.setPluginPermissions(pluginId, tablePermissions);

  // 6. Create a fresh PluginContext and activate
  const context: PluginContext = {
    registerSidebarSection: registry.registerSidebarSection.bind(registry),
    registerSidebarItem: (item) =>
      registry.registerSidebarItem({ ...item, sectionId: "plugins" }),
    registerView: (view) => registry.registerView(view, pluginId),
    registerCommand: registry.registerCommand.bind(registry),
    registerStatusBarItem: registry.registerStatusBarItem.bind(registry),
    openView: registry.openView.bind(registry),
    executeCommand: registry.executeCommand.bind(registry),
    db: {} as any, // Database access is provided via SDK props
    theme: themeManager,
  };

  await plugin.activate(context);

  // 7. Track the new plugin instance
  activePlugins.set(pluginId, plugin);

  console.log(`[hot-reload] Successfully reloaded plugin: ${pluginId}`);
}
