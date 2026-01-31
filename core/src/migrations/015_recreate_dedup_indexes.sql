-- Recreate dedup indexes to fix potential corruption
--
-- Migration 012 created these indexes before the WAL checkpoint fix was added.
-- On some systems, the indexes may have become corrupted during WAL replay,
-- causing dedup checks (WHERE sf_id = ?) to miss existing transactions.
--
-- This migration drops and recreates the indexes to ensure they're consistent.
-- This is safe: DROP IF EXISTS + CREATE rebuilds the index from table data.

-- Transaction dedup indexes
DROP INDEX IF EXISTS idx_sys_transactions_sf_id;
CREATE INDEX idx_sys_transactions_sf_id ON sys_transactions(sf_id);

DROP INDEX IF EXISTS idx_sys_transactions_lf_id;
CREATE INDEX idx_sys_transactions_lf_id ON sys_transactions(lf_id);

DROP INDEX IF EXISTS idx_sys_transactions_csv_fingerprint;
CREATE INDEX idx_sys_transactions_csv_fingerprint ON sys_transactions(csv_fingerprint);

-- Note: If you have duplicate transactions from this bug, you can find them with:
--   SELECT sf_id, COUNT(*) FROM sys_transactions
--   WHERE sf_id IS NOT NULL GROUP BY sf_id HAVING COUNT(*) > 1;
--
-- And remove the newer duplicates (keeping originals with user edits) with:
--   DELETE FROM sys_transactions WHERE transaction_id IN (
--     SELECT t2.transaction_id FROM sys_transactions t1
--     JOIN sys_transactions t2 ON t1.sf_id = t2.sf_id
--     WHERE t1.sf_id IS NOT NULL AND t1.created_at < t2.created_at
--   );
