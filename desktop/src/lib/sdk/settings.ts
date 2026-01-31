/**
 * Settings Service
 *
 * Provides a unified interface for app and plugin settings.
 *
 * Pattern for plugin developers:
 * - Settings (this file): User preferences, editable via Settings UI
 * - State (pluginState): Plugin runtime data, not user-editable
 * - Plugin files (read/write_plugin_config): Domain data like budget months
 */

import { invoke } from "@tauri-apps/api/core";
import { withActivity } from "./activity.svelte";

/**
 * App-level settings structure
 */
export interface AppSettings {
  theme: string; // Theme ID: "dark", "light", or "system"
  lastSyncDate: string | null;
  autoSyncOnStartup: boolean;
  autoUpdate: boolean;
  lastUpdateCheck?: string | null;
  hasCompletedOnboarding?: boolean;
  sidebarCollapsed?: boolean;
  hideDemoBanner?: boolean;
  currency?: string;
  lastSeenVersion?: string | null;
  developerMode?: boolean; // Enable DevTools for plugin development
}

/**
 * Full settings structure
 */
export interface Settings {
  app: AppSettings;
  plugins: Record<string, Record<string, unknown>>;
  disabledPlugins?: string[];
  importProfiles?: Partial<ImportProfilesContainer>;
}

/**
 * Default settings
 */
const DEFAULT_SETTINGS: Settings = {
  app: {
    theme: "system",
    lastSyncDate: null,
    autoSyncOnStartup: true,
    autoUpdate: true,
    lastUpdateCheck: null,
  },
  plugins: {},
};

// In-memory cache of settings
let settingsCache: Settings | null = null;

// Subscribers for settings changes
type SettingsSubscriber = (settings: Settings) => void;
const subscribers: Set<SettingsSubscriber> = new Set();

/**
 * Invalidate the settings cache (forces next read from disk)
 * Call this after external processes modify settings.json
 */
export function invalidateSettingsCache(): void {
  settingsCache = null;
}

/**
 * Read all settings from disk
 */
export async function readSettings(): Promise<Settings> {
  const jsonString = await invoke<string>("read_settings");
  const parsed = JSON.parse(jsonString);

  // Merge with defaults to ensure all fields exist
  // IMPORTANT: Preserve importProfiles to avoid deleting user's bank profiles
  settingsCache = {
    app: { ...DEFAULT_SETTINGS.app, ...parsed.app },
    plugins: { ...DEFAULT_SETTINGS.plugins, ...parsed.plugins },
    disabledPlugins: parsed.disabledPlugins || [],
    importProfiles: parsed.importProfiles,
  };

  return settingsCache;
}

/**
 * Write all settings to disk
 */
export async function writeSettings(settings: Settings): Promise<void> {
  await invoke("write_settings", { content: JSON.stringify(settings, null, 2) });
  settingsCache = settings;
  notifySubscribers();
}

/**
 * Get current settings (from cache or disk)
 */
export async function getSettings(): Promise<Settings> {
  if (settingsCache) {
    return settingsCache;
  }
  return readSettings();
}

/**
 * Get a specific app setting
 */
export async function getAppSetting<K extends keyof AppSettings>(key: K): Promise<AppSettings[K]> {
  const settings = await getSettings();
  return settings.app[key];
}

/**
 * Set a specific app setting
 */
export async function setAppSetting<K extends keyof AppSettings>(
  key: K,
  value: AppSettings[K]
): Promise<void> {
  const settings = await getSettings();
  settings.app[key] = value;
  await writeSettings(settings);
}

/**
 * Get all settings for a plugin
 */
export async function getPluginSettings<T extends Record<string, unknown>>(
  pluginId: string,
  defaults: T
): Promise<T> {
  const settings = await getSettings();
  const pluginSettings = settings.plugins[pluginId] || {};
  return { ...defaults, ...pluginSettings } as T;
}

/**
 * Set all settings for a plugin
 */
export async function setPluginSettings(
  pluginId: string,
  pluginSettings: Record<string, unknown>
): Promise<void> {
  const settings = await getSettings();
  settings.plugins[pluginId] = pluginSettings;
  await writeSettings(settings);
}

