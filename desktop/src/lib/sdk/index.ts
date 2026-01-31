/**
 * Treeline Plugin SDK
 *
 * This is the main entry point for plugin authors.
 * Import from '@treeline/sdk' (aliased in the build config)
 */

// Types
export type {
  Plugin,
  PluginManifest,
  PluginPermissions,
  PluginContext,
  SidebarSection,
  SidebarItem,
  ViewDefinition,
  Command,
  StatusBarItem,
  Tab,
  ThemeInterface,
} from "./types";

// Public SDK (for external plugins)
export { createPluginSDK } from "./public";
export type { PluginSDK, PluginTablePermissions } from "./public";

// Registry (for core use)
export { registry } from "./registry";

// API
export { executeQuery, executeQueryWithParams, db, deleteAccount } from "./api";
export type { QueryResult, ExecuteQueryOptions, QueryParam } from "./api";

// Theme
export { themeManager } from "./theme";
export type { ThemeDefinition } from "./theme";

// Settings
export {
  readSettings,
  writeSettings,
  getSettings,
  getAppSetting,
  setAppSetting,
  getPluginSettings,
  setPluginSettings,
  updatePluginSettings,
  subscribeToSettings,
  clearSettingsCache,
  readPluginState,
  writePluginState,
  runSync,
  isSyncNeeded,
  getDemoMode,
  setDemoMode,
  enableDemo,
  disableDemo,
  // Plugin enable/disable
  isPluginDisabled,
  getDisabledPlugins,
  enablePlugin,
  disablePlugin,
  // Backfill
  backfillPreview,
  backfillExecute,
  // CSV Import
  pickCsvFile,
  getCsvHeaders,
  importCsvPreview,
  importCsvExecute,
  // Import Profiles
  getImportProfiles,
  getImportProfile,
  listImportProfiles,
  saveImportProfile,
  deleteImportProfile,
  // Account to Profile Mappings
  getAccountProfileMapping,
  setAccountProfileMapping,
  removeAccountProfileMapping,
  getAccountProfileMappings,
  // Integrations
  setupSimplefin,
  setupLunchflow,
  // Integration Account Settings
  getIntegrationSettings,
  updateIntegrationAccountSetting,
  // Community Plugins
  installPlugin,
  uninstallPlugin,
  // Encryption
  getEncryptionStatus,
  tryAutoUnlock,
  unlockDatabase,
  enableEncryption,
  disableEncryption,
  // Watch Folder / Pending Imports
  listPendingImports,
  moveImportedFile,
  // Backup & Restore
  listBackups,
  createBackup,
  restoreBackup,
  deleteBackup,
  clearBackups,
  // Database Compact
  compactDatabase,
  formatBytes,
} from "./settings";
export type {
  Settings,
  AppSettings,
  SyncResult,
  ImportColumnMapping,
  ImportPreviewResult,
  ImportExecuteResult,
  ImportProfile,
  ImportProfileColumnMappings,
  ImportProfileOptions,
  ImportProfilesContainer,
  PluginInstallResult,
  EncryptionStatus,
  PendingImportFile,
  BackupMetadata,
  CompactResult,
  NumberFormat,
  TransactionSummary,
  BalanceSnapshotPreview,
  BackfillExecuteResult,
} from "./settings";

// Toast notifications
export { toast, showToast, dismissToast, dismissAllToasts } from "./toast.svelte";
export type { Toast, ToastOptions, ToastType } from "./toast.svelte";

// Activity tracking (for status bar)
export { activityStore, withActivity } from "./activity.svelte";
export type { Activity } from "./activity.svelte";

// Plugin updates tracking (for settings badge)
export { pluginUpdatesStore } from "./pluginUpdates.svelte";
export type { PluginUpdateInfo } from "./pluginUpdates.svelte";

// Platform utilities
export { isMac, modKey, formatShortcut } from "./platform";

// Logging (for troubleshooting)
export { logger, logPage, logAction, logError, getLogsPath } from "./logging";

// Currency utilities
export {
  SUPPORTED_CURRENCIES,
  DEFAULT_CURRENCY,
  getCurrencySymbol,
  formatCurrency,
  formatCurrencyCompact,
  formatAmount,
} from "./public";
