-- Migration: Accounts and Query plugin data tables

-- ============================================================================
-- ACCOUNTS PLUGIN - Account overrides
-- ============================================================================

CREATE TABLE IF NOT EXISTS sys_plugin_accounts_overrides (
    account_id TEXT PRIMARY KEY,
    classification_override TEXT CHECK (classification_override IN ('asset', 'liability', NULL)),
    exclude_from_net_worth BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT now(),
    updated_at TIMESTAMP DEFAULT now()
);

-- ============================================================================
-- QUERY PLUGIN - Query history and saved queries
-- ============================================================================

CREATE SEQUENCE IF NOT EXISTS seq_query_history_id START 1;

CREATE TABLE IF NOT EXISTS sys_plugin_query_history (
    history_id INTEGER PRIMARY KEY DEFAULT nextval('seq_query_history_id'),
    query TEXT NOT NULL,
    success BOOLEAN NOT NULL DEFAULT TRUE,
    executed_at TIMESTAMP DEFAULT now()
);

CREATE TABLE IF NOT EXISTS sys_plugin_query_saved (
    saved_query_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    query TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMP DEFAULT now(),
    updated_at TIMESTAMP DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_query_history_executed ON sys_plugin_query_history(executed_at DESC);