/**
 * Update specific plugin settings (merge with existing)
 */
export async function updatePluginSettings(
  pluginId: string,
  updates: Record<string, unknown>
): Promise<void> {
  const settings = await getSettings();
  settings.plugins[pluginId] = {
    ...(settings.plugins[pluginId] || {}),
    ...updates,
  };
  await writeSettings(settings);
}

/**
 * Subscribe to settings changes
 */
export function subscribeToSettings(callback: SettingsSubscriber): () => void {
  subscribers.add(callback);
  return () => subscribers.delete(callback);
}

/**
 * Notify all subscribers of settings changes
 */
function notifySubscribers(): void {
  if (settingsCache) {
    subscribers.forEach((callback) => callback(settingsCache!));
  }
}

/**
 * Clear settings cache (useful for testing or forced refresh)
 */
export function clearSettingsCache(): void {
  settingsCache = null;
}

// ============================================================================
// Plugin Enable/Disable
// ============================================================================

/**
 * Check if a plugin is disabled
 */
export async function isPluginDisabled(pluginId: string): Promise<boolean> {
  const settings = await getSettings();
  return (settings.disabledPlugins || []).includes(pluginId);
}

/**
 * Get list of disabled plugin IDs
 */
export async function getDisabledPlugins(): Promise<string[]> {
  const settings = await getSettings();
  return settings.disabledPlugins || [];
}

/**
 * Enable a plugin (remove from disabled list)
 * Requires app reload to take effect
 */
export async function enablePlugin(pluginId: string): Promise<void> {
  const settings = await getSettings();
  settings.disabledPlugins = (settings.disabledPlugins || []).filter(id => id !== pluginId);
  await writeSettings(settings);
}

/**
 * Disable a plugin (add to disabled list)
 * Requires app reload to take effect
 */
export async function disablePlugin(pluginId: string): Promise<void> {
  const settings = await getSettings();
  if (!settings.disabledPlugins) {
    settings.disabledPlugins = [];
  }
  if (!settings.disabledPlugins.includes(pluginId)) {
    settings.disabledPlugins.push(pluginId);
  }
  await writeSettings(settings);
}

// ============================================================================
// Plugin State (separate from settings - runtime state, not user preferences)
// ============================================================================

/**
 * Read plugin state (runtime state, not user settings)
 */
export async function readPluginState<T>(pluginId: string): Promise<T | null> {
  const jsonString = await invoke<string>("read_plugin_state", { pluginId });
  if (jsonString === "null") {
    return null;
  }
  return JSON.parse(jsonString) as T;
}

/**
 * Write plugin state (runtime state, not user settings)
 */
export async function writePluginState<T>(pluginId: string, state: T): Promise<void> {
  await invoke("write_plugin_state", {
    pluginId,
    content: JSON.stringify(state, null, 2)
  });
}

// ============================================================================
// Sync
// ============================================================================

export interface SyncResult {
  results: Array<{
    integration: string;
    accounts_synced: number;
    transactions_synced: number;
    transaction_stats?: {
      discovered: number;
      new: number;
      skipped: number;
    };
    provider_warnings?: string[];
    error?: string;
  }>;
}

export interface RunSyncOptions {
  dryRun?: boolean;
  balancesOnly?: boolean;
}

/**
 * Run sync and update lastSyncDate (unless dry run)
 */
export async function runSync(options: RunSyncOptions = {}): Promise<SyncResult> {
  const { dryRun = false, balancesOnly = false } = options;
  const jsonString = await invoke<string>("run_sync", { dryRun, balancesOnly });
  const result = JSON.parse(jsonString) as SyncResult;

  // Update lastSyncDate on success (but not for dry runs)
  if (!dryRun) {
    const today = new Date().toISOString().split("T")[0];
    await setAppSetting("lastSyncDate", today);
  }

  return result;
}

/**
 * Check if sync is needed (based on lastSyncDate)
 */
