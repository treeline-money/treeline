-- Migration: Transaction editing support
-- Adds columns for soft delete and transaction splitting

-- ============================================================================
-- SCHEMA CHANGES
-- ============================================================================

-- Soft delete support: deleted transactions stay in DB for dedup purposes
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMP;

-- Split transaction support: children reference their parent
-- Parent transaction is soft-deleted, children are visible
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS parent_transaction_id VARCHAR;

-- Index for finding children of a split transaction
CREATE INDEX IF NOT EXISTS idx_sys_transactions_parent_id ON sys_transactions(parent_transaction_id);

-- ============================================================================
-- VIEW UPDATES
-- ============================================================================

-- Update transactions view to filter out:
-- 1. Soft-deleted transactions (deleted_at IS NOT NULL)
-- 2. Split parent transactions (have children, so parent_transaction_id used by others)
-- We use deleted_at to hide split parents, so just filtering on deleted_at is sufficient
CREATE OR REPLACE VIEW transactions AS
SELECT
    t.transaction_id,
    t.account_id,
    t.amount,
    t.description,
    t.transaction_date,
    t.posted_date,
    t.tags,
    t.parent_transaction_id,
    -- Account details
    a.name AS account_name,
    a.account_type,
    a.currency,
    a.institution_name
FROM sys_transactions t
LEFT JOIN sys_accounts a ON t.account_id = a.account_id
WHERE t.deleted_at IS NULL;
