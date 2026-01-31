/**
 * Theme System
 *
 * CSS variable-based theming loaded from JSON files in ~/.treeline/themes/
 * Users can add custom themes by dropping JSON files in that directory.
 */

import { invoke } from "@tauri-apps/api/core";
import type { ThemeInterface } from "./types";
import { getAppSetting, setAppSetting } from "./settings";

// ============================================================================
// Theme Definitions
// ============================================================================

export interface ThemeDefinition {
  id: string;
  name: string;
  extends?: string;
  variables: Record<string, string>;
}

// Fallback theme variables in case themes can't be loaded
const FALLBACK_VARIABLES: Record<string, string> = {
  "--bg-primary": "#0d1117",
  "--bg-secondary": "#161b22",
  "--bg-tertiary": "#21262d",
  "--bg-hover": "#30363d",
  "--bg-active": "#388bfd22",
  "--border-primary": "#30363d",
  "--border-secondary": "#21262d",
  "--border-focus": "#58a6ff",
  "--text-primary": "#e6edf3",
  "--text-secondary": "#8b949e",
  "--text-muted": "#6e7681",
  "--text-link": "#58a6ff",
  "--accent-primary": "#58a6ff",
  "--accent-success": "#3fb950",
  "--accent-warning": "#d29922",
  "--accent-danger": "#f85149",
  "--color-positive": "#3fb950",
  "--color-negative": "#f85149",
  "--color-income": "#3fb950",
  "--color-expense": "#f85149",
  "--sidebar-bg": "#010409",
  "--sidebar-border": "#30363d",
  "--sidebar-item-hover": "#161b22",
  "--sidebar-item-active": "#0d1117",
  "--tab-bg": "#0d1117",
  "--tab-active-bg": "#161b22",
  "--tab-border": "#30363d",
  "--statusbar-bg": "#010409",
  "--statusbar-border": "#30363d",
  "--palette-bg": "#161b22",
  "--palette-border": "#30363d",
  "--palette-item-hover": "#21262d",
  "--input-bg": "#0d1117",
  "--input-border": "#30363d",
  "--input-focus-border": "#58a6ff",
  "--code-bg": "#161b22",
  "--logo-snow": "#ffffff",
  "--shadow-sm": "0 1px 2px rgba(0, 0, 0, 0.3)",
  "--shadow-md": "0 4px 6px rgba(0, 0, 0, 0.4)",
  "--shadow-lg": "0 10px 20px rgba(0, 0, 0, 0.5)",
  "--font-mono": "'JetBrains Mono', 'Fira Code', 'SF Mono', Consolas, monospace",
  "--font-sans": "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
  "--spacing-xs": "4px",
  "--spacing-sm": "8px",
  "--spacing-md": "12px",
  "--spacing-lg": "16px",
  "--spacing-xl": "24px",
  "--radius-sm": "4px",
  "--radius-md": "6px",
  "--radius-lg": "8px",
};

// ============================================================================
// Theme Manager
// ============================================================================

class ThemeManager implements ThemeInterface {
  private themes: Map<string, ThemeDefinition> = new Map();
  private _current: string = "dark";
  private subscribers: Set<(themeId: string) => void> = new Set();
  private initialized = false;

  get current(): string {
    return this._current;
  }

  subscribe(callback: (themeId: string) => void): () => void {
    this.subscribers.add(callback);
    callback(this._current); // Call immediately with current value
    return () => this.subscribers.delete(callback);
  }

  /**
   * Resolve theme variables, handling inheritance
   */
  private resolveVariables(themeId: string, visited: Set<string> = new Set()): Record<string, string> {
    // Prevent infinite loops from circular extends
    if (visited.has(themeId)) {
      console.warn(`Circular theme inheritance detected: ${themeId}`);
      return {};
    }
    visited.add(themeId);

    const theme = this.themes.get(themeId);
    if (!theme) {
      return {};
    }

    // If theme extends another, merge with base
    if (theme.extends && this.themes.has(theme.extends)) {
      const baseVariables = this.resolveVariables(theme.extends, visited);
      return { ...baseVariables, ...theme.variables };
    }

    return theme.variables;
  }

  setTheme(themeId: string) {
    if (!this.themes.has(themeId)) {
      console.warn(`Theme not found: ${themeId}, falling back to dark`);
      themeId = "dark";
      if (!this.themes.has(themeId)) {
        // Apply fallback variables directly
        this.applyVariables(FALLBACK_VARIABLES);
        return;
      }
    }

    this._current = themeId;
    const variables = this.resolveVariables(themeId);
    this.applyVariables(variables);
    this.subscribers.forEach((cb) => cb(themeId));

    // Persist to settings
    setAppSetting("theme", themeId).catch((err) => {
      console.warn("Failed to save theme preference:", err);
    });
  }

  private applyVariables(variables: Record<string, string>) {
    const root = document.documentElement;
    for (const [key, value] of Object.entries(variables)) {
      root.style.setProperty(key, value);
    }
  }

  getVar(name: string): string {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  }

  /**
   * Initialize theme system - loads themes from ~/.treeline/themes/
   */
  async init() {
    if (this.initialized) return;

    try {
      // Load themes from Tauri backend
      const themeList = await invoke<ThemeDefinition[]>("list_themes");
      this.themes = new Map(themeList.map((t) => [t.id, t]));
    } catch (err) {
      console.warn("Failed to load themes from backend, using fallback:", err);
      // Create a minimal dark theme from fallback
      this.themes.set("dark", {
        id: "dark",
        name: "Dark",
        variables: FALLBACK_VARIABLES,
      });
    }

    // Check for saved preference in settings
    try {
      const saved = await getAppSetting("theme");
      let themeToApply: string | null = null;

      if (saved === "system") {
        // Use system preference
        const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        themeToApply = prefersDark ? "dark" : "light";
      } else if (saved && this.themes.has(saved)) {
        // Direct theme ID
        themeToApply = saved;
      }

      if (themeToApply && this.themes.has(themeToApply)) {
        this._current = themeToApply;
        const variables = this.resolveVariables(themeToApply);
        this.applyVariables(variables);
        this.subscribers.forEach((cb) => cb(themeToApply!));
        this.initialized = true;
        return;
      }
    } catch (err) {
      console.warn("Failed to read theme from settings:", err);
    }

    // Default: use system preference
    const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    const defaultTheme = prefersDark ? "dark" : "light";
    this._current = defaultTheme;
    const variables = this.resolveVariables(defaultTheme);
    this.applyVariables(variables);
    this.subscribers.forEach((cb) => cb(defaultTheme));
    this.initialized = true;
  }

  /**
   * Get all available themes
   */
  getAvailableThemes(): { id: string; name: string }[] {
    return Array.from(this.themes.values()).map((t) => ({ id: t.id, name: t.name }));
  }

  /**
   * Reload themes from disk (useful after adding new theme files)
   */
  async reloadThemes(): Promise<void> {
    try {
      const themeList = await invoke<ThemeDefinition[]>("list_themes");
      this.themes = new Map(themeList.map((t) => [t.id, t]));

      // Re-apply current theme in case it was updated
      if (this.themes.has(this._current)) {
        const variables = this.resolveVariables(this._current);
        this.applyVariables(variables);
      }
    } catch (err) {
      console.warn("Failed to reload themes:", err);
    }
  }
}

export const themeManager = new ThemeManager();
