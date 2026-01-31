-- Simplify tag rules schema
--
-- Changes:
-- - sql_condition is now NOT NULL and is the canonical source for matching
-- - conditions + condition_logic merged into display_conditions (UI-only, optional)
-- - Drop and recreate to simplify (no users with production data yet)

DROP TABLE IF EXISTS sys_plugin_transactions_rules;

CREATE TABLE sys_plugin_transactions_rules (
    rule_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    sql_condition TEXT NOT NULL,        -- Canonical SQL WHERE clause, always used for matching
    display_conditions TEXT,            -- Optional JSON for UI rendering: {"logic": "all", "conditions": [...]}
    tags TEXT[] NOT NULL DEFAULT [],
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP DEFAULT now(),
    updated_at TIMESTAMP DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_transactions_rules_enabled ON sys_plugin_transactions_rules(enabled);
