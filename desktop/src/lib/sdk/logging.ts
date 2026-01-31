/**
 * Logging Service - Structured event logging for troubleshooting
 *
 * Privacy: NEVER log user data (transactions, accounts, balances, descriptions)
 * Only log: event names, view names, component names, sanitized error messages
 */

import { invoke } from "@tauri-apps/api/core";

/**
 * Log a page/view navigation
 * @param page - View ID (e.g., "accounts", "transactions", "budget")
 */
export async function logPage(page: string): Promise<void> {
  try {
    await invoke("log_page", { page });
  } catch {
    // Silently ignore logging errors - should never break the app
  }
}

/**
 * Log a user action
 * @param action - Action name (e.g., "sync_clicked", "settings_opened")
 * @param component - Component name (e.g., "sidebar", "statusbar", "modal")
 */
export async function logAction(action: string, component: string): Promise<void> {
  try {
    await invoke("log_action", { action, component });
  } catch {
    // Silently ignore logging errors
  }
}

/**
 * Log an error event
 * @param event - Error event name (e.g., "sync_error", "query_error")
 * @param message - Error message (will be sanitized)
 * @param details - Optional additional context
 */
export async function logError(event: string, message: string, details?: string): Promise<void> {
  try {
    // Sanitize message - remove any potential PII patterns
    const sanitizedMessage = sanitizeErrorMessage(message);
    await invoke("log_error", { event, message: sanitizedMessage, details });
  } catch {
    // Silently ignore logging errors
  }
}

/**
 * Get the path to logs.duckdb for support purposes
 */
export async function getLogsPath(): Promise<string | null> {
  try {
    return await invoke<string | null>("get_logs_path");
  } catch {
    return null;
  }
}

/**
 * Sanitize error messages to remove potential PII
 * Removes: account numbers, amounts, descriptions, paths with usernames
 */
function sanitizeErrorMessage(message: string): string {
  return (
    message
      // Remove potential account numbers (8+ digits)
      .replace(/\b\d{8,}\b/g, "[REDACTED]")
      // Remove dollar amounts
      .replace(/\$[\d,]+\.?\d*/g, "[AMOUNT]")
      // Remove home directory paths (macOS)
      .replace(/\/Users\/[^/\s]+/g, "~")
      // Remove home directory paths (Windows)
      .replace(/C:\\Users\\[^\\]+/g, "~")
      // Limit length
      .slice(0, 500)
  );
}

// Convenience object for consistent API
export const logger = {
  page: logPage,
  action: logAction,
  error: logError,
  getPath: getLogsPath,
};