export async function isSyncNeeded(): Promise<boolean> {
  const settings = await getSettings();

  if (!settings.app.autoSyncOnStartup) {
    return false;
  }

  const lastSyncDate = settings.app.lastSyncDate;
  if (!lastSyncDate) {
    return true;
  }

  const today = new Date().toISOString().split("T")[0];
  return lastSyncDate < today;
}

// ============================================================================
// Demo Mode
// ============================================================================

/**
 * Get current demo mode status
 */
export async function getDemoMode(): Promise<boolean> {
  return invoke<boolean>("get_demo_mode");
}

/**
 * Set demo mode (requires window reload to take effect)
 */
export async function setDemoMode(enabled: boolean): Promise<void> {
  await invoke("set_demo_mode", { enabled });
}

/**
 * Enable demo mode (sets up demo integration and syncs demo data)
 */
export async function enableDemo(): Promise<void> {
  await withActivity("Enabling demo mode", async () => {
    await invoke("enable_demo");
  });
  // rust-core modifies config directly, so invalidate our cache
  invalidateSettingsCache();
}

/**
 * Disable demo mode
 */
export async function disableDemo(): Promise<void> {
  await invoke("disable_demo");
  // rust-core modifies config directly, so invalidate our cache
  invalidateSettingsCache();
}

// ============================================================================
// Balance Backfill (Preview/Execute Pattern)
// ============================================================================

/**
 * Summary of a transaction for preview display
 */
export interface TransactionSummary {
  description: string;
  amount: number;
}

/**
 * Preview of a single balance snapshot for UI display
 */
export interface BalanceSnapshotPreview {
  date: string;
  /** The calculated balance for this date (end of day) */
  balance: number;
  /** Net transaction amount for this day (positive = inflow, negative = outflow), 0 if no transactions */
  daily_change: number;
  /** Transactions that occurred on this day */
  transactions: TransactionSummary[];
  /** True if no existing snapshot exists (will be created) */
  is_new: boolean;
  /** The existing balance if one exists (will be replaced) */
  existing_balance: number | null;
  /** Source of existing snapshot: "sync", "manual", "backfill", "import", or null */
  existing_source: string | null;
}

/**
 * Result of executing balance backfill
 */
export interface BackfillExecuteResult {
  snapshots_created: number;
  snapshots_updated: number;
  snapshots_skipped: number;
}

/**
 * Preview balance backfill - shows what snapshots would be created/updated
 * Returns a list of calculated end-of-day balances without persisting them
 *
 * @param accountId - The account ID to preview backfill for
 * @param knownBalance - A known balance amount
 * @param knownDate - The date of the known balance (YYYY-MM-DD format)
 * @param startDate - Optional start of date range (YYYY-MM-DD format)
 * @param endDate - Optional end of date range (YYYY-MM-DD format)
 */
export async function backfillPreview(
  accountId: string,
  knownBalance: number,
  knownDate: string,
  startDate?: string,
  endDate?: string
): Promise<BalanceSnapshotPreview[]> {
  return invoke<BalanceSnapshotPreview[]>("backfill_preview", {
    accountId,
    knownBalance,
    knownDate,
    startDate: startDate || null,
    endDate: endDate || null,
  });
}

/**
 * Execute balance backfill - creates/updates balance snapshots
 * Replaces all existing snapshots in range with calculated values
 *
 * @param accountId - The account ID to backfill
 * @param knownBalance - A known balance amount
 * @param knownDate - The date of the known balance (YYYY-MM-DD format)
 * @param startDate - Optional start of date range (YYYY-MM-DD format)
 * @param endDate - Optional end of date range (YYYY-MM-DD format)
 */
export async function backfillExecute(
  accountId: string,
  knownBalance: number,
  knownDate: string,
  startDate?: string,
  endDate?: string
): Promise<BackfillExecuteResult> {
  return invoke<BackfillExecuteResult>("backfill_execute", {
    accountId,
    knownBalance,
    knownDate,
    startDate: startDate || null,
    endDate: endDate || null,
  });
}

// ============================================================================
// CSV Import
// ============================================================================

