/**
 * Plugin Hot-Reload
 *
 * Watches for file changes in external plugins and reloads them without restarting the app.
 * Uses the Rust file watcher backend to detect changes in ~/.treeline/plugins/.
 */

import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { registry } from "../sdk/registry";
import { themeManager } from "../sdk/theme";
import { getDisabledPlugins } from "../sdk/settings";
import type { Plugin, PluginContext } from "../sdk/types";
import type { ExternalPluginInfo } from "./types";

// Active plugin instances for deactivation during reload
const activePlugins = new Map<string, Plugin>();

// Event listener cleanup
let unlisten: UnlistenFn | null = null;

// Reloads run one at a time. Concurrent reloadPlugin calls interleave their
// unregister/activate steps across awaits, which double-registers sidebar
// items — a duplicate key in Svelte's keyed each throws inside the render
// effect and kills the whole UI. A pull rewriting many plugins at once fires
// a burst of change events, so serialize and dedupe them.
let reloadChain: Promise<void> = Promise.resolve();
const queuedReloads = new Set<string>();

function queueReload(pluginId: string): void {
  if (queuedReloads.has(pluginId)) return;
  queuedReloads.add(pluginId);
  reloadChain = reloadChain.then(async () => {
    queuedReloads.delete(pluginId);
    if (!unlisten) return; // hot-reload was stopped while queued
    console.log(`[hot-reload] Detected change in plugin: ${pluginId}`);
    try {
      await reloadPlugin(pluginId);
    } catch (error) {
      console.error(`[hot-reload] Failed to reload plugin ${pluginId}:`, error);
    }
  });
}

/**
 * Track an active plugin instance so we can call deactivate() on reload.
 * Called from initializePlugins() after a plugin is activated.
 */
export function trackActivePlugin(pluginId: string, plugin: Plugin): void {
  activePlugins.set(pluginId, plugin);
}

/**
 * Start hot-reload: watch the plugins directory and listen for change events.
 */
export async function startHotReload(): Promise<void> {
  // Start the file watcher on the Rust side
  await invoke("watch_plugins_dir");

  // Listen for change events from the backend
  unlisten = await listen<string>("plugin-file-changed", (event) => {
    queueReload(event.payload);
  });

  console.log("[hot-reload] Plugin hot-reload started");
}

/**
 * Stop hot-reload: stop the file watcher and remove event listener.
 */
export async function stopHotReload(): Promise<void> {
  if (unlisten) {
    unlisten();
    unlisten = null;
  }

  try {
    await invoke("unwatch_plugins_dir");
  } catch (error) {
    console.error("[hot-reload] Failed to stop file watcher:", error);
  }

  console.log("[hot-reload] Plugin hot-reload stopped");
}

/**
 * Reload a single external plugin by ID.
 */
async function reloadPlugin(pluginId: string): Promise<void> {
  // Skip disabled plugins
  const disabledPlugins = await getDisabledPlugins();
  if (disabledPlugins.includes(pluginId)) {
    console.log(`[hot-reload] Skipping disabled plugin: ${pluginId}`);
    return;
  }

  // 1. Call deactivate() on the old plugin instance if it exists
  const oldPlugin = activePlugins.get(pluginId);
  if (oldPlugin?.deactivate) {
    try {
      await oldPlugin.deactivate();
    } catch (error) {
      console.warn(`[hot-reload] deactivate() failed for ${pluginId}:`, error);
    }
  }

  // 2. Unregister everything except tabs — keeps the tab bar stable during reload.
  //    The tabs reference viewIds that will be re-registered momentarily.
  registry.unregisterPlugin(pluginId, { keepTabs: true });
  activePlugins.delete(pluginId);

  // 4. Re-discover plugins to get fresh manifest from disk
  const discovered = await invoke<ExternalPluginInfo[]>("discover_plugins");
  const pluginInfo = discovered.find((p) => p.manifest.id === pluginId);

  if (!pluginInfo) {
    // Plugin was deleted — close the orphaned tabs we kept open
    const orphanTabs = registry.tabs.filter((t) => !registry.hasView(t.viewId));
    for (const tab of orphanTabs) {
      registry.closeTab(tab.id);
    }
    console.log(`[hot-reload] Plugin ${pluginId} removed (no longer on disk)`);
    return;
  }

  // 5. Re-import the JS module with cache-busting query parameter
  const pluginsDir = await invoke<string>("get_plugins_dir");
  const pluginPath = `${pluginsDir}/${pluginInfo.manifest.id}/${pluginInfo.manifest.main}`;
  const assetUrl = convertFileSrc(pluginPath);
  const cacheBustedUrl = `${assetUrl}?t=${Date.now()}`;

  let module: any;
  try {
    module = await import(/* @vite-ignore */ cacheBustedUrl);
  } catch (error) {
    console.error(`[hot-reload] Failed to import ${pluginId}:`, error);
    return;
  }

  if (!module.plugin) {
    console.error(`[hot-reload] Plugin ${pluginId} does not export 'plugin'`);
    return;
  }

  const plugin: Plugin = module.plugin;

  // NOTE: Migrations are intentionally skipped during hot-reload. Running them on every
  // file save would be destructive — a half-typed migration would execute and be recorded
  // as completed, with no rollback mechanism. Developers should restart the app to run
  // new migrations.

  // 6. Register permissions from the fresh manifest
  const permissions = pluginInfo.manifest.permissions ?? {};
  const tablePermissions = {
    read: permissions.read ?? permissions.tables?.read,
    write: permissions.write ?? permissions.tables?.write,
    create: permissions.create ?? permissions.tables?.create,
    schemaName: permissions.schemaName,
  };
  registry.setPluginPermissions(pluginId, tablePermissions);

  // 7. Create a fresh PluginContext and activate
  const context: PluginContext = {
    registerSidebarSection: registry.registerSidebarSection.bind(registry),
    registerSidebarItem: (item) =>
      registry.registerSidebarItem({ ...item, sectionId: "plugins" }),
    registerView: (view) => registry.registerView(view, pluginId),
    registerCommand: registry.registerCommand.bind(registry),
    registerStatusBarItem: registry.registerStatusBarItem.bind(registry),
    openView: registry.openView.bind(registry),
    executeCommand: registry.executeCommand.bind(registry),
    db: {} as any,
    theme: themeManager,
  };

  await plugin.activate(context);
  activePlugins.set(pluginId, plugin);

  // 8. Notify so ContentArea remounts the plugin's existing tabs with the fresh view
  //    Tabs were kept open — they already reference the correct viewId, which now
  //    points to the freshly registered mount function.
  console.log(`[hot-reload] Reloaded plugin: ${plugin.manifest.name} (${pluginId})`);
}
