-- Migration: Plugin schema isolation
--
-- Organizes plugin tables into DuckDB schemas for better isolation.
-- - Core plugin tables (affecting data input) stay in main schema with simpler names
-- - Community plugin tables move to dedicated schemas

-- ============================================================================
-- CORE PLUGIN TABLES - Rename (stay in main schema)
-- ============================================================================

-- Transactions plugin - auto-tag rules
-- Must drop index first, then rename table, then recreate index
DROP INDEX IF EXISTS idx_transactions_rules_enabled;
ALTER TABLE sys_plugin_transactions_rules RENAME TO sys_transactions_rules;
CREATE INDEX idx_transactions_rules_enabled ON sys_transactions_rules(enabled);

-- Accounts plugin - classification overrides (no indexes to drop)
ALTER TABLE sys_plugin_accounts_overrides RENAME TO sys_accounts_overrides;

-- ============================================================================
-- QUERY PLUGIN SCHEMA
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS plugin_query;

-- Drop index before migrating
DROP INDEX IF EXISTS idx_query_history_executed;

-- Create new table and copy data
CREATE SEQUENCE IF NOT EXISTS plugin_query.seq_history_id START 1;
CREATE TABLE plugin_query.history (
    history_id INTEGER PRIMARY KEY DEFAULT nextval('plugin_query.seq_history_id'),
    query TEXT NOT NULL,
    success BOOLEAN NOT NULL DEFAULT TRUE,
    executed_at TIMESTAMP DEFAULT now()
);
INSERT INTO plugin_query.history (history_id, query, success, executed_at)
SELECT history_id, query, success, executed_at FROM sys_plugin_query_history;
DROP TABLE sys_plugin_query_history;
DROP SEQUENCE IF EXISTS seq_query_history_id;

CREATE INDEX idx_plugin_query_history_executed ON plugin_query.history(executed_at DESC);

-- Saved queries (no indexes)
CREATE TABLE plugin_query.saved (
    saved_query_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    query TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMP DEFAULT now(),
    updated_at TIMESTAMP DEFAULT now()
);
INSERT INTO plugin_query.saved SELECT * FROM sys_plugin_query_saved;
DROP TABLE sys_plugin_query_saved;

-- ============================================================================
-- BUDGET PLUGIN SCHEMA
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS plugin_budget;

-- Drop the convenience view first (depends on the table)
DROP VIEW IF EXISTS budget_categories;

-- Drop indexes before migrating
DROP INDEX IF EXISTS idx_budget_categories_month;
DROP INDEX IF EXISTS idx_budget_categories_month_type;
DROP INDEX IF EXISTS idx_budget_rollovers_source;
DROP INDEX IF EXISTS idx_budget_rollovers_target;

-- Migrate budget categories
CREATE TABLE plugin_budget.categories (
    category_id TEXT PRIMARY KEY,
    month TEXT NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('income', 'expense')),
    name TEXT NOT NULL,
    expected DECIMAL(12,2) NOT NULL DEFAULT 0,
    tags TEXT[] NOT NULL DEFAULT [],
    require_all BOOLEAN NOT NULL DEFAULT FALSE,
    amount_sign TEXT CHECK (amount_sign IN ('positive', 'negative', NULL)),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP DEFAULT now(),
    updated_at TIMESTAMP DEFAULT now()
);
INSERT INTO plugin_budget.categories SELECT * FROM sys_plugin_budget_categories;
DROP TABLE sys_plugin_budget_categories;

CREATE INDEX idx_plugin_budget_categories_month ON plugin_budget.categories(month);
CREATE INDEX idx_plugin_budget_categories_month_type ON plugin_budget.categories(month, type);

-- Migrate budget rollovers
CREATE TABLE plugin_budget.rollovers (
    rollover_id TEXT PRIMARY KEY,
    source_month TEXT NOT NULL,
    from_category TEXT NOT NULL,
    to_category TEXT NOT NULL,
    to_month TEXT NOT NULL,
    amount DECIMAL(12,2) NOT NULL,
    created_at TIMESTAMP DEFAULT now()
);
INSERT INTO plugin_budget.rollovers SELECT * FROM sys_plugin_budget_rollovers;
DROP TABLE sys_plugin_budget_rollovers;

CREATE INDEX idx_plugin_budget_rollovers_source ON plugin_budget.rollovers(source_month);
CREATE INDEX idx_plugin_budget_rollovers_target ON plugin_budget.rollovers(to_month);

-- ============================================================================
-- COMMUNITY PLUGIN SCHEMAS (empty - plugins migrate themselves on first load)
-- ============================================================================

-- These schemas are created by the plugins themselves when they first run.
-- Not all users have these plugins installed, so we don't migrate data here.
--
-- Plugins that will self-migrate:
-- - plugin_goals: goals (from sys_plugin_goals)
-- - plugin_subscriptions: subscriptions (from sys_plugin_subscriptions)
-- - plugin_cashflow: scheduled (from sys_plugin_cashflow_items)
-- - plugin_emergency_fund: config, snapshots (from sys_plugin_emergency_fund_*)
