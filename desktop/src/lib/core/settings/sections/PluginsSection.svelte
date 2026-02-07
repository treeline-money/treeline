<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { invoke } from "@tauri-apps/api/core";
  import { marked } from "marked";
  import { Icon } from "../../../shared";
  import {
    installPlugin,
    uninstallPlugin,
    executeQuery,
    createBackup,
    registry,
    toast,
    themeManager,
  } from "../../../sdk";
  import "../settings-shared.css";

  interface PluginInfo {
    id: string;
    name: string;
    description: string;
    enabled: boolean;
  }

  interface CommunityPluginInfo {
    id: string;
    name: string;
    description: string;
    author: string;
    repo: string;
  }

  interface InstalledPluginInfo {
    id: string;
    name: string;
    version: string;
    description: string;
    author: string;
  }

  interface PluginUpdateInfo {
    pluginId: string;
    installedVersion: string;
    latestVersion: string;
    source: string;
  }

  interface Props {
    plugins: PluginInfo[];
    communityPlugins: CommunityPluginInfo[];
    installedCommunityPlugins: InstalledPluginInfo[];
    isLoadingCommunityPlugins: boolean;
    pluginUpdates: Map<string, PluginUpdateInfo>;
    pluginsNeedReload: boolean;
    onTogglePlugin: (pluginId: string, enabled: boolean) => void;
    onPluginsChanged: () => void;
    onRestartApp: () => void;
  }

  let {
    plugins,
    communityPlugins,
    installedCommunityPlugins,
    isLoadingCommunityPlugins,
    pluginUpdates,
    pluginsNeedReload,
    onTogglePlugin,
    onPluginsChanged,
    onRestartApp,
  }: Props = $props();

  // Local state for plugin actions
  let installingPluginId = $state<string | null>(null);
  let uninstallingPluginId = $state<string | null>(null);
  let loadingManifestPluginId = $state<string | null>(null);
  let upgradingPluginId = $state<string | null>(null);

  // Plugin detail modal state
  interface PluginDetail {
    id: string;
    name: string;
    description: string;
    author: string;
    repo: string;
    version?: string;
    installed: boolean;
  }
  let selectedPlugin = $state<PluginDetail | null>(null);
  let pluginReadme = $state<string | null>(null);
  let isLoadingReadme = $state(false);
  let currentTheme = $state(themeManager.current);

  // Theme subscription
  $effect(() => {
    const unsubscribe = themeManager.subscribe((theme) => {
      currentTheme = theme;
    });
    return unsubscribe;
  });

  // Uninstall confirmation state
  interface UninstallConfirmation {
    plugin: InstalledPluginInfo;
    schemaName: string | null;
    dependentPlugins: { pluginId: string; pluginName: string; tables: string[] }[];
  }
  let uninstallConfirmation = $state<UninstallConfirmation | null>(null);
  let deletePluginData = $state(false);

  // Install confirmation state
  interface InstallConfirmation {
    plugin: CommunityPluginInfo;
    permissions: {
      read?: string[];
      write?: string[];
    };
  }
  let installConfirmation = $state<InstallConfirmation | null>(null);

  // Helper functions
  function isPluginInstalled(pluginId: string): boolean {
    return installedCommunityPlugins.some(p => p.id === pluginId);
  }

  function getInstalledVersion(pluginId: string): string | null {
    const installed = installedCommunityPlugins.find(p => p.id === pluginId);
    return installed?.version || null;
  }

  // Fetch manifest for permissions
  async function fetchPluginManifest(repo: string): Promise<{ read?: string[]; write?: string[] } | null> {
    try {
      const resultStr = await invoke<string>("fetch_plugin_manifest", { url: repo });
      const result = JSON.parse(resultStr);
      if (!result.success) return null;
      const perms = result.manifest?.permissions;
      return { read: perms?.read, write: perms?.write };
    } catch (e) {
      console.error("Failed to fetch plugin manifest:", e);
      return null;
    }
  }

  // Show install confirmation
  async function showInstallConfirmation(plugin: CommunityPluginInfo) {
    loadingManifestPluginId = plugin.id;
    try {
      const permissions = await fetchPluginManifest(plugin.repo);
      if (permissions) {
        installConfirmation = { plugin, permissions };
      } else {
        await executeInstallPlugin(plugin);
      }
    } catch (e) {
      await executeInstallPlugin(plugin);
    } finally {
      loadingManifestPluginId = null;
    }
  }

  function cancelInstall() {
    installConfirmation = null;
  }

  async function executeInstallPlugin(plugin: CommunityPluginInfo) {
    installingPluginId = plugin.id;
    installConfirmation = null;
    try {
      // Create automatic backup before plugin install
      try {
        await createBackup(10);
      } catch (e) {
        console.warn("Pre-install backup failed:", e);
      }
      const result = await installPlugin(plugin.repo);
      if (result.success) {
        toast.success("Plugin installed", `${result.plugin_name} ${result.version} installed`);
        onPluginsChanged();
      } else {
        throw new Error(result.error || "Unknown error");
      }
    } catch (e) {
      toast.error("Failed to install plugin", e instanceof Error ? e.message : String(e));
    } finally {
      installingPluginId = null;
    }
  }

  async function handleInstallPlugin(plugin: CommunityPluginInfo) {
    await showInstallConfirmation(plugin);
  }

  // Find dependent plugins
  function findDependentPlugins(schemaName: string, excludePluginId: string): { pluginId: string; pluginName: string; tables: string[] }[] {
    const dependent: { pluginId: string; pluginName: string; tables: string[] }[] = [];
    const allPermissions = registry.getAllPluginPermissions();

    for (const [pluginId, permissions] of allPermissions) {
      if (pluginId === excludePluginId) continue;
      const readTables = permissions.read || [];
      const overlappingTables = readTables.filter(t =>
        t.toLowerCase().startsWith(schemaName.toLowerCase() + ".")
      );
      if (overlappingTables.length > 0) {
        const installedPlugin = installedCommunityPlugins.find(p => p.id === pluginId);
        const corePlugin = plugins.find(p => p.id === pluginId);
        const pluginName = installedPlugin?.name || corePlugin?.name || pluginId;
        dependent.push({ pluginId, pluginName, tables: overlappingTables });
      }
    }
    return dependent;
  }

  function showUninstallConfirmation(plugin: InstalledPluginInfo) {
    const permissions = registry.getPluginPermissions(plugin.id);
    const schemaName = permissions.schemaName || `plugin_${plugin.id.replace(/-/g, "_")}`;
    const dependentPlugins = findDependentPlugins(schemaName, plugin.id);
    uninstallConfirmation = { plugin, schemaName, dependentPlugins };
    deletePluginData = false;
  }

  function cancelUninstall() {
    uninstallConfirmation = null;
    deletePluginData = false;
  }

  async function confirmUninstall() {
    if (!uninstallConfirmation) return;
    const plugin = uninstallConfirmation.plugin;
    const schemaName = deletePluginData ? uninstallConfirmation.schemaName : null;
    uninstallingPluginId = plugin.id;
    uninstallConfirmation = null;

    try {
      if (schemaName) {
        const validSchemaPattern = /^plugin_[a-z0-9_]+$/;
        if (validSchemaPattern.test(schemaName)) {
          try {
            await executeQuery(`DROP SCHEMA IF EXISTS ${schemaName} CASCADE`, { readonly: false });
          } catch (e) {
            console.warn(`Failed to drop schema ${schemaName}:`, e);
          }
        }
      }
      await uninstallPlugin(plugin.id);
      const dataMsg = schemaName ? " and its data" : "";
      toast.success("Plugin uninstalled", `${plugin.name}${dataMsg} has been removed`);
      onPluginsChanged();
    } catch (e) {
      toast.error("Failed to uninstall plugin", e instanceof Error ? e.message : String(e));
    } finally {
      uninstallingPluginId = null;
    }
  }

  async function handleUninstallPlugin(plugin: InstalledPluginInfo) {
    showUninstallConfirmation(plugin);
  }

  // Plugin upgrade
  async function executeUpgradePlugin(update: PluginUpdateInfo) {
    upgradingPluginId = update.pluginId;
    try {
      // Create automatic backup before plugin update
      try {
        await createBackup(10);
      } catch (e) {
        console.warn("Pre-update backup failed:", e);
      }
      const resultStr = await invoke<string>("upgrade_plugin", { pluginId: update.pluginId });
      const result = JSON.parse(resultStr);
      if (result.success) {
        toast.success("Plugin updated", `${result.plugin_name} updated to ${result.version}`);
        onPluginsChanged();
      } else {
        throw new Error(result.error || "Unknown error");
      }
    } catch (e) {
      toast.error("Failed to update plugin", e instanceof Error ? e.message : String(e));
    } finally {
      upgradingPluginId = null;
    }
  }

  // Plugin detail modal
  function openPluginDetail(plugin: CommunityPluginInfo | InstalledPluginInfo, fromRegistry: boolean) {
    const installed = isPluginInstalled(plugin.id);
    const version = getInstalledVersion(plugin.id) || (plugin as InstalledPluginInfo).version;

    selectedPlugin = {
      id: plugin.id,
      name: plugin.name,
      description: plugin.description,
      author: plugin.author,
      repo: fromRegistry ? (plugin as CommunityPluginInfo).repo : "",
      version: version || undefined,
      installed,
    };

    if (!fromRegistry) {
      const registryPlugin = communityPlugins.find(p => p.id === plugin.id);
      if (registryPlugin) {
        selectedPlugin.repo = registryPlugin.repo;
      }
    }

    pluginReadme = null;
    if (selectedPlugin.repo) {
      fetchPluginReadme(selectedPlugin.repo);
    }
  }

  function closePluginDetail() {
    selectedPlugin = null;
    pluginReadme = null;
  }

  function resolveMarkdownPaths(markdown: string, owner: string, repoName: string): string {
    const rawBase = `https://raw.githubusercontent.com/${owner}/${repoName}/main`;
    const githubBase = `https://github.com/${owner}/${repoName}/blob/main`;

    let resolved = markdown.replace(
      /!\[([^\]]*)\]\((?!https?:\/\/)\.?\/?([^)]+)\)/g,
      `![$1](${rawBase}/$2)`
    );

    resolved = resolved.replace(
      /\[([^\]]+)\]\((?!https?:\/\/)(?!#)\.?\/?([^)]+)\)/g,
      `[$1](${githubBase}/$2)`
    );

    return resolved;
  }

  async function fetchPluginReadme(repoUrl: string) {
    isLoadingReadme = true;
    try {
      const match = repoUrl.match(/github\.com\/([^/]+)\/([^/]+)/);
      if (!match) {
        pluginReadme = null;
        return;
      }

      const [, owner, repoName] = match;
      const rawBase = `https://raw.githubusercontent.com/${owner}/${repoName}/main`;

      let response = await fetch(`${rawBase}/PLUGIN_PREVIEW.md`);
      if (!response.ok) {
        response = await fetch(`${rawBase}/README.md`);
      }

      if (!response.ok) {
        pluginReadme = null;
        return;
      }

      const rawMarkdown = await response.text();
      pluginReadme = resolveMarkdownPaths(rawMarkdown, owner, repoName);
    } catch (e) {
      console.error("Failed to fetch plugin preview:", e);
      pluginReadme = null;
    } finally {
      isLoadingReadme = false;
    }
  }
