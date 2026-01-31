-- Migration: Add classification column to sys_accounts
--
-- Moves classification from plugin_accounts.overrides to sys_accounts directly.
-- This simplifies queries and makes the data model honest about what we require.
--
-- Classification values: 'asset' or 'liability'
-- Uses Plaid nomenclature for account types:
--   - depository, investment, other → asset
--   - credit, loan → liability

-- Add classification column with default 'asset'
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS classification VARCHAR DEFAULT 'asset';

-- Migrate existing data: apply any overrides from plugin table, or compute default from account_type
-- Uses COALESCE to check override first, then compute from account_type
UPDATE sys_accounts SET classification = COALESCE(
  -- First, try to get override from plugin_accounts.overrides if that table exists
  (SELECT classification_override
   FROM plugin_accounts.overrides
   WHERE account_id = sys_accounts.account_id),
  -- If no override, compute default based on account_type (Plaid nomenclature)
  -- credit and loan are liabilities, everything else is an asset
  CASE
    WHEN LOWER(COALESCE(account_type, '')) IN ('credit', 'loan') THEN 'liability'
    ELSE 'asset'
  END
);
