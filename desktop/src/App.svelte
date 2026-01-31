<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import Shell from "./lib/core/Shell.svelte";
  import WelcomeModal from "./lib/core/WelcomeModal.svelte";
  import UnlockModal from "./lib/core/UnlockModal.svelte";
  import WhatsNewModal from "./lib/core/WhatsNewModal.svelte";
  import { initializePlugins } from "./lib/plugins";
  import { themeManager, isSyncNeeded, runSync, toast, getAppSetting, setAppSetting, registry, activityStore, tryAutoUnlock, getEncryptionStatus } from "./lib/sdk";
  import { loadCurrency } from "./lib/shared";

  let isLoading = $state(true);
  let loadingStatus = $state("Initializing...");
  let showWelcome = $state(false);
  let showUnlock = $state(false);
  let showWhatsNew = $state(false);

  onMount(async () => {
    try {
      // Initialize theme system (loads themes from ~/.treeline/themes/)
      loadingStatus = "Loading theme...";
      await themeManager.init();

      // Check encryption status and try auto-unlock
      loadingStatus = "Checking encryption...";
      const autoUnlocked = await tryAutoUnlock();

      if (!autoUnlocked) {
        // Database is encrypted and needs manual unlock
        isLoading = false;
        showUnlock = true;
        return;
      }

      // Continue with normal initialization
      await continueInitialization();
    } catch (error) {
      console.error("Initialization error:", error);
      loadingStatus = `Error: ${error}`;
    }
  });

  async function continueInitialization() {
    try {
      // Run database migrations first (ensures schema is up-to-date)
      loadingStatus = "Checking database...";
      try {
        console.log("[Startup] Running migrations...");
        await invoke("run_migrations");
        console.log("[Startup] Migrations complete");
      } catch (e) {
        console.warn("[Startup] Migration check:", e);
        // Don't fail startup on migration issues - the database might not exist yet
      }

      // Check if first-time user
      loadingStatus = "Checking setup...";
      const hasCompletedOnboarding = await getAppSetting("hasCompletedOnboarding");

      // Load currency preference
      loadingStatus = "Loading preferences...";
      await loadCurrency();

      // Load all plugins
      loadingStatus = "Loading plugins...";
      await initializePlugins();

      isLoading = false;

      // Show welcome modal for first-time users
      if (!hasCompletedOnboarding) {
        showWelcome = true;
      } else {
        // Check if we should show "What's New" (version changed since last seen)
        await checkForWhatsNew();

        // Check if sync is needed (after UI is loaded so user sees the app)
        checkAndRunSync();
      }
    } catch (error) {
      console.error("Initialization error:", error);
      loadingStatus = `Error: ${error}`;
    }
  }

  async function handleUnlocked() {
    showUnlock = false;
    isLoading = true;
    loadingStatus = "Initializing...";
    await continueInitialization();
  }

  function handleWelcomeComplete(action?: "settings" | "import" | "manual") {
    showWelcome = false;
    // Use a small delay to ensure Shell is rendered before triggering commands
    setTimeout(() => {
      if (action === "settings") {
        registry.executeCommand("core:settings:integrations");
      } else if (action === "import") {
        registry.executeCommand("data:import");
      } else if (action === "manual") {
        // Open Accounts view for manual entry starting point
        registry.openView("accounts");
      } else {
        // Demo mode was enabled, data was synced by enableDemo()
        registry.emit("data:refresh");
      }
    }, 100);
  }

  async function checkForWhatsNew() {
    try {
      const currentVersion = await getVersion();
      const lastSeenVersion = await getAppSetting("lastSeenVersion");

      // Show "What's New" if:
      // 1. User has seen a previous version (not first launch)
      // 2. Current version is different from last seen
      if (lastSeenVersion && lastSeenVersion !== currentVersion) {
        showWhatsNew = true;
      } else if (!lastSeenVersion) {
        // First time seeing version tracking - record current version without showing modal
        await setAppSetting("lastSeenVersion", currentVersion);
      }
    } catch (e) {
      console.error("Failed to check for what's new:", e);
    }
  }

  function handleWhatsNewClose() {
    showWhatsNew = false;
  }

  async function checkAndRunSync() {
    try {
      const needsSync = await isSyncNeeded();
      if (needsSync) {
        const stopActivity = activityStore.start("Syncing accounts...");

        try {
          const result = await runSync();
          const totalAccounts = result.results.reduce(
            (sum, r) => sum + (r.accounts_synced || 0),
            0
          );
          const totalTransactions = result.results.reduce(
            (sum, r) => sum + (r.transaction_stats?.new || r.transactions_synced || 0),
            0
          );

          // Check for errors
          const errors = result.results.filter((r) => r.error);
          if (errors.length > 0) {
            toast.warning(
              "Sync completed with warnings",
              errors.map((e) => e.error).join(", ")
            );
          } else if (totalTransactions > 0 || totalAccounts > 0) {
            toast.success(
              "Sync complete",
              `${totalAccounts} accounts, ${totalTransactions} new transactions`
            );
          }
          // Don't show toast if nothing synced (no integrations configured)
        } catch (e) {
          // Don't show error toast on startup for missing integrations
          console.log("Startup sync skipped:", e);
        } finally {
          stopActivity();
        }
      }
    } catch (e) {
      console.error("Failed to check sync status:", e);
    }
  }
</script>

{#if isLoading}
  <div class="loading-screen">
    <div class="loading-content">
      <svg class="loading-logo" viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
        <path d="M32 12 L20 35 L35 40 L44 35 Z" fill="var(--logo-snow)"/>
        <path d="M20 35 L35 40 L44 35 L54 52 L10 52 Z" fill="var(--accent-primary)"/>
        <path d="M32 12 L54 52 L10 52 Z" stroke="var(--accent-primary)" stroke-width="2.5" fill="none"/>
      </svg>
      <span class="loading-text">treeline</span>
      <span class="loading-status">{loadingStatus}</span>
    </div>
  </div>
{:else if showUnlock}
  <!-- Show unlock modal over a blank background -->
  <div class="locked-screen">
    <UnlockModal open={true} onunlock={handleUnlocked} />
  </div>
{:else}
  <Shell />
  {#if showWelcome}
    <WelcomeModal onComplete={handleWelcomeComplete} />
  {/if}
  {#if showWhatsNew}
    <WhatsNewModal onclose={handleWhatsNewClose} />
  {/if}
{/if}

<style>
  .locked-screen {
    width: 100vw;
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-primary);
  }

  .loading-screen {
    width: 100vw;
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-primary);
  }

  .loading-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--spacing-md);
  }

  .loading-logo {
    width: 48px;
    height: 48px;
    animation: pulse 2s ease-in-out infinite;
  }

  .loading-text {
    font-family: 'Outfit', var(--font-sans);
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: -0.5px;
  }

  .loading-status {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-muted);
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }
</style>
