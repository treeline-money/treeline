-- Migration: Transactions plugin - auto-tag rules
--
-- Moves auto-tagging rules from settings.json to DuckDB.
-- Rules can use SQL conditions or structured conditions.

CREATE TABLE IF NOT EXISTS sys_plugin_transactions_rules (
    rule_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    sql_condition TEXT,                       -- Raw SQL WHERE clause (power user mode)
    conditions TEXT,                          -- JSON array of condition objects (UI builder mode)
    condition_logic TEXT NOT NULL DEFAULT 'all' CHECK (condition_logic IN ('all', 'any')),
    tags TEXT[] NOT NULL DEFAULT [],
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP DEFAULT now(),
    updated_at TIMESTAMP DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_transactions_rules_enabled ON sys_plugin_transactions_rules(enabled);
