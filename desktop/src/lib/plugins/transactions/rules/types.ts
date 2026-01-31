/**
 * Auto-Tag Rules Types
 *
 * Rules are stored in DuckDB table sys_transactions_rules
 */

/**
 * Condition types for rule matching (used in UI builder)
 */
export type ConditionOperator =
  | "contains"      // description contains string (case-insensitive)
  | "starts_with"   // description starts with string
  | "ends_with"     // description ends with string
  | "equals"        // exact match
  | "regex"         // regex pattern match
  | "greater_than"  // amount > value
  | "less_than"     // amount < value
  | "between";      // amount between min and max

export interface RuleCondition {
  field: "description" | "amount" | "account";
  operator: ConditionOperator;
  value: string | number;
  value2?: number; // For "between" operator
}

/**
 * Display conditions - stored as JSON for UI rendering only
 * The actual matching is done via sqlCondition
 */
export interface DisplayConditions {
  logic: "all" | "any"; // AND vs OR
  conditions: RuleCondition[];
}

/**
 * A single auto-tag rule
 *
 * The rule's matching logic is defined by `sqlCondition` (a raw SQL WHERE clause).
 * `displayConditions` is optional and used only for UI rendering in the condition builder.
 */
export interface TagRule {
  id: string;
  name: string;
  // SQL WHERE clause - the canonical source for matching (always required)
  sqlCondition: string;
  // Optional JSON for UI rendering - if not present, show raw SQL editor
  displayConditions?: DisplayConditions;
  tags: string[];
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
  // Stats (optional, for display)
  matchCount?: number;
}

/**
 * Plugin settings shape for transactions plugin
 */
export interface TransactionsPluginSettings {
  rules: TagRule[];
  [key: string]: unknown; // Allow indexing for Record<string, unknown> constraint
}

/**
 * Default settings
 */
export const DEFAULT_TRANSACTIONS_SETTINGS: TransactionsPluginSettings = {
  rules: [],
};

/**
 * Result of testing a rule against transactions
 */
export interface RuleTestResult {
  matchingCount: number;
  sampleMatches: Array<{
    transaction_id: string;
    description: string;
    amount: number;
    account_name: string;
    transaction_date: string;
    tags: string[];
  }>;
}