export interface ImportColumnMapping {
  dateColumn?: string;
  amountColumn?: string;
  descriptionColumn?: string;
  debitColumn?: string;
  creditColumn?: string;
  /** Optional running balance column - creates balance snapshots when imported */
  balanceColumn?: string;
}

export type NumberFormat = "us" | "eu" | "eu_space";

export interface ImportPreviewResult {
  file: string;
  flip_signs: boolean;
  debit_negative: boolean;
  skip_rows: number;
  number_format: NumberFormat;
  preview: Array<{
    date: string;
    description: string | null;
    amount: number;
    balance?: number | null;
  }>;
}

export interface ImportExecuteResult {
  batch_id: string;
  discovered: number;
  imported: number;
  skipped: number;
  fingerprints_checked: number;
  /** Number of balance snapshots created from running balance column */
  balance_snapshots_created: number;
}

/**
 * Open file picker dialog for CSV files
 */
export async function pickCsvFile(): Promise<string | null> {
  const result = await invoke<string | null>("pick_csv_file");
  return result;
}

/**
 * Get CSV column headers for mapping UI
 * @param filePath Path to the CSV file
 * @param skipRows Number of rows to skip before the header row (default: 0)
 */
export async function getCsvHeaders(
  filePath: string,
  skipRows: number = 0
): Promise<string[]> {
  return invoke<string[]>("get_csv_headers", { filePath, skipRows });
}

/**
 * Preview CSV import (detect columns, show first few transactions)
 * If anchorBalance and anchorDate are provided, calculates historical balances in preview
 */
export async function importCsvPreview(
  filePath: string,
  accountId: string,
  columnMapping: ImportColumnMapping = {},
  flipSigns: boolean = false,
  debitNegative: boolean = false,
  skipRows: number = 0,
  numberFormat: NumberFormat = "us",
  anchorBalance?: number,
  anchorDate?: string
): Promise<ImportPreviewResult> {
  const jsonString = await invoke<string>("import_csv_preview", {
    filePath,
    accountId,
    dateColumn: columnMapping.dateColumn || null,
    amountColumn: columnMapping.amountColumn || null,
    descriptionColumn: columnMapping.descriptionColumn || null,
    debitColumn: columnMapping.debitColumn || null,
    creditColumn: columnMapping.creditColumn || null,
    balanceColumn: columnMapping.balanceColumn || null,
    flipSigns,
    debitNegative,
    skipRows,
    numberFormat,
    anchorBalance: anchorBalance ?? null,
    anchorDate: anchorDate ?? null,
  });
  return JSON.parse(jsonString) as ImportPreviewResult;
}

/**
 * Execute CSV import
 */
export async function importCsvExecute(
  filePath: string,
  accountId: string,
  columnMapping: ImportColumnMapping = {},
  flipSigns: boolean = false,
  debitNegative: boolean = false,
  skipRows: number = 0,
  numberFormat: NumberFormat = "us"
): Promise<ImportExecuteResult> {
  const jsonString = await invoke<string>("import_csv_execute", {
    filePath,
    accountId,
    dateColumn: columnMapping.dateColumn || null,
    amountColumn: columnMapping.amountColumn || null,
    descriptionColumn: columnMapping.descriptionColumn || null,
    debitColumn: columnMapping.debitColumn || null,
    creditColumn: columnMapping.creditColumn || null,
    balanceColumn: columnMapping.balanceColumn || null,
    flipSigns,
    debitNegative,
    skipRows,
    numberFormat,
  });
  return JSON.parse(jsonString) as ImportExecuteResult;
}

// ============================================================================
// Import Profiles (named, reusable across accounts)
// ============================================================================

export interface ImportProfileColumnMappings {
  date?: string;
  amount?: string;
  description?: string;
  debit?: string;
  credit?: string;
  /** Optional running balance column - creates balance snapshots when imported */
  balance?: string;
}

export interface ImportProfileOptions {
  flipSigns?: boolean;
  debitNegative?: boolean;
  skipRows?: number;
  numberFormat?: NumberFormat;
}