</script>

<section class="section">
  <h3 class="section-title">Plugins</h3>

  {#if pluginsNeedReload}
    <div class="reload-notice">
      <Icon name="info" size={14} />
      <span>Restart the app to apply plugin changes</span>
      <button class="btn primary small" onclick={onRestartApp}>Restart Now</button>
    </div>
  {/if}

  <div class="setting-group">
    <h4 class="group-title">Built-in Plugins</h4>
    <p class="group-desc">Enable or disable built-in functionality. Disabled plugins won't appear in the sidebar.</p>

    <div class="plugin-list">
      {#each plugins as plugin}
        <div class="plugin-item">
          <div class="plugin-info">
            <span class="plugin-name">{plugin.name}</span>
            <span class="plugin-desc">{plugin.description}</span>
          </div>
          <label class="toggle-switch">
            <input
              type="checkbox"
              checked={plugin.enabled}
              onchange={() => onTogglePlugin(plugin.id, !plugin.enabled)}
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
      {/each}
    </div>
  </div>

  <div class="setting-group">
    <h4 class="group-title">Browse Plugins</h4>
    <p class="group-desc">Discover and install plugins to extend Treeline.</p>

    {#if isLoadingCommunityPlugins}
      <div class="loading-placeholder">Loading plugins...</div>
    {:else if communityPlugins.length === 0 && installedCommunityPlugins.length === 0}
      <div class="empty-state">
        <p>No plugins available yet.</p>
      </div>
    {:else}
      <div class="plugin-list">
        {#each communityPlugins as plugin}
          {@const installed = isPluginInstalled(plugin.id)}
          {@const installedVersion = getInstalledVersion(plugin.id)}
          <div class="plugin-item community clickable" onclick={() => openPluginDetail(plugin, true)} role="button" tabindex="0" onkeydown={(e) => e.key === 'Enter' && openPluginDetail(plugin, true)}>
            <div class="plugin-info">
              <div class="plugin-header">
                <span class="plugin-name">{plugin.name}</span>
                {#if installed && installedVersion}
                  <span class="plugin-version">v{installedVersion}</span>
                {/if}
              </div>
              <span class="plugin-desc">{plugin.description}</span>
              <span class="plugin-author" class:official={plugin.author === "Treeline"}>by {plugin.author}</span>
            </div>
            <div class="plugin-actions" role="toolbar" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
              {#if installed}
                {@const update = pluginUpdates.get(plugin.id)}
                {#if update}
                  <button
                    class="btn primary small"
                    onclick={() => executeUpgradePlugin(update)}
                    disabled={upgradingPluginId === plugin.id}
                  >
                    {upgradingPluginId === plugin.id ? "Updating..." : `Update to ${update.latestVersion}`}
                  </button>
                {/if}
                <button
                  class="btn secondary small"
                  onclick={() => {
                    const p = installedCommunityPlugins.find(ip => ip.id === plugin.id);
                    if (p) handleUninstallPlugin(p);
                  }}
                  disabled={uninstallingPluginId === plugin.id}
                >
                  {uninstallingPluginId === plugin.id ? "Removing..." : "Remove"}
                </button>
              {:else}
                <button
                  class="btn primary small"
                  onclick={() => handleInstallPlugin(plugin)}
                  disabled={installingPluginId === plugin.id || loadingManifestPluginId === plugin.id}
                >
                  {#if installingPluginId === plugin.id}
                    Installing...
                  {:else if loadingManifestPluginId === plugin.id}
                    Loading...
                  {:else}
                    Install
                  {/if}
                </button>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</section>

<!-- Plugin Detail Modal -->
{#if selectedPlugin}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="sub-modal-overlay plugin-detail-overlay" onclick={closePluginDetail} onkeydown={(e) => e.key === 'Escape' && closePluginDetail()} role="dialog" aria-modal="true" tabindex="-1">
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="sub-modal plugin-detail-modal" onclick={(e) => e.stopPropagation()} role="document">
      <div class="sub-modal-header">
        <span class="sub-modal-title">{selectedPlugin.name}</span>
        <button class="close-btn" onclick={closePluginDetail}>
          <Icon name="x" size={16} />
        </button>
      </div>
      <div class="sub-modal-body plugin-detail-body">
        <div class="plugin-detail-meta">
          {#if selectedPlugin.version}
            <span class="plugin-detail-version">v{selectedPlugin.version}</span>
          {/if}
          <span class="plugin-detail-author">by {selectedPlugin.author}</span>
          {#if selectedPlugin.installed}
            <span class="plugin-detail-badge installed">Installed</span>
          {/if}
        </div>
        <p class="plugin-detail-desc">{selectedPlugin.description}</p>

        {#if selectedPlugin.repo}
          <button class="link-btn repo-link" onclick={() => openUrl(selectedPlugin!.repo)}>
            <Icon name="external-link" size={14} />
            View on GitHub
          </button>
        {/if}

        {#if isLoadingReadme}
          <div class="readme-loading">
            <div class="loading-spinner"></div>
            <span>Loading...</span>
          </div>
        {:else if pluginReadme}
          <div class="plugin-readme">
            <div class="readme-content" data-theme={currentTheme}>{@html marked(pluginReadme)}</div>
          </div>
        {/if}
      </div>
      <div class="sub-modal-actions">
        {#if selectedPlugin.installed}
          <button
            class="btn secondary"
            onclick={() => {
              const p = installedCommunityPlugins.find(ip => ip.id === selectedPlugin!.id);
              if (p) {
                handleUninstallPlugin(p);
                closePluginDetail();
              }
            }}
            disabled={uninstallingPluginId === selectedPlugin.id}
          >
            {uninstallingPluginId === selectedPlugin.id ? "Removing..." : "Remove Plugin"}
          </button>
        {:else}
          <button
            class="btn primary"
            onclick={() => {
              const p = communityPlugins.find(cp => cp.id === selectedPlugin!.id);
              if (p) {
                closePluginDetail();
                handleInstallPlugin(p);
              }
            }}
            disabled={installingPluginId === selectedPlugin.id || loadingManifestPluginId === selectedPlugin.id}
          >
            {#if installingPluginId === selectedPlugin.id}
              Installing...
            {:else if loadingManifestPluginId === selectedPlugin.id}
              Loading...
            {:else}
              Install Plugin
            {/if}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<!-- Uninstall Confirmation Modal -->
{#if uninstallConfirmation}
  <div class="sub-modal-overlay" onclick={cancelUninstall} role="presentation">
    <div class="sub-modal uninstall-confirm-modal" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === 'Escape' && cancelUninstall()} role="dialog" tabindex="-1">
      <div class="sub-modal-header">
        <h3>Remove {uninstallConfirmation.plugin.name}?</h3>
        <button class="close-btn" onclick={cancelUninstall}>✕</button>
      </div>
      <div class="sub-modal-content">
        {#if uninstallConfirmation.schemaName}
          <div class="uninstall-option">
            <label class="checkbox-label">
              <input type="checkbox" bind:checked={deletePluginData} />
              <span>Also delete plugin data</span>
            </label>
            <div class="tables-list">
              <span class="tables-label">Schema:</span>
              <code class="table-name">{uninstallConfirmation.schemaName}</code>
            </div>
          </div>

          {#if deletePluginData && uninstallConfirmation.dependentPlugins.length > 0}
            <div class="dependency-warning">
              <div class="warning-header">
                <Icon name="alert-triangle" size={16} />
                <span>Warning: Other plugins depend on this data</span>
              </div>
              <ul class="dependent-list">
                {#each uninstallConfirmation.dependentPlugins as dep}
                  <li>
                    <strong>{dep.pluginName}</strong> reads: {dep.tables.join(", ")}
                  </li>
                {/each}
              </ul>
              <p class="warning-note">Deleting this data may break these plugins.</p>
            </div>
          {/if}
        {:else}
          <p class="no-data-note">This plugin has no data to delete.</p>
        {/if}
      </div>
      <div class="sub-modal-actions">
        <button class="btn secondary" onclick={cancelUninstall}>Cancel</button>
        <button
          class="btn danger"
          onclick={confirmUninstall}
          disabled={uninstallingPluginId === uninstallConfirmation.plugin.id}
        >
          {uninstallingPluginId === uninstallConfirmation.plugin.id ? "Removing..." : "Remove"}
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Install Confirmation Modal -->
{#if installConfirmation}
  <div class="sub-modal-overlay" onclick={cancelInstall} role="presentation">
    <div class="sub-modal install-confirm-modal" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === 'Escape' && cancelInstall()} role="dialog" tabindex="-1">
      <div class="sub-modal-header">
        <h3>Install {installConfirmation.plugin.name}?</h3>
        <button class="close-btn" onclick={cancelInstall}>✕</button>
      </div>
      <div class="sub-modal-content">
        <p class="install-desc">{installConfirmation.plugin.description}</p>
        <p class="install-author">by {installConfirmation.plugin.author}</p>

        <div class="permissions-section">
          <h4 class="permissions-title">Permissions</h4>

          {#if installConfirmation.permissions.read?.length}
            <div class="permission-group">
              <span class="permission-label">Can read:</span>
              <div class="permission-tables">
                {#each installConfirmation.permissions.read as table}
                  <code class="table-name">{table === "*" ? "all tables" : table}</code>
                {/each}
              </div>
            </div>
          {/if}

          {#if installConfirmation.permissions.write?.length}
            <div class="permission-group">
              <span class="permission-label">Can write:</span>
              <div class="permission-tables">
                {#each installConfirmation.permissions.write as table}
                  <code class="table-name">{table}</code>
                {/each}
              </div>
            </div>
          {/if}

          {#if !installConfirmation.permissions.read?.length && !installConfirmation.permissions.write?.length}
            <p class="no-permissions">No special permissions required.</p>
          {/if}
        </div>
      </div>
      <div class="sub-modal-actions">
        <button class="btn secondary" onclick={cancelInstall}>Cancel</button>
        <button
          class="btn primary"
          onclick={() => executeInstallPlugin(installConfirmation!.plugin)}
          disabled={installingPluginId === installConfirmation.plugin.id}
        >
          {installingPluginId === installConfirmation.plugin.id ? "Installing..." : "Install"}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Reload notice */
  .reload-notice {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    background: rgba(239, 180, 68, 0.15);
    border: 1px solid rgba(239, 180, 68, 0.3);
    border-radius: 6px;
    margin-bottom: var(--spacing-md);
    font-size: 12px;
    color: #efb444;
  }

  /* Plugin list */
  .plugin-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .plugin-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    gap: var(--spacing-md);
  }

  .plugin-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .plugin-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .plugin-desc {
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.3;
  }

  /* Community plugins */
  .loading-placeholder,
  .empty-state {
    color: var(--text-muted);
    font-size: 13px;
    padding: var(--spacing-md);
    text-align: center;
  }

  .empty-state p {
    margin: 0;
  }

  .plugin-item.community {
    flex-direction: column;
    align-items: stretch;
    gap: var(--spacing-sm);
  }

  .plugin-item.community .plugin-info {
    gap: 4px;
  }

  .plugin-header {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
  }

  .plugin-version {
    font-size: 11px;
    color: var(--text-muted);
    background: var(--bg-tertiary);
    padding: 2px 6px;
    border-radius: 4px;
  }

  .plugin-author {
    font-size: 11px;
    color: var(--text-muted);
  }

  .plugin-author.official {
    color: var(--accent-primary);
    font-weight: 500;
  }

  .plugin-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-sm);
  }

  .plugin-item.clickable {
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
  }

  .plugin-item.clickable:hover {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
  }

  .plugin-item.clickable:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  /* Plugin detail modal */
  .plugin-detail-overlay {
    z-index: 1100;
  }

  .plugin-detail-modal {
    max-width: 500px;
    width: 90%;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
  }

  .plugin-detail-body {
    padding: var(--spacing-lg);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
  }

  .plugin-detail-meta {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    flex-wrap: wrap;
  }

  .plugin-detail-version {
    font-size: 12px;
    color: var(--text-muted);
    background: var(--bg-tertiary);
    padding: 2px 8px;
    border-radius: 4px;
  }

  .plugin-detail-author {
    font-size: 12px;
    color: var(--text-muted);
  }

  .plugin-detail-badge {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    padding: 2px 8px;
    border-radius: 4px;
  }

  .plugin-detail-badge.installed {
    background: var(--accent-success);
    color: white;
  }

  .plugin-detail-desc {
    font-size: 14px;
    color: var(--text-primary);
    line-height: 1.5;
    margin: 0;
  }

  .repo-link {
    display: inline-flex;
    align-items: center;
    gap: var(--spacing-xs);
    font-size: 13px;
    color: var(--accent-primary);
  }

  .repo-link:hover {
    text-decoration: underline;
  }

  .readme-loading {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    color: var(--text-muted);
    font-size: 13px;
  }

  .loading-spinner {
    width: 16px;
    height: 16px;
    border: 2px solid var(--border-primary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .plugin-readme {
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    padding: var(--spacing-md);
  }

  .readme-content {
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-secondary);
    max-height: 300px;
    overflow-y: auto;
  }

  .readme-content :global(h1),
  .readme-content :global(h2),
  .readme-content :global(h3),
  .readme-content :global(h4) {
    color: var(--text-primary);
    margin: var(--spacing-md) 0 var(--spacing-sm) 0;
    font-weight: 600;
  }

  .readme-content :global(h1) { font-size: 18px; }
  .readme-content :global(h2) { font-size: 16px; }
  .readme-content :global(h3) { font-size: 14px; }
  .readme-content :global(h4) { font-size: 13px; }

  .readme-content :global(h1:first-child),
  .readme-content :global(h2:first-child),
  .readme-content :global(h3:first-child) {
    margin-top: 0;
  }

  .readme-content :global(p) {
    margin: 0 0 var(--spacing-sm) 0;
  }

  .readme-content :global(ul),
  .readme-content :global(ol) {
    margin: 0 0 var(--spacing-sm) 0;
    padding-left: var(--spacing-lg);
  }

  .readme-content :global(li) {
    margin-bottom: 4px;
  }

  .readme-content :global(code) {
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--bg-primary);
    padding: 2px 6px;
    border-radius: 4px;
  }

  .readme-content :global(pre) {
    background: var(--bg-primary);
    padding: var(--spacing-sm);
    border-radius: 4px;
    overflow-x: auto;
    margin: 0 0 var(--spacing-sm) 0;
  }

  .readme-content :global(pre code) {
    background: none;
    padding: 0;
  }

  .readme-content :global(a) {
    color: var(--accent-primary);
  }

  .readme-content :global(a:hover) {
    text-decoration: underline;
  }

  .readme-content :global(img) {
    max-width: 100%;
    height: auto;
    border-radius: 6px;
    margin: var(--spacing-sm) 0;
  }

  /* Theme-aware images in plugin previews */
  .readme-content[data-theme="light"] :global(img[src*="-light"]) {
    display: block;
  }
  .readme-content[data-theme="light"] :global(img[src*="-dark"]) {
    display: none;
  }
  .readme-content[data-theme="dark"] :global(img[src*="-light"]) {
    display: none;
  }
  .readme-content[data-theme="dark"] :global(img[src*="-dark"]) {
    display: block;
  }

  /* Uninstall confirmation modal */
  .uninstall-confirm-modal {
    max-width: 450px;
  }

  .uninstall-option {
    margin-bottom: var(--spacing-md);
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    font-size: 13px;
    cursor: pointer;
    margin-bottom: var(--spacing-sm);
  }

  .checkbox-label input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-primary);
  }

  .tables-list {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--spacing-xs);
    padding-left: 24px;
  }

  .tables-label {
    font-size: 11px;
    color: var(--text-muted);
  }

  .table-name {
    font-family: var(--font-mono);
    font-size: 11px;
    background: var(--bg-tertiary);
    padding: 4px 8px;
    border-radius: 4px;
    color: var(--text-secondary);
  }

  .dependency-warning {
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 6px;
    padding: var(--spacing-md);
    margin-top: var(--spacing-md);
  }

  .warning-header {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    font-size: 13px;
    font-weight: 500;
    color: var(--accent-danger);
    margin-bottom: var(--spacing-sm);
  }

  .dependent-list {
    margin: 0 0 var(--spacing-sm) var(--spacing-lg);
    padding: 0;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .dependent-list li {
    margin-bottom: var(--spacing-xs);
  }

  .warning-note {
    font-size: 11px;
    color: var(--text-muted);
    margin: 0;
  }

  .no-data-note {
    font-size: 13px;
    color: var(--text-secondary);
    margin: 0;
  }

  /* Install confirmation modal */
  .install-confirm-modal {
    max-width: 480px;
  }

  .install-desc {
    font-size: 14px;
    color: var(--text-primary);
    margin: 0;
    line-height: 1.4;
  }

  .install-author {
    font-size: 12px;
    color: var(--text-muted);
    margin: var(--spacing-xs) 0 var(--spacing-lg) 0;
  }

  .permissions-section {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: var(--spacing-md) var(--spacing-lg);
  }

  .permissions-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin: 0 0 var(--spacing-md) 0;
  }

  .permission-group {
    margin-bottom: var(--spacing-md);
  }

  .permission-group:last-child {
    margin-bottom: 0;
  }

  .permission-label {
    display: block;
    font-size: 12px;
    color: var(--text-secondary);
    margin-bottom: var(--spacing-xs);
  }

  .permission-tables {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .no-permissions {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0;
  }
</style>
