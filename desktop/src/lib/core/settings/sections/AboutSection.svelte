<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Icon } from "../../../shared";
  import { toast, type AppSettings } from "../../../sdk";
  import { checkForUpdate, downloadAndInstall, restartApp, subscribeToUpdates, type UpdateState } from "../../../sdk/updater";
  import "../settings-shared.css";

  interface Props {
    appVersion: string;
    autoUpdate: boolean;
    onAutoUpdateChange: (enabled: boolean) => void;
    onOpenExternalUrl: (url: string) => void;
  }

  let {
    appVersion,
    autoUpdate,
    onAutoUpdateChange,
    onOpenExternalUrl,
  }: Props = $props();

  // Treeline directory path
  let treelineDir = $state("~/.treeline");

  onMount(async () => {
    try {
      treelineDir = await invoke<string>("get_treeline_dir_display");
    } catch (e) {
      console.error("Failed to get treeline dir:", e);
    }
  });

  // Update check state
  let isCheckingForUpdate = $state(false);
  let lastUpdateCheckResult = $state<string | null>(null);
  let updateState = $state<UpdateState>({
    available: false,
    version: null,
    notes: null,
    isDownloading: false,
    downloadProgress: 0,
    error: null,
  });
  let isInstallingUpdate = $state(false);
  let isRestartingApp = $state(false);

  // Subscribe to update state
  $effect(() => {
    const unsubscribe = subscribeToUpdates((state) => {
      updateState = state;
    });
    return () => unsubscribe();
  });

  let isUpdateReadyToInstall = $derived(updateState.downloadProgress === 100 && !updateState.isDownloading);

  async function handleCheckForUpdate() {
    isCheckingForUpdate = true;
    lastUpdateCheckResult = null;
    try {
      const update = await checkForUpdate(true);
      if (update) {
        lastUpdateCheckResult = `Update available: v${update.version}`;
      } else {
        lastUpdateCheckResult = "You're up to date!";
        toast.info("No updates", "You're running the latest version");
      }
    } catch (e) {
      lastUpdateCheckResult = "Failed to check for updates";
      toast.error("Update check failed", e instanceof Error ? e.message : String(e));
    } finally {
      isCheckingForUpdate = false;
    }
  }

  async function handleInstallUpdate() {
    isInstallingUpdate = true;
    try {
      await downloadAndInstall();
    } catch (e) {
      toast.error("Update failed", e instanceof Error ? e.message : String(e));
    } finally {
      isInstallingUpdate = false;
    }
  }

  async function handleRestartApp() {
    isRestartingApp = true;
    try {
      await restartApp();
    } catch (e) {
      toast.error("Restart failed", e instanceof Error ? e.message : String(e));
      isRestartingApp = false;
    }
  }
</script>