export interface ImportProfile {
  columnMappings: ImportProfileColumnMappings;
  options: ImportProfileOptions;
}

/**
 * Container for import profiles and account mappings
 */
export interface ImportProfilesContainer {
  profiles: Record<string, ImportProfile>;
  accountMappings: Record<string, string>; // accountId -> profileName
}

interface SettingsWithProfiles extends Settings {
  importProfiles?: Partial<ImportProfilesContainer>;
}

/**
 * Get the import profiles container, ensuring nested structure
 */
function ensureImportProfilesContainer(settings: SettingsWithProfiles): ImportProfilesContainer {
  if (!settings.importProfiles) {
    settings.importProfiles = {};
  }
  if (!settings.importProfiles.profiles) {
    settings.importProfiles.profiles = {};
  }
  if (!settings.importProfiles.accountMappings) {
    settings.importProfiles.accountMappings = {};
  }
  return settings.importProfiles as ImportProfilesContainer;
}

/**
 * Get all import profiles
 */
export async function getImportProfiles(): Promise<Record<string, ImportProfile>> {
  const jsonString = await invoke<string>("read_settings");
  const settings = JSON.parse(jsonString) as SettingsWithProfiles;
  const container = ensureImportProfilesContainer(settings);
  return container.profiles;
}

/**
 * Get a specific import profile by name
 */
export async function getImportProfile(name: string): Promise<ImportProfile | null> {
  const profiles = await getImportProfiles();
  return profiles[name] || null;
}

/**
 * Get list of all profile names
 */
export async function listImportProfiles(): Promise<string[]> {
  const profiles = await getImportProfiles();
  return Object.keys(profiles);
}

/**
 * Save or update a named import profile
 */
export async function saveImportProfile(
  name: string,
  columnMappings: ImportProfileColumnMappings,
  options: ImportProfileOptions = {}
): Promise<void> {
  const jsonString = await invoke<string>("read_settings");
  const settings = JSON.parse(jsonString) as SettingsWithProfiles;
  const container = ensureImportProfilesContainer(settings);

  container.profiles[name] = {
    columnMappings,
    options,
  };

  await invoke("write_settings", { content: JSON.stringify(settings, null, 2) });
  invalidateSettingsCache();
}

/**
 * Delete an import profile by name
 */
export async function deleteImportProfile(name: string): Promise<boolean> {
  const jsonString = await invoke<string>("read_settings");
  const settings = JSON.parse(jsonString) as SettingsWithProfiles;
  const container = ensureImportProfilesContainer(settings);

  if (!container.profiles[name]) {
    return false;
  }

  delete container.profiles[name];
  await invoke("write_settings", { content: JSON.stringify(settings, null, 2) });
  invalidateSettingsCache();
  return true;
}

// ============================================================================
// Account to Profile Mappings
// ============================================================================

/**
 * Get the profile name mapped to an account
 */
export async function getAccountProfileMapping(accountId: string): Promise<string | null> {
  const jsonString = await invoke<string>("read_settings");
  const settings = JSON.parse(jsonString) as SettingsWithProfiles;
  const container = ensureImportProfilesContainer(settings);
  return container.accountMappings[accountId] || null;
}

/**
 * Set the profile mapping for an account
 */
export async function setAccountProfileMapping(accountId: string, profileName: string): Promise<void> {
  const jsonString = await invoke<string>("read_settings");
  const settings = JSON.parse(jsonString) as SettingsWithProfiles;
  const container = ensureImportProfilesContainer(settings);

  container.accountMappings[accountId] = profileName;

  await invoke("write_settings", { content: JSON.stringify(settings, null, 2) });
  invalidateSettingsCache();
}

/**
 * Remove the profile mapping for an account
 */
export async function removeAccountProfileMapping(accountId: string): Promise<boolean> {
  const jsonString = await invoke<string>("read_settings");
  const settings = JSON.parse(jsonString) as SettingsWithProfiles;
  const container = ensureImportProfilesContainer(settings);

  if (!container.accountMappings[accountId]) {
    return false;
  }

  delete container.accountMappings[accountId];
  await invoke("write_settings", { content: JSON.stringify(settings, null, 2) });
  invalidateSettingsCache();
  return true;
}

