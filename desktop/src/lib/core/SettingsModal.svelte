<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getVersion } from "@tauri-apps/api/app";
  import { Icon, SUPPORTED_CURRENCIES, DEFAULT_CURRENCY, setCurrency } from "../shared";
  import {
    getSettings,
    setAppSetting,
    runSync,
    executeQuery,
    executeQueryWithParams,
    setupSimplefin,
    setupLunchflow,
    getIntegrationSettings,
    updateIntegrationAccountSetting,
    getDisabledPlugins,
    enablePlugin,
    disablePlugin,
    getDemoMode,
    disableDemo,
    getImportProfiles,
    deleteImportProfile,
    getAccountProfileMappings,
    removeAccountProfileMapping,
    registry,
    toast,
    themeManager,
    activityStore,
    pluginUpdatesStore,
    type Settings,
    type AppSettings,
    type ImportProfile,
  } from "../sdk";
  import { invoke } from "@tauri-apps/api/core";
  import { getCorePluginManifests } from "../plugins";
  import { restartApp } from "../sdk/updater";

  // Import section components
  import {
    GeneralSection,
    AppearanceSection,
    IntegrationsSection,
    PluginsSection,
    StorageSection,
    AdvancedSection,
    AboutSection,
  } from "./settings/sections";
  import SimplefinSetupModal from "./settings/SimplefinSetupModal.svelte";
  import LunchflowSetupModal from "./settings/LunchflowSetupModal.svelte";

  type Section = "general" | "appearance" | "integrations" | "plugins" | "storage" | "advanced" | "about";

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    initialSection?: Section;
  }

  let { isOpen, onClose, initialSection }: Props = $props();

  // Core state
  let settings = $state<Settings | null>(null);
  let isLoading = $state(true);
  let isSyncing = $state(false);
  let appVersion = $state<string>("...");

  // Active section
  let activeSection = $state<Section>(initialSection ?? "general");

  // Update active section when initialSection changes (e.g., deep link)
  $effect(() => {
    if (initialSection) {
      activeSection = initialSection;
    }
  });

  const sections: { id: Section; label: string; icon: string }[] = [
    { id: "general", label: "General", icon: "settings" },
    { id: "appearance", label: "Appearance", icon: "palette" },
    { id: "integrations", label: "Integrations", icon: "link" },
    { id: "plugins", label: "Plugins", icon: "zap" },
    { id: "storage", label: "Storage", icon: "database" },
    { id: "advanced", label: "Advanced", icon: "command" },
    { id: "about", label: "About", icon: "info" },
  ];

  // Demo mode state
  let isDemoMode = $state(false);
  let isExitingDemo = $state(false);

  // Currency state
  let currentCurrency = $state<string>(DEFAULT_CURRENCY);

  // Import profiles state
  interface ImportProfileWithMappings {
    name: string;
    profile: ImportProfile;
    accountNames: string[];
  }
  let importProfiles = $state<ImportProfileWithMappings[]>([]);
  let isLoadingProfiles = $state(false);
  let deletingProfileName = $state<string | null>(null);

  // Integration state
  interface Integration {
    integration_name: string;
    created_at: string;
    updated_at: string;
  }
  let integrations = $state<Integration[]>([]);
  let isLoadingIntegrations = $state(false);

  // SimpleFIN accounts and status
  interface SimplefinAccount {
    account_id: string;
    simplefin_id: string;
    name: string;
    institution_name: string;
    account_type: string | null;
    balances_only: boolean;
  }
  let simplefinAccounts = $state<SimplefinAccount[]>([]);
  let connectionWarnings = $state<string[]>([]);
  let isCheckingConnection = $state(false);
  let connectionCheckSuccess = $state<boolean | null>(null);
  let simplefinSettings = $state<Record<string, unknown>>({});

  // SimpleFIN setup modal state
  let showSetupModal = $state(false);
  let setupToken = $state("");
  let isSettingUp = $state(false);
  let setupError = $state<string | null>(null);
  let setupSuccess = $state(false);
  let isFetchingAccounts = $state(false);

  interface SetupAccount {
    simplefin_id: string;
    name: string;
    institution_name: string | null;
    balance: string | null;
    balances_only: boolean;
  }
  let setupAccounts = $state<SetupAccount[]>([]);

  // Lunchflow state
  interface LunchflowAccount {
    account_id: string;
    lunchflow_id: string;
    name: string;
    institution_name: string;
    account_type: string | null;
    currency: string | null;
    balances_only: boolean;
  }
  let lunchflowAccounts = $state<LunchflowAccount[]>([]);

  // Lunchflow setup modal state
  let showLunchflowSetupModal = $state(false);
  let lunchflowApiKey = $state("");
  let isSettingUpLunchflow = $state(false);
  let lunchflowSetupError = $state<string | null>(null);
  let lunchflowSetupSuccess = $state(false);
  let isFetchingLunchflowAccounts = $state(false);

  interface LunchflowSetupAccount {
    lunchflow_id: string;
    name: string;
    institution_name: string | null;
    balance: string | null;
    currency: string | null;
    balances_only: boolean;
  }
  let lunchflowSetupAccounts = $state<LunchflowSetupAccount[]>([]);

  // Plugin state
  interface PluginInfo {
    id: string;
    name: string;
    description: string;
    enabled: boolean;
  }
  let plugins = $state<PluginInfo[]>([]);
  let pluginsNeedReload = $state(false);

  // Community plugins state
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
  let communityPlugins = $state<CommunityPluginInfo[]>([]);
  let installedCommunityPlugins = $state<InstalledPluginInfo[]>([]);
  let isLoadingCommunityPlugins = $state(false);

  // Plugin updates
  interface PluginUpdateInfo {
    pluginId: string;
    installedVersion: string;
    latestVersion: string;
    source: string;
  }
  let pluginUpdates = $state<Map<string, PluginUpdateInfo>>(new Map());

  // --- Loaders ---

  async function loadSettings() {
    isLoading = true;
    try {
      settings = await getSettings();
      isDemoMode = await getDemoMode();
      currentCurrency = settings?.app?.currency || DEFAULT_CURRENCY;
      await loadImportProfiles();
    } catch (e) {
      console.error("Failed to load settings:", e);
    } finally {
      isLoading = false;
    }
  }

  async function loadImportProfiles() {
    isLoadingProfiles = true;
    try {
      const profiles = await getImportProfiles();
      const accountMappings = await getAccountProfileMappings();

      const accountIds = Object.keys(accountMappings);
      let accountNameMap: Record<string, string> = {};

      if (accountIds.length > 0) {
        const placeholders = accountIds.map(() => '?').join(', ');
        const res = await executeQueryWithParams(
          `SELECT account_id, name FROM sys_accounts WHERE account_id IN (${placeholders})`,
          accountIds
        );
        for (const row of res.rows) {
          accountNameMap[row[0] as string] = row[1] as string;
        }
      }

      importProfiles = Object.entries(profiles).map(([name, profile]) => {
        const accountNames: string[] = [];
        for (const [accountId, profileName] of Object.entries(accountMappings)) {
          if (profileName === name && accountNameMap[accountId]) {
            accountNames.push(accountNameMap[accountId]);
          }
        }
        return { name, profile, accountNames };
      });
    } catch (e) {
      console.error("Failed to load import profiles:", e);
      importProfiles = [];
    } finally {
      isLoadingProfiles = false;
    }
  }

  async function loadIntegrations() {
    isLoadingIntegrations = true;
    try {
      const result = await executeQuery(
        "SELECT integration_name, created_at, updated_at FROM sys_integrations"
      );
      integrations = result.rows.map((row) => ({
        integration_name: row[0] as string,
        created_at: row[1] as string,
        updated_at: row[2] as string,
      }));
    } catch (e) {
      console.error("Failed to load integrations:", e);
      integrations = [];
    } finally {
      isLoadingIntegrations = false;
    }
  }

  async function loadSimplefinAccounts() {
    try {
      simplefinSettings = await getIntegrationSettings("simplefin");
      const accountSettings = (simplefinSettings.accountSettings || {}) as Record<string, { balancesOnly?: boolean }>;

      const result = await executeQuery(
        `SELECT account_id, name, institution_name, account_type, sf_id as simplefin_id
         FROM sys_accounts
         WHERE sf_id IS NOT NULL
         ORDER BY institution_name, name`
      );
      simplefinAccounts = result.rows.map((row) => {
        const simplefinId = row[4] as string;
        return {
          account_id: row[0] as string,
          simplefin_id: simplefinId,
          name: row[1] as string,
          institution_name: row[2] as string,
          account_type: row[3] as string | null,
          balances_only: accountSettings[simplefinId]?.balancesOnly || false,
        };
      });
    } catch (e) {
      console.error("Failed to load SimpleFIN accounts:", e);
      simplefinAccounts = [];
    }
  }

  async function loadLunchflowAccounts() {
    try {
      const lunchflowSettings = await getIntegrationSettings("lunchflow");
      const accountSettings = (lunchflowSettings.accountSettings || {}) as Record<string, { balancesOnly?: boolean }>;

      const result = await executeQuery(
        `SELECT account_id, name, institution_name, account_type, currency, lf_id as lunchflow_id
         FROM sys_accounts
         WHERE lf_id IS NOT NULL
         ORDER BY institution_name, name`
      );
      lunchflowAccounts = result.rows.map((row) => {
        const lunchflowId = row[5] as string;
        return {
          account_id: row[0] as string,
          lunchflow_id: lunchflowId,
          name: row[1] as string,
          institution_name: row[2] as string,
          account_type: row[3] as string | null,
          currency: row[4] as string | null,
          balances_only: accountSettings[lunchflowId]?.balancesOnly || false,
        };
      });
    } catch (e) {
      console.error("Failed to load Lunchflow accounts:", e);
      lunchflowAccounts = [];
    }
  }

  async function loadPlugins() {
    try {
      const manifests = getCorePluginManifests();
      const disabled = await getDisabledPlugins();
      plugins = manifests
        .filter(m => m.id !== "settings")
        .map(m => ({
          id: m.id,
          name: m.name,
          description: m.description,
          enabled: !disabled.includes(m.id),
        }));
    } catch (e) {
      console.error("Failed to load plugins:", e);
    }
  }

  const PLUGINS_REGISTRY_URL = "https://raw.githubusercontent.com/treeline-money/treeline/main/plugins.json";

  async function loadCommunityPlugins() {
    isLoadingCommunityPlugins = true;
    try {
      const response = await fetch(PLUGINS_REGISTRY_URL);
      if (!response.ok) throw new Error("Failed to fetch plugins");
      const data = await response.json();
      communityPlugins = data.plugins || [];

      const installed = await invoke<Array<{ manifest: InstalledPluginInfo; path: string }>>("discover_plugins");
      installedCommunityPlugins = installed.map(p => ({
        id: p.manifest.id,
        name: p.manifest.name,
        version: p.manifest.version,
        description: p.manifest.description,
        author: p.manifest.author,
      }));

      checkPluginUpdates();
    } catch (e) {
      console.error("Failed to load community plugins:", e);
      communityPlugins = [];
      installedCommunityPlugins = [];
    } finally {
      isLoadingCommunityPlugins = false;
    }
  }

  async function checkPluginUpdates() {
    if (installedCommunityPlugins.length === 0) return;

    const updates = new Map<string, PluginUpdateInfo>();
    const checks = installedCommunityPlugins.map(async (plugin) => {
      try {
        const resultStr = await invoke<string>("check_plugin_update", { pluginId: plugin.id });
        const result = JSON.parse(resultStr);
        if (result.has_update) {
          updates.set(plugin.id, {
            pluginId: plugin.id,
            installedVersion: result.installed_version,
            latestVersion: result.latest_version,
            source: result.source,
          });
        }
      } catch (e) {
        console.error(`Failed to check update for ${plugin.id}:`, e);
      }
    });

    await Promise.all(checks);
    pluginUpdates = updates;
    pluginUpdatesStore.setUpdates(updates);
  }

  // --- Handlers ---

  async function handleSync(balancesOnly: boolean = false) {
    isSyncing = true;
    const stopActivity = activityStore.start("Syncing accounts...");
    try {
      const result = await runSync({ balancesOnly });
      const totalAccounts = result.results.reduce((sum, r) => sum + (r.accounts_synced || 0), 0);
      const totalTransactions = result.results.reduce((sum, r) => sum + (r.transaction_stats?.new || r.transactions_synced || 0), 0);
      const errors = result.results.filter((r) => r.error);
      if (errors.length > 0) {
        toast.warning("Sync completed with warnings", errors.map((e) => e.error).join(", "));
      } else {
        toast.success("Sync complete", `${totalAccounts} accounts, ${totalTransactions} new transactions`);
      }
      await loadSettings();
      // Refresh account lists so UI updates after sync
      await loadSimplefinAccounts();
      await loadLunchflowAccounts();
    } catch (e) {
      toast.error("Sync failed", e instanceof Error ? e.message : String(e));
    } finally {
      stopActivity();
      isSyncing = false;
    }
  }

  async function handleCurrencyChange(currency: string) {
    if (!settings) return;
    await setAppSetting("currency", currency);
    currentCurrency = currency;
    setCurrency(currency);
    if (settings.app) {
      settings.app.currency = currency;
    }
    registry.emit("data:refresh");
    toast.success("Currency updated", `Your currency is now ${SUPPORTED_CURRENCIES[currency]?.name || currency}`);
  }

  async function handleAutoSyncChange(enabled: boolean) {
    if (!settings) return;
    await setAppSetting("autoSyncOnStartup", enabled);
    settings.app.autoSyncOnStartup = enabled;
  }

  async function handleDeleteProfile(profileName: string) {
    deletingProfileName = profileName;
    try {
      const accountMappings = await getAccountProfileMappings();
      for (const [accountId, mappedProfile] of Object.entries(accountMappings)) {
        if (mappedProfile === profileName) {
          await removeAccountProfileMapping(accountId);
        }
      }
      await deleteImportProfile(profileName);
      await loadImportProfiles();
      toast.success(`Profile "${profileName}" deleted`);
    } catch (e) {
      console.error("Failed to delete profile:", e);
      toast.error("Failed to delete profile");
    } finally {
      deletingProfileName = null;
    }
  }

  async function handleThemeChange(theme: AppSettings["theme"]) {
    if (!settings) return;
    await setAppSetting("theme", theme);
    settings.app.theme = theme;
    if (theme === "system") {
      const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      themeManager.setTheme(prefersDark ? "dark" : "light");
    } else {
      themeManager.setTheme(theme);
    }
  }

  async function handleAutoUpdateChange(enabled: boolean) {
    if (!settings) return;
    await setAppSetting("autoUpdate", enabled);
    settings.app.autoUpdate = enabled;
  }

  async function handleDeveloperModeChange(enabled: boolean) {
    if (!settings) return;
    await setAppSetting("developerMode", enabled);
    settings.app.developerMode = enabled;
    // Explicitly set devtools state based on the setting
    try {
      await invoke("set_devtools", { open: enabled });
    } catch (e) {
      console.error("Failed to set devtools:", e);
    }
  }

  async function handleExitDemoMode() {
    isExitingDemo = true;
    try {
      await disableDemo();
      isDemoMode = false;
      toast.success("Demo mode disabled", "Restart to load your real data");
      // Close modal and notify Shell to show restart banner
      onClose();
      registry.emit("demo:exitPending");
    } catch (e) {
      toast.error("Failed to exit demo mode", e instanceof Error ? e.message : String(e));
    } finally {
      isExitingDemo = false;
    }
  }

  async function handleCheckConnection() {
    isCheckingConnection = true;
    connectionWarnings = [];
    connectionCheckSuccess = null;
    try {
      const result = await runSync({ dryRun: true });
      const simplefinResult = result.results.find((r) => r.integration === "simplefin");
      if (simplefinResult?.provider_warnings) {
        connectionWarnings = simplefinResult.provider_warnings;
      }
      if (simplefinResult?.error) {
        connectionWarnings = [simplefinResult.error];
      }
      connectionCheckSuccess = connectionWarnings.length === 0;
    } catch (e) {
      connectionWarnings = [e instanceof Error ? e.message : String(e)];
      connectionCheckSuccess = false;
    } finally {
      isCheckingConnection = false;
    }
  }

  async function handleToggleBalancesOnly(account: SimplefinAccount) {
    const newValue = !account.balances_only;
    try {
      await updateIntegrationAccountSetting("simplefin", account.simplefin_id, newValue);
      account.balances_only = newValue;
      simplefinAccounts = [...simplefinAccounts];
    } catch (e) {
      console.error("Failed to update balances only setting:", e);
      toast.error("Failed to update setting", e instanceof Error ? e.message : String(e));
    }
  }

  async function handleDisconnect(integrationName: string) {
    try {
      await executeQueryWithParams(
        `DELETE FROM sys_integrations WHERE integration_name = ?`,
        [integrationName],
        { readonly: false }
      );
      toast.success("Disconnected", `${integrationName} integration removed`);
      await loadIntegrations();
    } catch (e) {
      toast.error("Failed to disconnect", e instanceof Error ? e.message : String(e));
    }
  }

  async function togglePlugin(pluginId: string, enabled: boolean) {
    try {
      if (enabled) {
        await enablePlugin(pluginId);
      } else {
        await disablePlugin(pluginId);
      }
      plugins = plugins.map(p => p.id === pluginId ? { ...p, enabled } : p);
      pluginsNeedReload = true;
    } catch (e) {
      console.error("Failed to toggle plugin:", e);
      toast.error("Failed to update plugin", e instanceof Error ? e.message : String(e));
    }
  }

  async function openExternalUrl(url: string) {
    try {
      await openUrl(url);
    } catch (e) {
      console.error("Failed to open URL:", e);
      toast.error("Failed to open link", "Could not open external browser");
    }
  }

  function formatLastSync(dateStr: string | null): string {
    if (!dateStr) return "Never";
    const date = new Date(dateStr);
    return date.toLocaleDateString("en-US", {
      month: "long",
      day: "numeric",
      year: "numeric",
    });
  }

  // SimpleFIN setup handlers
  function openSetupModal() {
    setupToken = "";
    setupError = null;
    setupSuccess = false;
    isFetchingAccounts = false;
    setupAccounts = [];
    showSetupModal = true;
  }

  function closeSetupModal() {
    showSetupModal = false;
    setupToken = "";
    setupError = null;
    setupSuccess = false;
  }

  async function handleSetupSimplefin() {
    if (!setupToken.trim()) {
      setupError = "Please enter a setup token";
      return;
    }

    isSettingUp = true;
    setupError = null;

    try {
      await setupSimplefin(setupToken.trim());
      await loadIntegrations();

      isFetchingAccounts = true;
      setupSuccess = true;

      try {
        await runSync({ balancesOnly: true });
        const result = await executeQuery(
          `SELECT
             sf_id as simplefin_id,
             name,
             institution_name,
             balance
           FROM sys_accounts
           WHERE sf_id IS NOT NULL
           ORDER BY institution_name, name`
        );

        setupAccounts = result.rows.map((row) => ({
          simplefin_id: row[0] as string,
          name: row[1] as string,
          institution_name: row[2] as string | null,
          balance: row[3] as string | null,
          balances_only: false,
        }));
      } catch (e) {
        console.error("Failed to fetch accounts after setup:", e);
        setupAccounts = [];
      } finally {
        isFetchingAccounts = false;
      }
    } catch (e) {
      setupError = e instanceof Error ? e.message : String(e);
      setupSuccess = false;
    } finally {
      isSettingUp = false;
    }
  }

  function toggleSetupAccountBalancesOnly(simplefinId: string) {
    setupAccounts = setupAccounts.map(acc =>
      acc.simplefin_id === simplefinId
        ? { ...acc, balances_only: !acc.balances_only }
        : acc
    );
  }

  async function handleSyncAfterSetup() {
    for (const acc of setupAccounts) {
      if (acc.balances_only) {
        try {
          await updateIntegrationAccountSetting("simplefin", acc.simplefin_id, true);
        } catch (e) {
          console.error("Failed to save account setting:", e);
        }
      }
    }

    closeSetupModal();
    await handleSync(false);
  }

  // Lunchflow setup handlers
  function openLunchflowSetupModal() {
    lunchflowApiKey = "";
    lunchflowSetupError = null;
    lunchflowSetupSuccess = false;
    isFetchingLunchflowAccounts = false;
    lunchflowSetupAccounts = [];
    showLunchflowSetupModal = true;
  }

  function closeLunchflowSetupModal() {
    showLunchflowSetupModal = false;
    lunchflowApiKey = "";
    lunchflowSetupError = null;
    lunchflowSetupSuccess = false;
  }

  async function handleSetupLunchflow() {
    if (!lunchflowApiKey.trim()) {
      lunchflowSetupError = "Please enter an API key";
      return;
    }

    isSettingUpLunchflow = true;
    lunchflowSetupError = null;

    try {
      await setupLunchflow(lunchflowApiKey.trim());
      await loadIntegrations();

      isFetchingLunchflowAccounts = true;
      lunchflowSetupSuccess = true;

      try {
        await runSync({ balancesOnly: true });
        const result = await executeQuery(
          `SELECT
             lf_id as lunchflow_id,
             name,
             institution_name,
             balance,
             currency
           FROM sys_accounts
           WHERE lf_id IS NOT NULL
           ORDER BY institution_name, name`
        );

        lunchflowSetupAccounts = result.rows.map((row) => ({
          lunchflow_id: row[0] as string,
          name: row[1] as string,
          institution_name: row[2] as string | null,
          balance: row[3] as string | null,
          currency: row[4] as string | null,
          balances_only: false,
        }));
      } catch (e) {
        console.error("Failed to fetch accounts after setup:", e);
        lunchflowSetupAccounts = [];
      } finally {
        isFetchingLunchflowAccounts = false;
      }
    } catch (e) {
      lunchflowSetupError = e instanceof Error ? e.message : String(e);
      lunchflowSetupSuccess = false;
    } finally {
      isSettingUpLunchflow = false;
    }
  }

  function toggleLunchflowSetupAccountBalancesOnly(lunchflowId: string) {
    lunchflowSetupAccounts = lunchflowSetupAccounts.map(acc =>
      acc.lunchflow_id === lunchflowId
        ? { ...acc, balances_only: !acc.balances_only }
        : acc
    );
  }

  async function handleLunchflowSyncAfterSetup() {
    for (const acc of lunchflowSetupAccounts) {
      if (acc.balances_only) {
        try {
          await updateIntegrationAccountSetting("lunchflow", acc.lunchflow_id, true);
        } catch (e) {
          console.error("Failed to save account setting:", e);
        }
      }
    }

    closeLunchflowSetupModal();
    await handleSync(false);
  }

  async function handleToggleLunchflowBalancesOnly(account: LunchflowAccount) {
    const newValue = !account.balances_only;
    try {
      await updateIntegrationAccountSetting("lunchflow", account.lunchflow_id, newValue);
      account.balances_only = newValue;
      lunchflowAccounts = [...lunchflowAccounts];
    } catch (e) {
      console.error("Failed to update balances only setting:", e);
      toast.error("Failed to update setting", e instanceof Error ? e.message : String(e));
    }
  }

  function handleOverlayClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onClose();
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onClose();
    }
  }

  // Load data when modal opens
  $effect(() => {
    if (isOpen) {
      pluginsNeedReload = false;
      loadSettings();
      loadPlugins();
      loadCommunityPlugins();
      loadIntegrations().then(() => {
        if (integrations.some((i) => i.integration_name === "simplefin")) {
          loadSimplefinAccounts();
        }
        if (integrations.some((i) => i.integration_name === "lunchflow")) {
          loadLunchflowAccounts();
        }
      });
      getVersion().then((v) => (appVersion = v));
    }
  });
