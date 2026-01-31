-- Add source column to track where balance snapshots came from
-- Values: 'sync', 'manual', 'backfill', or NULL for legacy data

ALTER TABLE sys_balance_snapshots ADD COLUMN IF NOT EXISTS source VARCHAR;

-- Update the balance_snapshots view to include source
CREATE OR REPLACE VIEW balance_snapshots AS
SELECT
    s.snapshot_id,
    s.account_id,
    s.balance,
    s.snapshot_time,
    s.source,
    s.created_at,
    s.updated_at,
    -- Account details
    a.name AS account_name,
    a.institution_name
FROM sys_balance_snapshots s
LEFT JOIN sys_accounts a ON s.account_id = a.account_id;
