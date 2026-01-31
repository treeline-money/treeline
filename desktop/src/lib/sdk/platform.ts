/**
 * Platform detection utilities for keyboard shortcuts
 */

// Type for the User-Agent Client Hints API (not yet in all TS libs)
interface NavigatorUAData {
  platform: string;
}

declare global {
  interface Navigator {
    userAgentData?: NavigatorUAData;
  }
}

/**
 * Check if the current platform is macOS
 * Uses modern userAgentData API with fallback to userAgent string
 */
export function isMac(): boolean {
  if (typeof navigator === "undefined") return false;

  // Modern API (Chromium-based browsers, which Tauri uses)
  if (navigator.userAgentData?.platform) {
    return navigator.userAgentData.platform.toLowerCase() === "macos";
  }

  // Fallback to userAgent string
  return /Mac|iPhone|iPad|iPod/i.test(navigator.userAgent);
}

/**
 * Get the modifier key symbol for the current platform
 * Returns ⌘ on Mac, Ctrl+ on Windows/Linux
 * Includes trailing + for Windows so "modKey() + P" = "Ctrl+P"
 */
export function modKey(): string {
  return isMac() ? "⌘" : "Ctrl+";
}

/**
 * Format a shortcut string for the current platform
 * Converts "cmd+P" to "⌘P" on Mac or "Ctrl+P" on Windows
 */
export function formatShortcut(shortcut: string): string {
  const mac = isMac();
  return shortcut
    .replace(/cmd\+/gi, mac ? "⌘" : "Ctrl+")
    .replace(/ctrl\+/gi, "Ctrl+")
    .replace(/alt\+/gi, mac ? "⌥" : "Alt+")
    .replace(/shift\+/gi, "⇧");
}
