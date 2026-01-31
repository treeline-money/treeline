/**
 * Accounts Plugin Types
 */

import { executeQuery } from "../../sdk";

export type BalanceClassification = "asset" | "liability";

export interface Account {
  account_id: string;
  name: string;
  nickname: string | null;
  account_type: string | null;
  currency: string;
  balance: number | null;
  institution_name: string | null;
  created_at: string;
  updated_at: string;
}

export interface AccountWithStats extends Account {
  transaction_count: number;
  first_transaction: string | null;
  last_transaction: string | null;
  computed_balance: number;
  balance_as_of: string | null;
  classification: BalanceClassification;
  isManual: boolean;
}

export interface BalanceSnapshot {
  snapshot_id: string;
  account_id: string;
  balance: number;
  snapshot_time: string;
}

export interface BalanceTrendPoint {
  month: string;
  day: number;
  balance: number;
  snapshot_time: string;
}

export interface AccountsConfig {
  // Override balance classification for accounts (account_id -> classification)
  classificationOverrides: Record<string, BalanceClassification>;
  // Accounts excluded from net worth calculation
  excludedFromNetWorth: string[];
}

// Default asset/liability mapping based on account_type
export function getDefaultClassification(accountType: string | null): BalanceClassification {
  if (!accountType) return "asset";
  const liabilityTypes = ["credit", "loan"];
  return liabilityTypes.includes(accountType.toLowerCase()) ? "liability" : "asset";
}

/**
 * Basic account info with classification (for account selectors, etc.)
 */
export interface AccountBasicInfo {
  id: string;
  name: string;
  institution_name: string | null;
  account_type: string | null;
  classification: BalanceClassification;
}

/**
 * Load accounts with their classification.
 * Classification is stored directly in sys_accounts.
 */
export async function loadAccountsWithClassification(): Promise<AccountBasicInfo[]> {
  const res = await executeQuery(`
    SELECT account_id, name, institution_name, account_type, classification
    FROM sys_accounts
    ORDER BY name
  `);

  return res.rows.map(row => ({
    id: row[0] as string,
    name: row[1] as string,
    institution_name: row[2] as string | null,
    account_type: row[3] as string | null,
    classification: (row[4] as BalanceClassification) || "asset",
  }));
}