/**
 * Get all account to profile mappings
 */
export async function getAccountProfileMappings(): Promise<Record<string, string>> {
  const jsonString = await invoke<string>("read_settings");
  const settings = JSON.parse(jsonString) as SettingsWithProfiles;
  const container = ensureImportProfilesContainer(settings);
  return container.accountMappings;
}

// ============================================================================
// Integrations
// ============================================================================

/**
 * Setup SimpleFIN integration with a setup token
 */
export async function setupSimplefin(token: string): Promise<string> {
  return invoke<string>("setup_simplefin", { token });
}

/**
 * Setup Lunchflow integration with an API key
 *
 * Lunchflow is a multi-provider bank aggregator supporting 20,000+ banks
 * across 40+ countries (EU, UK, US, Canada, Asia, Brazil).
 *
 * @param apiKey - The Lunchflow API key from the user's dashboard
 * @param baseUrl - Optional custom base URL for testing (omit for production)
 */
export async function setupLunchflow(
  apiKey: string,
  baseUrl?: string
): Promise<string> {
  return invoke<string>("setup_lunchflow", { apiKey, baseUrl });
}

// ============================================================================
// Integration Account Settings
// ============================================================================

/**
 * Get integration settings from the database
 */
export async function getIntegrationSettings(integrationName: string): Promise<Record<string, unknown>> {
  const result = await invoke<string>("execute_query_with_params", {
    query: `SELECT integration_settings FROM sys_integrations WHERE integration_name = ?`,
    params: [integrationName],
    readonly: true,
  });
  const parsed = JSON.parse(result);
  if (parsed.rows && parsed.rows.length > 0 && parsed.rows[0][0]) {
    return JSON.parse(parsed.rows[0][0]);
  }
  return {};
}

/**
 * Update the balancesOnly setting for a specific account within an integration
 * When true, sync will only fetch balances for this account (not transactions)
 *
 * @param integrationName - The integration name (e.g., "simplefin")
 * @param providerAccountId - The provider's account ID (e.g., SimpleFIN account ID)
 * @param balancesOnly - Whether to only sync balances for this account
 */
export async function updateIntegrationAccountSetting(
  integrationName: string,
  providerAccountId: string,
  balancesOnly: boolean
): Promise<void> {
  // Get current integration settings
  const settings = await getIntegrationSettings(integrationName);

  // Initialize accountSettings if it doesn't exist
  if (!settings.accountSettings) {
    settings.accountSettings = {};
  }

  // Update the specific account's settings
  const accountSettings = settings.accountSettings as Record<string, { balancesOnly?: boolean }>;
  if (!accountSettings[providerAccountId]) {
    accountSettings[providerAccountId] = {};
  }
  accountSettings[providerAccountId].balancesOnly = balancesOnly;

  // Write back to database
  const settingsJson = JSON.stringify(settings);
  await invoke<string>("execute_query_with_params", {
    query: `UPDATE sys_integrations SET integration_settings = ? WHERE integration_name = ?`,
    params: [settingsJson, integrationName],
    readonly: false,
  });
}

// ============================================================================
// Community Plugin Installation
// ============================================================================

export interface PluginInstallResult {
  success: boolean;
  plugin_id: string;
  plugin_name: string;
  version: string;
  install_dir: string;
  source?: string;
  error?: string;
}

/**
 * Install a plugin from a GitHub URL
 * Downloads pre-built release assets (manifest.json and index.js)
 *
 * @param url - GitHub repository URL (e.g., https://github.com/user/repo)
 * @param version - Optional version tag (e.g., "v1.0.0"). Defaults to latest release.
 */
export async function installPlugin(url: string, version?: string): Promise<PluginInstallResult> {
  const jsonString = await invoke<string>("install_plugin", { url, version: version || null });
  return JSON.parse(jsonString) as PluginInstallResult;
}

/**
 * Uninstall a plugin by ID
 *
 * @param pluginId - The plugin ID to uninstall
 */
