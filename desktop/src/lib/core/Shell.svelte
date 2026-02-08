<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Sidebar from "./Sidebar.svelte";
  import TabBar from "./TabBar.svelte";
  import ContentArea from "./ContentArea.svelte";
  import StatusBar from "./StatusBar.svelte";
  import CommandPalette from "./CommandPalette.svelte";
  import SettingsModal from "./SettingsModal.svelte";
  import ToastContainer from "./ToastContainer.svelte";
  import UpdateBanner from "./UpdateBanner.svelte";
  import ImportModal from "./ImportModal.svelte";
  import PendingImportsModal from "./PendingImportsModal.svelte";
  import { Icon } from "../shared";
  import { registry, getDemoMode, enableDemo, disableDemo, runSync, toast, getAppSetting, activityStore, listPendingImports, pluginUpdatesStore, logger, type PendingImportFile } from "../sdk";
  import { initUpdater, restartApp, checkForUpdate } from "../sdk/updater";

  let commandPaletteOpen = $state(false);
  let settingsModalOpen = $state(false);
  let settingsInitialSection = $state<"general" | "appearance" | "integrations" | "plugins" | "storage" | "advanced" | "about" | undefined>(undefined);
  let importModalOpen = $state(false);
  let droppedFilePath = $state<string | null>(null);
  let isDraggingFile = $state(false);
  let isDemoMode = $state(false);
  let hideDemoBanner = $state(false);
  let isExitingDemo = $state(false);
  let demoExitPending = $state(false);  // Demo disabled, waiting for user to restart
  let isRestartingFromDemo = $state(false);

  // Pending imports (watch folder)
  let pendingImportFiles = $state<PendingImportFile[]>([]);
  let showPendingImportsModal = $state(false);
  let pendingImportsDismissed = $state(false); // Reset on app restart or after import

  // Check demo mode on mount and subscribe to refresh events
  onMount(() => {
    checkDemoMode();
    checkPendingImports();
    initUpdater();
    pluginUpdatesStore.checkOnStartup();
    const unsubscribe = registry.on("data:refresh", checkDemoMode);
    const unsubscribeDemoExit = registry.on("demo:exitPending", () => {
      isDemoMode = false;
      demoExitPending = true;
    });

    // Set up Tauri drag-drop listener
    let dragDropUnlisten: (() => void) | undefined;
    let focusUnlisten: (() => void) | undefined;

    const appWindow = getCurrentWindow();

    appWindow.onDragDropEvent((event) => {
      const eventType = event.payload.type;
      if (eventType === "over" || eventType === "enter") {
        isDraggingFile = true;
      } else if (eventType === "leave") {
        isDraggingFile = false;
      } else if (eventType === "drop") {
        isDraggingFile = false;
        const paths = event.payload.paths;
        if (paths && paths.length > 0) {
          const filePath = paths[0];
          if (filePath.toLowerCase().endsWith(".csv")) {
            droppedFilePath = filePath;
            importModalOpen = true;
            toast.info("CSV detected", filePath.split("/").pop() || "file");
          } else {
            toast.warning("Unsupported file", "Only CSV files can be imported");
          }
        }
      }
    }).then((unlisten) => {
      dragDropUnlisten = unlisten;
    });

    // Check for pending imports when window regains focus
    appWindow.onFocusChanged(({ payload: focused }) => {
      if (focused && !importModalOpen && !showPendingImportsModal && !pendingImportsDismissed) {
        checkPendingImports();
      }
    }).then((unlisten) => {
      focusUnlisten = unlisten;
    });

    return () => {
      unsubscribe();
      unsubscribeDemoExit();
      if (dragDropUnlisten) dragDropUnlisten();
      if (focusUnlisten) focusUnlisten();
    };
  });

  async function checkDemoMode() {
    isDemoMode = await getDemoMode();
    hideDemoBanner = (await getAppSetting("hideDemoBanner")) ?? false;
  }

  async function checkPendingImports() {
    try {
      const files = await listPendingImports();
      if (files.length > 0) {
        pendingImportFiles = files;
        showPendingImportsModal = true;
      }
    } catch (e) {
      console.error("Failed to check pending imports:", e);
    }
  }

  function handlePendingImportContinue(filePath: string) {
    showPendingImportsModal = false;
    droppedFilePath = filePath;
    importModalOpen = true;
  }

  async function handleExitDemo() {
    isExitingDemo = true;
    try {
      await disableDemo();
      isDemoMode = false;
      demoExitPending = true;
      toast.success("Demo mode disabled", "Restart to load your real data");
    } catch (e) {
      toast.error("Failed to exit demo mode", e instanceof Error ? e.message : String(e));
    } finally {
      isExitingDemo = false;
    }
  }

  async function handleRestartFromDemo() {
    isRestartingFromDemo = true;
    try {
      await restartApp();
    } catch (e) {
      toast.error("Failed to restart", e instanceof Error ? e.message : String(e));
      isRestartingFromDemo = false;
    }
  }

  // Register core commands
  $effect(() => {
    registry.registerCommand({
      id: "core:command-palette",
      name: "Open Command Palette",
      category: "Core",
      shortcut: "cmd+P",
      execute: () => {
        commandPaletteOpen = true;
      },
    });

    registry.registerCommand({
      id: "core:settings",
      name: "Open Settings",
      category: "Core",
      shortcut: "cmd+,",
      execute: () => {
        logger.action("settings_opened", "command");
        settingsInitialSection = undefined;
        settingsModalOpen = true;
      },
    });

    registry.registerCommand({
      id: "core:settings:integrations",
      name: "Open Integrations Settings",
      category: "Core",
      execute: () => {
        settingsInitialSection = "integrations";
        settingsModalOpen = true;
      },
    });

    registry.registerCommand({
      id: "core:settings:plugins",
      name: "Browse Plugins",
      category: "Core",
      execute: () => {
        settingsInitialSection = "plugins";
        settingsModalOpen = true;
      },
    });

    registry.registerCommand({
      id: "core:settings:storage",
      name: "Open Storage Settings",
      category: "Core",
      execute: () => {
        settingsInitialSection = "storage";
        settingsModalOpen = true;
      },
    });

    registry.registerCommand({
      id: "core:checkForUpdates",
      name: "Check for Updates",
      category: "Core",
      execute: async () => {
        try {
          toast.info("Checking for updates...");
          const update = await checkForUpdate(true); // force check
          if (update) {
            toast.success("Update available", `Version ${update.version} is ready to install`);
          } else {
            toast.info("No updates available", "You're running the latest version");
          }
        } catch (e) {
          toast.error("Update check failed", e instanceof Error ? e.message : String(e));
        }
      },
    });

    registry.registerCommand({
      id: "data:sync",
      name: "Sync All Integrations",
      category: "Data",
      execute: async () => {
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

          const errors = result.results.filter((r) => r.error);
          if (errors.length > 0) {
            toast.warning(
              "Sync completed with warnings",
              errors.map((e) => e.error).join(", ")
            );
          } else {
            toast.success(
              "Sync complete",
              `${totalAccounts} accounts, ${totalTransactions} new transactions`
            );
          }

          // Notify components to refresh their data (e.g., sidebar badge)
          registry.emit("data:refresh");
        } catch (e) {
          toast.error("Sync failed", e instanceof Error ? e.message : String(e));
        } finally {
          stopActivity();
        }
      },
    });

    registry.registerCommand({
      id: "data:import",
      name: "Import CSV",
      category: "Data",
      shortcut: "cmd+I",
      execute: () => {
        importModalOpen = true;
      },
    });

    registry.registerCommand({
      id: "data:pendingImports",
      name: "Review Pending Imports",
      category: "Data",
      execute: async () => {
        pendingImportsDismissed = false; // User explicitly asked, reset dismissal
        await checkPendingImports();
        if (pendingImportFiles.length === 0) {
          toast.info("No pending imports", "Drop CSV files in ~/.treeline/imports/");
        }
      },
    });

    registry.registerCommand({
      id: "data:toggleDemoMode",
      name: "Toggle Demo Mode",
      category: "Data",
      execute: async () => {
        const current = await getDemoMode();
        const newMode = !current;

        try {
          if (newMode) {
            toast.info("Enabling demo mode...", "Setting up demo data");
            await enableDemo();
            toast.success("Demo mode enabled", "Switched to demo data");
            registry.emit("data:refresh");
          } else {
            await disableDemo();
            isDemoMode = false;
            demoExitPending = true;
            toast.success("Demo mode disabled", "Restart to load your real data");
          }
        } catch (e) {
          toast.error("Demo mode toggle failed", e instanceof Error ? e.message : String(e));
        }
      },
    });
  });

  // Global keyboard shortcuts
  function handleKeydown(e: KeyboardEvent) {
    // Command palette: Cmd+P or Ctrl+P
    if ((e.metaKey || e.ctrlKey) && e.key === "p") {
      e.preventDefault();
      commandPaletteOpen = true;
    }

    // Settings: Cmd+, or Ctrl+,
    if ((e.metaKey || e.ctrlKey) && e.key === ",") {
      e.preventDefault();
      settingsModalOpen = true;
    }

    // Import: Cmd+I or Ctrl+I (without Shift)
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key === "i") {
      e.preventDefault();
      importModalOpen = true;
    }

    // DevTools: Cmd+Shift+I or Ctrl+Shift+I (toggle)
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key === "I") {
      e.preventDefault();
      // Pass null/undefined to toggle
      invoke("set_devtools", { open: null }).catch((err) => {
        console.error("Failed to toggle devtools:", err);
      });
    }

    // Close tab: Cmd+W
    if ((e.metaKey || e.ctrlKey) && e.key === "w") {
      e.preventDefault();
      const activeTab = registry.activeTab;
      if (activeTab) {
        registry.closeTab(activeTab.id);
      }
    }

    // Switch tabs: Cmd+1-9
    if ((e.metaKey || e.ctrlKey) && e.key >= "1" && e.key <= "9") {
      e.preventDefault();
      const index = parseInt(e.key) - 1;
      const tabs = registry.tabs;
      if (tabs[index]) {
        registry.setActiveTab(tabs[index].id);
      }
    }
  }

