/**
 * Plugin Updates Store - Track available plugin updates for badge display
 */

import { invoke } from "@tauri-apps/api/core";

export interface PluginUpdateInfo {
  pluginId: string;
  installedVersion: string;
  latestVersion: string;
  source: string;
}

interface InstalledPluginManifest {
  id: string;
  name: string;
  version: string;
  source?: string;
}

class PluginUpdatesStore {
  private _updates = $state<Map<string, PluginUpdateInfo>>(new Map());
  private _isChecking = $state(false);
  private _lastChecked = $state<number | null>(null);

  get updates(): Map<string, PluginUpdateInfo> {
    return this._updates;
  }

  get count(): number {
    return this._updates.size;
  }

  get isChecking(): boolean {
    return this._isChecking;
  }

  get hasUpdates(): boolean {
    return this._updates.size > 0;
  }

  /**
   * Set updates from SettingsModal check
   */
  setUpdates(updates: Map<string, PluginUpdateInfo>): void {
    this._updates = updates;
    this._lastChecked = Date.now();
  }

  /**
   * Clear a specific update (after upgrade)
   */
  clearUpdate(pluginId: string): void {
    const newUpdates = new Map(this._updates);
    newUpdates.delete(pluginId);
    this._updates = newUpdates;
  }

  /**
   * Check for updates on startup - discovers installed plugins automatically
   */
  async checkOnStartup(): Promise<void> {
    if (this._isChecking) return;

    try {
      // Discover installed community plugins
      const installed = await invoke<Array<{ manifest: InstalledPluginManifest; path: string }>>("discover_plugins");
      const communityPlugins = installed
        .filter(p => p.manifest.source?.startsWith("https://github.com/"))
        .map(p => ({ id: p.manifest.id, source: p.manifest.source }));

      if (communityPlugins.length === 0) return;

      await this.checkForUpdates(communityPlugins);
    } catch (e) {
      console.error("Failed to check plugin updates on startup:", e);
    }
  }

  /**
   * Check for updates (called on app load or manually)
   */
  async checkForUpdates(installedPlugins: Array<{ id: string; source?: string }>): Promise<void> {
    if (this._isChecking) return;

    this._isChecking = true;
    const updates = new Map<string, PluginUpdateInfo>();

    try {
      for (const plugin of installedPlugins) {
        if (!plugin.source?.startsWith("https://github.com/")) continue;

        try {
          const result = await invoke<string>("check_plugin_update", {
            pluginId: plugin.id,
          });
          const data = JSON.parse(result);
          if (data.success && data.has_update) {
            updates.set(plugin.id, {
              pluginId: plugin.id,
              installedVersion: data.installed_version,
              latestVersion: data.latest_version,
              source: data.source,
            });
          }
        } catch {
          // Skip plugins that fail to check
        }
      }

      this._updates = updates;
      this._lastChecked = Date.now();
    } finally {
      this._isChecking = false;
    }
  }

  /**
   * Clear all updates
   */
  clear(): void {
    this._updates = new Map();
  }
}

export const pluginUpdatesStore = new PluginUpdatesStore();