<section class="section">
  <h3 class="section-title">About</h3>

  <div class="about-content">
    <div class="about-logo">
      <svg viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
        <path d="M32 12 L20 35 L35 40 L44 35 Z" fill="var(--logo-snow)"/>
        <path d="M20 35 L35 40 L44 35 L54 52 L10 52 Z" fill="var(--accent-primary)"/>
        <path d="M32 12 L54 52 L10 52 Z" stroke="var(--accent-primary)" stroke-width="2.5" fill="none"/>
      </svg>
    </div>
    <div class="about-name">treeline</div>
    <div class="about-tagline">The Obsidian of personal finance</div>

    <div class="about-info">
      <div class="info-row">
        <span class="info-label">Version:</span>
        <span class="info-value">{appVersion}</span>
      </div>
    </div>

    <div class="about-paths">
      <div class="path-row">
        <span class="path-label">Database:</span>
        <span class="path-value">{treelineDir}/treeline.duckdb</span>
      </div>
      <div class="path-row">
        <span class="path-label">Plugins:</span>
        <span class="path-value">{treelineDir}/plugins/</span>
      </div>
    </div>

    <div class="about-links">
      <button
        class="link-btn"
        onclick={() => onOpenExternalUrl("https://treeline.money")}
      >
        Website
      </button>
      <span class="link-separator">·</span>
      <button
        class="link-btn"
        onclick={() => onOpenExternalUrl("https://discord.gg/EcNvBnSft5")}
      >
        Discord
      </button>
      <span class="link-separator">·</span>
      <button
        class="link-btn"
        onclick={() => onOpenExternalUrl("https://github.com/treeline-money/treeline")}
      >
        GitHub
      </button>
    </div>
  </div>

  <div class="setting-group">
    <h4 class="group-title">Updates</h4>

    <label class="checkbox-setting">
      <input
        type="checkbox"
        checked={autoUpdate}
        onchange={(e) => onAutoUpdateChange(e.currentTarget.checked)}
      />
      <span>Automatically check for updates</span>
    </label>
    <p class="group-desc">When enabled, Treeline will check for updates on startup and every 24 hours. You'll be notified when an update is available.</p>

    {#if updateState.available || updateState.isDownloading || isUpdateReadyToInstall}
      <!-- Update available - show inline update UI -->
      <div class="update-card">
        {#if isUpdateReadyToInstall}
          <div class="update-card-content">
            <Icon name="check-circle" size={20} class="update-icon success" />
            <div class="update-info">
              <strong>Update ready!</strong>
              <span>Treeline v{updateState.version} has been downloaded. Restart to apply.</span>
            </div>
          </div>
          <button
            class="btn primary"
            onclick={handleRestartApp}
            disabled={isRestartingApp}
          >
            {isRestartingApp ? "Restarting..." : "Restart Now"}
          </button>
        {:else if updateState.isDownloading}
          <div class="update-card-content">
            <Icon name="download" size={20} class="update-icon" />
            <div class="update-info">
              <strong>Downloading update...</strong>
              <span>v{updateState.version} — {updateState.downloadProgress}%</span>
            </div>
          </div>
          <div class="update-progress">
            <div class="update-progress-fill" style="width: {updateState.downloadProgress}%"></div>
          </div>
        {:else}
          <div class="update-card-content">
            <Icon name="arrow-up-circle" size={20} class="update-icon" />
            <div class="update-info">
              <strong>Update available</strong>
              <span>Treeline v{updateState.version} is ready to download</span>
            </div>
          </div>
          <button
            class="btn primary"
            onclick={handleInstallUpdate}
            disabled={isInstallingUpdate}
          >
            {isInstallingUpdate ? "Starting..." : "Download & Install"}
          </button>
        {/if}
      </div>
    {:else}
      <!-- No update - show check button -->
      <button
        class="btn secondary"
        onclick={handleCheckForUpdate}
        disabled={isCheckingForUpdate}
      >
        {#if isCheckingForUpdate}
          <Icon name="refresh" size={14} class="spinning" />
          Checking...
        {:else}
          <Icon name="refresh" size={14} />
          Check for Updates
        {/if}
      </button>
      {#if lastUpdateCheckResult}
        <p class="update-result">{lastUpdateCheckResult}</p>
      {/if}
    {/if}
  </div>
</section>

<style>
  /* About section */
  .about-content {
    text-align: center;
    padding: var(--spacing-lg) 0;
  }

  .about-logo {
    width: 48px;
    height: 48px;
    margin: 0 auto var(--spacing-sm);
  }

  .about-logo svg {
    width: 100%;
    height: 100%;
  }

  .about-name {
    font-family: 'Outfit', var(--font-sans);
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .about-tagline {
    font-size: 13px;
    color: var(--text-muted);
    margin-bottom: var(--spacing-lg);
  }

  .about-info {
    margin-bottom: var(--spacing-md);
  }

  .info-row {
    font-size: 13px;
  }

  .info-label {
    color: var(--text-secondary);
  }

  .info-value {
    color: var(--text-primary);
    font-family: var(--font-mono);
  }

  .about-paths {
    text-align: left;
    max-width: 320px;
    margin: 0 auto var(--spacing-md);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-secondary);
    border-radius: 6px;
    font-size: 11px;
  }

  .path-row {
    display: flex;
    gap: var(--spacing-sm);
    margin-bottom: 4px;
  }

  .path-row:last-child {
    margin-bottom: 0;
  }

  .path-label {
    color: var(--text-muted);
    min-width: 70px;
  }

  .path-value {
    color: var(--text-secondary);
    font-family: var(--font-mono);
  }

  .about-links {
    font-size: 13px;
  }

  .link-separator {
    color: var(--text-muted);
    margin: 0 var(--spacing-sm);
  }

  /* Update card styles */
  .update-card {
    background: linear-gradient(135deg, rgba(37, 99, 235, 0.1) 0%, rgba(59, 130, 246, 0.1) 100%);
    border: 1px solid rgba(59, 130, 246, 0.3);
    border-radius: 8px;
    padding: var(--spacing-md);
  }

  .update-card-content {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-sm);
    margin-bottom: var(--spacing-md);
  }

  :global(.update-icon) {
    color: var(--accent-primary);
    flex-shrink: 0;
    margin-top: 2px;
  }

  :global(.update-icon.success) {
    color: var(--accent-success, #22c55e);
  }

  .update-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .update-info strong {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .update-info span {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .update-progress {
    height: 6px;
    background: rgba(255, 255, 255, 0.2);
    border-radius: 3px;
    overflow: hidden;
    margin-top: var(--spacing-sm);
  }

  .update-progress-fill {
    height: 100%;
    background: var(--accent-primary);
    transition: width 0.2s ease;
  }

  .update-result {
    font-size: 12px;
    color: var(--text-secondary);
    margin: var(--spacing-sm) 0 0 0;
  }
</style>
