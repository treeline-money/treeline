<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Icon } from "../shared";
  import {
    subscribeToUpdates,
    downloadAndInstall,
    restartApp,
    dismissUpdate,
    type UpdateState,
  } from "../sdk/updater";

  let updateState = $state<UpdateState>({
    available: false,
    version: null,
    notes: null,
    isDownloading: false,
    downloadProgress: 0,
    error: null,
  });

  let isInstalling = $state(false);
  let isRestarting = $state(false);
  let unsubscribe: (() => void) | null = null;

  onMount(() => {
    unsubscribe = subscribeToUpdates((state) => {
      updateState = state;
    });
  });

  onDestroy(() => {
    unsubscribe?.();
  });

  async function handleUpdate() {
    isInstalling = true;
    try {
      await downloadAndInstall();
      // After download completes, prompt to restart
      isInstalling = false;
    } catch (e) {
      isInstalling = false;
      console.error("Failed to install update:", e);
    }
  }

  async function handleRestart() {
    isRestarting = true;
    try {
      await restartApp();
    } catch (e) {
      isRestarting = false;
      console.error("Failed to restart:", e);
    }
  }

  function handleDismiss() {
    dismissUpdate();
  }

  // Determine what state we're in
  let showBanner = $derived(updateState.available || updateState.isDownloading);
  let isDownloadComplete = $derived(updateState.downloadProgress === 100 && !updateState.isDownloading);
</script>

{#if showBanner}
  <div class="update-banner">
    <span class="update-icon">
      <Icon name="refresh" size={16} />
    </span>
    <span class="update-text">
      {#if isDownloadComplete}
        <strong>Update ready!</strong> — Restart to apply v{updateState.version}
      {:else if updateState.isDownloading}
        <strong>Downloading update...</strong> — {updateState.downloadProgress}%
      {:else}
        <strong>Update available!</strong> — Treeline v{updateState.version} is ready
      {/if}
    </span>
    <div class="update-actions">
      {#if isDownloadComplete}
        <button
          class="update-btn primary"
          onclick={handleRestart}
          disabled={isRestarting}
        >
          {isRestarting ? "Restarting..." : "Restart Now"}
        </button>
      {:else if updateState.isDownloading}
        <div class="progress-bar">
          <div class="progress-fill" style="width: {updateState.downloadProgress}%"></div>
        </div>
      {:else}
        <button
          class="update-btn primary"
          onclick={handleUpdate}
          disabled={isInstalling}
        >
          {isInstalling ? "Downloading..." : "Update Now"}
        </button>
        <button class="update-btn secondary" onclick={handleDismiss}>
          Later
        </button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .update-banner {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 1rem;
    background: linear-gradient(90deg, #2563eb 0%, #3b82f6 100%);
    color: white;
    font-size: 0.875rem;
  }

  .update-icon {
    display: flex;
    align-items: center;
  }

  .update-text {
    flex: 1;
  }

  .update-text strong {
    font-weight: 600;
  }

  .update-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .update-btn {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.75rem;
    border-radius: 4px;
    font-size: 0.75rem;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .update-btn.primary {
    background: rgba(255, 255, 255, 0.95);
    border: none;
    color: #2563eb;
  }

  .update-btn.primary:hover:not(:disabled) {
    background: white;
  }

  .update-btn.secondary {
    background: rgba(255, 255, 255, 0.2);
    border: 1px solid rgba(255, 255, 255, 0.3);
    color: white;
  }

  .update-btn.secondary:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.3);
  }

  .update-btn:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  .progress-bar {
    width: 120px;
    height: 6px;
    background: rgba(255, 255, 255, 0.3);
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: white;
    transition: width 0.2s ease;
  }
</style>
