-- Migration: Plugin accounts schema
--
-- Moves accounts overrides from sys_accounts_overrides to plugin_accounts.overrides

-- Create plugin_accounts schema
CREATE SCHEMA IF NOT EXISTS plugin_accounts;

-- Create new table
CREATE TABLE IF NOT EXISTS plugin_accounts.overrides (
    account_id VARCHAR PRIMARY KEY,
    classification_override VARCHAR,
    exclude_from_net_worth BOOLEAN DEFAULT FALSE
);

-- Copy data from old table if it exists
INSERT INTO plugin_accounts.overrides (account_id, classification_override, exclude_from_net_worth)
SELECT account_id, classification_override, COALESCE(exclude_from_net_worth, FALSE)
FROM sys_accounts_overrides
WHERE NOT EXISTS (
    SELECT 1 FROM plugin_accounts.overrides po
    WHERE po.account_id = sys_accounts_overrides.account_id
);

-- Drop old table
DROP TABLE IF EXISTS sys_accounts_overrides;
