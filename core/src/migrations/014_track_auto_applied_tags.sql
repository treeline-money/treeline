-- Migration: Track auto-applied tags
-- Adds a boolean flag to track when tags were applied by auto-tag rules

ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS tags_auto_applied BOOLEAN DEFAULT FALSE;

-- All existing transactions with tags are either manually tagged or were
-- auto-tagged before this tracking - we mark them as manually tagged (false)
-- since we can't determine the origin retroactively

-- Update the transactions view to include the new field
CREATE OR REPLACE VIEW transactions AS
SELECT
    -- Core fields (pass-through, already mapped by adapters)
    t.transaction_id,
    t.account_id,
    t.amount,
    t.description,
    t.transaction_date,
    t.posted_date,
    t.tags,
    t.parent_transaction_id,
    t.tags_auto_applied,

    -- Computed: source identification
    -- Note: Demo mode uses its own database, so no 'demo' case needed here
    CASE
        WHEN t.sf_id IS NOT NULL THEN 'simplefin'
        WHEN t.lf_id IS NOT NULL THEN 'lunchflow'
        WHEN t.csv_batch_id IS NOT NULL THEN 'csv_import'
        WHEN t.parent_transaction_id IS NOT NULL THEN 'split'
        WHEN t.is_manual THEN 'manual'
        ELSE 'unknown'
    END AS source,

    -- Account info (joined)
    a.name AS account_name,
    a.account_type,
    a.currency,
    a.institution_name
FROM sys_transactions t
LEFT JOIN sys_accounts a ON t.account_id = a.account_id
WHERE t.deleted_at IS NULL;
