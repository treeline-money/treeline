-- Migration 013: Refresh accounts view after schema changes
-- The accounts view uses SELECT * FROM sys_accounts, but DuckDB caches the column list.
-- After adding new columns (is_manual, sf_*, lf_*) in migration 012, we need to recreate the view.

DROP VIEW IF EXISTS accounts;

CREATE VIEW accounts AS
SELECT * FROM sys_accounts;