</script>

<svelte:window onkeydown={handleKeydown} />

<div class="shell">
  <Sidebar />

  <div class="main-area">
    <TabBar />
    <UpdateBanner />
    {#if demoExitPending}
      <div class="restart-banner">
        <span class="restart-icon"><Icon name="refresh" size={16} /></span>
        <span class="restart-text">
          <strong>Restart required</strong> — Restart to load your real data
        </span>
        <button class="restart-btn" onclick={handleRestartFromDemo} disabled={isRestartingFromDemo}>
          {isRestartingFromDemo ? "Restarting..." : "Restart Now"}
        </button>
      </div>
    {:else if isDemoMode && !hideDemoBanner}
      <div class="demo-banner">
        <span class="demo-icon"><Icon name="beaker" size={16} /></span>
        <span class="demo-text">
          <strong>Demo Mode</strong> — Exploring with sample data
        </span>
        <button class="demo-exit-btn" onclick={handleExitDemo} disabled={isExitingDemo}>
          <Icon name="log-out" size={12} />
          {isExitingDemo ? "Exiting..." : "Exit Demo Mode"}
        </button>
      </div>
    {/if}
    <ContentArea />
  </div>

  <StatusBar />

  <CommandPalette bind:isOpen={commandPaletteOpen} />
  <SettingsModal isOpen={settingsModalOpen} onClose={() => settingsModalOpen = false} initialSection={settingsInitialSection} />
  <ImportModal
    open={importModalOpen}
    onclose={() => {
      importModalOpen = false;
      droppedFilePath = null;
    }}
    onsuccess={async (batchId) => {
      importModalOpen = false;
      droppedFilePath = null;
      pendingImportsDismissed = false; // Re-enable checks after successful import
      toast.success("Import complete", `Batch ${batchId.slice(-8)}`);
      registry.emit("data:refresh");
      // Check if there are more files to import
      await checkPendingImports();
    }}
    initialFilePath={droppedFilePath}
  />
  <PendingImportsModal
    open={showPendingImportsModal}
    files={pendingImportFiles}
    onclose={() => {
      showPendingImportsModal = false;
      pendingImportsDismissed = true; // Don't prompt again this session
    }}
    oncontinue={handlePendingImportContinue}
  />
  <ToastContainer />

  {#if isDraggingFile}
    <div class="drop-overlay">
      <div class="drop-content">
        <Icon name="database" size={48} />
        <span class="drop-title">Drop CSV to Import</span>
        <span class="drop-hint">Release to open import dialog</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .shell {
    width: 100vw;
    height: 100vh;
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: 1fr auto;
    background: var(--bg-primary);
    color: var(--text-primary);
    font-family: var(--font-sans);
  }

  .main-area {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .demo-banner {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 1rem;
    background: linear-gradient(90deg, #b45309 0%, #d97706 100%);
    color: white;
    font-size: 0.875rem;
  }

  .demo-icon {
    display: flex;
    align-items: center;
  }

  .demo-text {
    flex: 1;
  }

  .demo-text strong {
    font-weight: 600;
  }

  .demo-exit-btn {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.75rem;
    background: rgba(255, 255, 255, 0.2);
    border: 1px solid rgba(255, 255, 255, 0.3);
    border-radius: 4px;
    color: white;
    font-size: 0.75rem;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .demo-exit-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.3);
  }

  .demo-exit-btn:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  .restart-banner {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 1rem;
    background: linear-gradient(90deg, #2563eb 0%, #3b82f6 100%);
    color: white;
    font-size: 0.875rem;
  }

  .restart-icon {
    display: flex;
    align-items: center;
  }

  .restart-text {
    flex: 1;
  }

  .restart-text strong {
    font-weight: 600;
  }

  .restart-btn {
    padding: 0.375rem 0.75rem;
    background: rgba(255, 255, 255, 0.95);
    border: none;
    border-radius: 4px;
    color: #2563eb;
    font-size: 0.75rem;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .restart-btn:hover:not(:disabled) {
    background: white;
  }

  .restart-btn:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  :global(footer.statusbar) {
    grid-column: 1 / -1;
  }

  .drop-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 9999;
    pointer-events: none;
  }

  .drop-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--spacing-md);
    padding: var(--spacing-xl);
    background: var(--bg-secondary);
    border: 2px dashed var(--accent-primary);
    border-radius: 12px;
    color: var(--accent-primary);
  }

  .drop-title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .drop-hint {
    font-size: 13px;
    color: var(--text-muted);
  }
</style>
