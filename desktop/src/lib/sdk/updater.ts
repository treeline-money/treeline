/**
 * Auto-Update Service
 *
 * Handles checking for updates, downloading, and installing.
 * Respects user preferences for automatic updates.
 *
 * Uses custom Rust commands that support staging updates via
 * `~/.treeline/use-staging-updates` file for testing release candidates.
 */

import { invoke } from "@tauri-apps/api/core";
import { relaunch, exit } from "@tauri-apps/plugin-process";
import { getAppSetting, setAppSetting } from "./settings";

/** Update info returned from Rust check_for_app_update command */
interface AppUpdateInfo {
  version: string;
  body: string | null;
  date: string | null;
}

/** Simplified Update type matching what we need from the original */
interface Update {
  version: string;
  body: string | null;
}

// Check interval: 24 hours in milliseconds
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

// Minimum time between checks (to avoid spam on app restarts)
const MIN_CHECK_INTERVAL_MS = 60 * 1000; // 1 minute

// Store the current update if available
let availableUpdate: Update | null = null;
let checkIntervalId: ReturnType<typeof setInterval> | null = null;
let isDownloading = false;
let downloadProgress = 0;

// Subscribers for update state changes
type UpdateSubscriber = (state: UpdateState) => void;
const subscribers: Set<UpdateSubscriber> = new Set();

export interface UpdateState {
  available: boolean;
  version: string | null;
  notes: string | null;
  isDownloading: boolean;
  downloadProgress: number;
  error: string | null;
}

function getState(): UpdateState {
  return {
    available: availableUpdate !== null,
    version: availableUpdate?.version ?? null,
    notes: availableUpdate?.body ?? null,
    isDownloading,
    downloadProgress,
    error: null,
  };
}

function notifySubscribers(state?: UpdateState): void {
  const currentState = state ?? getState();
  subscribers.forEach((callback) => callback(currentState));
}

/**
 * Subscribe to update state changes
 */
export function subscribeToUpdates(callback: UpdateSubscriber): () => void {
  subscribers.add(callback);
  // Immediately notify with current state
  callback(getState());
  return () => subscribers.delete(callback);
}

/**
 * Check if enough time has passed since last check
 */
async function shouldCheck(): Promise<boolean> {
  const lastCheck = await getAppSetting("lastUpdateCheck");
  if (!lastCheck) return true;

  const lastCheckTime = new Date(lastCheck).getTime();
  const now = Date.now();
  return now - lastCheckTime >= MIN_CHECK_INTERVAL_MS;
}

/**
 * Check for updates
 * Returns the update info if available, null if no update, throws on error
 *
 * Uses custom Rust command that supports staging updates via ~/.treeline/use-staging-updates
 */
export async function checkForUpdate(force = false): Promise<Update | null> {
  // Don't check too frequently unless forced
  if (!force && !(await shouldCheck())) {
    return availableUpdate;
  }

  try {
    console.log("Checking for updates...");
    const update = await invoke<AppUpdateInfo | null>("check_for_app_update");
    console.log("Update check result:", update ? `v${update.version} available` : "no update");
    availableUpdate = update;

    // Record the check time
    await setAppSetting("lastUpdateCheck", new Date().toISOString());

    notifySubscribers();
    return update;
  } catch (error) {
    console.error("Failed to check for updates:", error);

    // Provide a user-friendly error message
    // Check failures often happen when release assets aren't ready yet (~30 min after release)
    const rawError = error instanceof Error ? error.message : String(error);
    const isReleaseNotReady =
      rawError.includes("404") ||
      rawError.includes("fetch") ||
      rawError.includes("release") ||
      rawError.includes("JSON") ||
      rawError.includes("Not Found");

    const friendlyError = isReleaseNotReady
      ? "Update not ready yet. Please try again in a few minutes."
      : rawError;

    notifySubscribers({
      ...getState(),
      error: friendlyError,
    });
    // Re-throw so callers know there was an error (vs no update available)
    throw new Error(friendlyError);
  }
}

/**
 * Download and install the available update
 *
 * Uses custom Rust command that supports staging updates
 */
export async function downloadAndInstall(): Promise<void> {
  if (!availableUpdate) {
    throw new Error("No update available");
  }

  if (isDownloading) {
    throw new Error("Download already in progress");
  }

  isDownloading = true;
  downloadProgress = 0;
  notifySubscribers();

  try {
    // Call Rust command to download and install
    // The Update is stored in Rust state from the check call
    await invoke("download_and_install_app_update");

    downloadProgress = 100;
    isDownloading = false;
    notifySubscribers();
  } catch (error) {
    isDownloading = false;
    downloadProgress = 0;

    // Provide a user-friendly error message
    // Download failures often happen when release assets aren't ready yet (~30 min after release)
    const rawError = error instanceof Error ? error.message : String(error);
    const isDownloadError =
      rawError.includes("404") ||
      rawError.includes("network") ||
      rawError.includes("fetch") ||
      rawError.includes("download") ||
      rawError.includes("Not Found");

    const friendlyError = isDownloadError
      ? "Update not ready yet. Please try again in a few minutes."
      : rawError;

    notifySubscribers({
      ...getState(),
      error: friendlyError,
    });
    throw new Error(friendlyError);
  }
}

/**
 * Restart the application to apply the update
 * Tries relaunch first, falls back to exit if it fails
 * (relaunch has known issues on some platforms in Tauri v2)
 */
export async function restartApp(): Promise<void> {
  try {
    await relaunch();
  } catch (error) {
    console.warn("Relaunch failed, falling back to exit:", error);
    // Fall back to just exiting - user will need to reopen the app
    // The update will be applied on next launch
    await exit(0);
  }
}

/**
 * Dismiss the current update notification
 * The update will be shown again on next check
 */
export function dismissUpdate(): void {
  availableUpdate = null;
  notifySubscribers();
}

/**
 * Start periodic update checks (every 24 hours)
 */
export function startPeriodicChecks(): void {
  if (checkIntervalId) return;

  checkIntervalId = setInterval(async () => {
    const autoUpdate = await getAppSetting("autoUpdate");
    if (autoUpdate) {
      await checkForUpdate();
    }
  }, CHECK_INTERVAL_MS);
}

/**
 * Stop periodic update checks
 */
export function stopPeriodicChecks(): void {
  if (checkIntervalId) {
    clearInterval(checkIntervalId);
    checkIntervalId = null;
  }
}

/**
 * Initialize the updater service
 * Should be called once on app startup
 */
export async function initUpdater(): Promise<void> {
  // Check for updates on startup
  const autoUpdate = await getAppSetting("autoUpdate");

  // Always check on startup (if auto-update is enabled)
  if (autoUpdate) {
    try {
      await checkForUpdate();
    } catch (error) {
      // Don't fail app startup on update check error
      console.error("Update check on startup failed:", error);
    }
  }

  // Start periodic checks
  startPeriodicChecks();
}

/**
 * Get current update state
 */
export function getUpdateState(): UpdateState {
  return getState();
}
