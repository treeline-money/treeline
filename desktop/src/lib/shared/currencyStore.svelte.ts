/**
 * Reactive currency store for the UI.
 *
 * This store holds the user's currency preference and provides
 * reactive access for components to format amounts correctly.
 */

import { getSettings } from "../sdk/settings";
import {
  DEFAULT_CURRENCY,
  formatCurrency as formatCurrencyBase,
  formatCurrencyCompact as formatCurrencyCompactBase,
  getCurrencySymbol as getCurrencySymbolBase,
} from "./currency";

// Reactive state for current currency
let currentCurrency = $state<string>(DEFAULT_CURRENCY);

/**
 * Get the current currency preference.
 */
export function getCurrency(): string {
  return currentCurrency;
}

/**
 * Set the current currency (called when settings change).
 */
export function setCurrency(currency: string): void {
  currentCurrency = currency;
}

/**
 * Load currency from settings. Call this on app startup.
 */
export async function loadCurrency(): Promise<void> {
  try {
    const settings = await getSettings();
    currentCurrency = settings?.app?.currency || DEFAULT_CURRENCY;
  } catch (e) {
    console.error("Failed to load currency preference:", e);
    currentCurrency = DEFAULT_CURRENCY;
  }
}

/**
 * Reactive currency value for use in components.
 * Use this with $derived or in templates.
 */
export function useCurrency(): string {
  return currentCurrency;
}

/**
 * Format amount using the user's currency preference.
 * This is the preferred function for UI components.
 */
export function formatUserCurrency(
  amount: number,
  options?: { minimumFractionDigits?: number; maximumFractionDigits?: number }
): string {
  return formatCurrencyBase(amount, currentCurrency, options);
}

/**
 * Format amount compactly using the user's currency preference.
 */
export function formatUserCurrencyCompact(amount: number): string {
  return formatCurrencyCompactBase(amount, currentCurrency);
}

/**
 * Get the symbol for the user's current currency.
 */
export function getUserCurrencySymbol(): string {
  return getCurrencySymbolBase(currentCurrency);
}
