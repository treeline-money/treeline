/**
 * Tagging Plugin Types
 *
 * Centralized type definitions for the tagging/transactions plugin.
 */

// Re-export from suggestions (Transaction is defined there)
export type { Transaction, TagSuggestion, TagSuggester } from "./suggestions";

/**
 * Split amount entry for the split transaction modal.
 */
export interface SplitAmount {
  description: string;
  amount: string;
}

/**
 * Account info for dropdowns and filters.
 */
export interface AccountInfo {
  id: string;
  name: string;
}