</script>

{#if isOpen}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="settings-overlay"
    onclick={handleOverlayClick}
    onkeydown={handleKeyDown}
    role="dialog"
    aria-modal="true"
    aria-labelledby="settings-title"
    tabindex="-1"
  >
    <div class="settings-modal">
      <!-- Header -->
      <div class="modal-header">
        <h2 id="settings-title" class="modal-title">Settings</h2>
        <button class="close-btn" onclick={onClose} aria-label="Close settings">
          <Icon name="x" size={18} />
        </button>
      </div>

      <div class="settings-content">
        <!-- Sidebar navigation -->
        <nav class="settings-nav">
          {#each sections as section}
            <button
              class="nav-item"
              class:active={activeSection === section.id}
              onclick={() => (activeSection = section.id)}
            >
              <Icon name={section.icon} size={16} />
              <span class="nav-label">{section.label}</span>
            </button>
          {/each}
        </nav>

        <!-- Main content area -->
        <main class="settings-main">
          {#if isLoading}
            <div class="loading">Loading settings...</div>
          {:else if settings}
            {#if activeSection === "general"}
              <GeneralSection
                {settings}
                {currentCurrency}
                {importProfiles}
                {isLoadingProfiles}
                {deletingProfileName}
                {isSyncing}
                onCurrencyChange={handleCurrencyChange}
                onAutoSyncChange={handleAutoSyncChange}
                onSync={() => handleSync()}
                onDeleteProfile={handleDeleteProfile}
                {formatLastSync}
              />
            {:else if activeSection === "appearance"}
              <AppearanceSection
                currentTheme={settings.app.theme}
                onThemeChange={handleThemeChange}
              />
            {:else if activeSection === "integrations"}
              <IntegrationsSection
                {settings}
                {isDemoMode}
                {isExitingDemo}
                {isSyncing}
                {integrations}
                {simplefinAccounts}
                {lunchflowAccounts}
                {connectionWarnings}
                {isCheckingConnection}
                {connectionCheckSuccess}
                onExitDemoMode={handleExitDemoMode}
                onCheckConnection={handleCheckConnection}
                onToggleBalancesOnly={handleToggleBalancesOnly}
                onToggleLunchflowBalancesOnly={handleToggleLunchflowBalancesOnly}
                onOpenSetupModal={openSetupModal}
                onOpenLunchflowSetupModal={openLunchflowSetupModal}
                onDisconnect={handleDisconnect}
                onOpenExternalUrl={openExternalUrl}
                {formatLastSync}
              />
            {:else if activeSection === "plugins"}
              <PluginsSection
                {plugins}
                {communityPlugins}
                {installedCommunityPlugins}
                {isLoadingCommunityPlugins}
                {pluginUpdates}
                {pluginsNeedReload}
                onTogglePlugin={togglePlugin}
                onPluginsChanged={() => {
                  pluginsNeedReload = true;
                  loadCommunityPlugins();
                }}
                onRestartApp={restartApp}
              />
            {:else if activeSection === "storage"}
              <StorageSection {isDemoMode} />
            {:else if activeSection === "advanced"}
              <AdvancedSection
                developerMode={settings.app.developerMode ?? false}
                onDeveloperModeChange={handleDeveloperModeChange}
              />
            {:else if activeSection === "about"}
              <AboutSection
                {appVersion}
                autoUpdate={settings.app.autoUpdate}
                onAutoUpdateChange={handleAutoUpdateChange}
                onOpenExternalUrl={openExternalUrl}
              />
            {/if}
          {/if}
        </main>
      </div>
    </div>
  </div>

  <!-- SimpleFIN Setup Modal -->
  <SimplefinSetupModal
    isOpen={showSetupModal}
    {setupToken}
    {isSettingUp}
    {setupError}
    {setupSuccess}
    {isFetchingAccounts}
    {setupAccounts}
    onClose={closeSetupModal}
    onSetupTokenChange={(token) => setupToken = token}
    onSetup={handleSetupSimplefin}
    onToggleAccountBalancesOnly={toggleSetupAccountBalancesOnly}
    onSyncAfterSetup={handleSyncAfterSetup}
    onOpenExternalUrl={openExternalUrl}
  />

  <!-- Lunchflow Setup Modal -->
  <LunchflowSetupModal
    isOpen={showLunchflowSetupModal}
    apiKey={lunchflowApiKey}
    isSettingUp={isSettingUpLunchflow}
    setupError={lunchflowSetupError}
    setupSuccess={lunchflowSetupSuccess}
    isFetchingAccounts={isFetchingLunchflowAccounts}
    setupAccounts={lunchflowSetupAccounts}
    onClose={closeLunchflowSetupModal}
    onApiKeyChange={(key) => lunchflowApiKey = key}
    onSetup={handleSetupLunchflow}
    onToggleAccountBalancesOnly={toggleLunchflowSetupAccountBalancesOnly}
    onSyncAfterSetup={handleLunchflowSyncAfterSetup}
    onOpenExternalUrl={openExternalUrl}
  />
{/if}

<style>
  .settings-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    backdrop-filter: blur(2px);
  }

  .settings-modal {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 12px;
    width: 90%;
    max-width: 720px;
    height: 80%;
    max-height: 600px;
    display: flex;
    flex-direction: column;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-md) var(--spacing-lg);
    border-bottom: 1px solid var(--border-primary);
    background: var(--bg-secondary);
  }

  .modal-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .close-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 4px;
  }

  .close-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .settings-content {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .settings-nav {
    width: 160px;
    flex-shrink: 0;
    border-right: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    padding: var(--spacing-sm);
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    transition: all 0.15s;
    margin-bottom: 2px;
  }

  .nav-item:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .nav-item.active {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-weight: 500;
  }

  .settings-main {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-lg);
  }

  .loading {
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
