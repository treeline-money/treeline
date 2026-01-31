/**
 * Tag Suggestion Types
 *
 * Defines the interface for tag suggestion strategies.
 * Implementations can use different algorithms (frequency-based, ML, rules, etc.)
 */

export interface Transaction {
  transaction_id: string;
  account_id: string;
  description: string;
  amount: number;
  tags: string[];
  account_name?: string;
  account_nickname?: string | null;
  transaction_date: string;
  parent_transaction_id?: string | null;
  /** True if any tags on this transaction were applied by auto-tag rules */
  tags_auto_applied?: boolean;
}

export interface TagSuggestion {
  tag: string;
  score: number;
  reason?: string;
}

/**
 * Interface for tag suggestion strategies.
 * Implementations should be stateless and compute suggestions based on
 * the provided transactions and database state.
 */
export interface TagSuggester {
  /**
   * Generate tag suggestions for multiple transactions in a single batch.
   * This is the primary method - batching is preferred for performance.
   *
   * @param transactions List of transactions to generate suggestions for
   * @param limit Maximum number of suggestions per transaction
   * @returns Map of transaction_id to list of suggestions
   */
  suggestBatch(
    transactions: Transaction[],
    limit: number
  ): Promise<Map<string, TagSuggestion[]>>;
}