export async function uninstallPlugin(pluginId: string): Promise<{ success: boolean; plugin_id: string; plugin_name: string }> {
  const jsonString = await invoke<string>("uninstall_plugin", { pluginId });
  return JSON.parse(jsonString);
}

// ============================================================================
// Encryption
// ============================================================================

export interface EncryptionStatus {
  encrypted: boolean;
  locked: boolean;
  algorithm: string | null;
  version: number | null;
}

/**
 * Get current encryption status
 */
export async function getEncryptionStatus(): Promise<EncryptionStatus> {
  return invoke<EncryptionStatus>("get_encryption_status");
}

/**
 * Try to auto-unlock using keychain key (called on app startup)
 * Returns true if unlocked (or not encrypted), false if needs manual unlock
 */
export async function tryAutoUnlock(): Promise<boolean> {
  return invoke<boolean>("try_auto_unlock");
}

/**
 * Unlock encrypted database with password
 * @param password - The encryption password
 */
export async function unlockDatabase(password: string): Promise<void> {
  return invoke<void>("unlock_database", { password });
}

/**
 * Enable encryption on the database
 * @param password - The new encryption password
 */
export async function enableEncryption(password: string): Promise<void> {
  return invoke<void>("enable_encryption", { password });
}

/**
 * Disable encryption on the database
 * @param password - The current encryption password
 */
export async function disableEncryption(password: string): Promise<void> {
  return invoke<void>("disable_encryption", { password });
}

// ============================================================================
// Watch Folder / Pending Imports
// ============================================================================

/**
 * Pending import file information
 */
export interface PendingImportFile {
  path: string;
  filename: string;
  size_bytes: number;
}

/**
 * List CSV files waiting in the imports folder (~/.treeline/imports/)
 */
export async function listPendingImports(): Promise<PendingImportFile[]> {
  return invoke<PendingImportFile[]>("list_pending_imports");
}

/**
 * Move an imported file to the "imported" subfolder
 * Call this after a successful import to clean up the pending file
 */
export async function moveImportedFile(filePath: string): Promise<void> {
  return invoke<void>("move_imported_file", { filePath });
}

// ============================================================================
// Backup & Restore
// ============================================================================

export interface BackupMetadata {
  name: string;
  created_at: string;
  size_bytes: number;
}

/**
 * List all available backups
 */
export async function listBackups(): Promise<BackupMetadata[]> {
  const jsonString = await invoke<string>("list_backups");
  return JSON.parse(jsonString) as BackupMetadata[];
}

/**
 * Create a new backup
 * @param maxBackups - Optional max number of backups to retain (auto-deletes oldest)
 */
export async function createBackup(maxBackups?: number): Promise<BackupMetadata> {
  const jsonString = await invoke<string>("create_backup", { maxBackups: maxBackups || null });
  return JSON.parse(jsonString) as BackupMetadata;
}

/**
 * Restore from a backup
 * @param backupName - The backup filename to restore from
 */
export async function restoreBackup(backupName: string): Promise<void> {
  return invoke<void>("restore_backup", { backupName });
}

/**
 * Delete a backup
 * @param backupName - The backup filename to delete
 */
export async function deleteBackup(backupName: string): Promise<void> {
  return invoke<void>("delete_backup", { backupName });
}

export interface ClearBackupsResult {
  deleted_count: number;
}

/**
 * Clear all backups
 */
export async function clearBackups(): Promise<ClearBackupsResult> {
  const jsonString = await invoke<string>("clear_backups");
  return JSON.parse(jsonString) as ClearBackupsResult;
}

// ============================================================================
// Database Compact
// ============================================================================

export interface CompactResult {
  original_size: number;
  compacted_size: number;
}

/**
 * Compact the database (CHECKPOINT + VACUUM)
 * Reduces file size by reclaiming unused space
 */
export async function compactDatabase(): Promise<CompactResult> {
  const jsonString = await invoke<string>("compact_database");
  return JSON.parse(jsonString) as CompactResult;
}

/**
 * Format bytes to human-readable size
 */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}
