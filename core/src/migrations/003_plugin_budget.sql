-- Migration: Budget plugin data tables
-- Moves budget configuration from JSON files to DuckDB for:
-- 1. Single-file backup (just copy the .duckdb file)
-- 2. SQL queryability (power users can query budget history)
-- 3. Data integrity

-- ============================================================================
-- BUDGET CATEGORIES (the "template")
-- ============================================================================

-- One row per budget category. This is the master definition.
CREATE TABLE IF NOT EXISTS sys_plugin_budget_categories (
    category_id TEXT PRIMARY KEY,             -- 'income-Paychecks' or 'expense-Food'
    type TEXT NOT NULL CHECK (type IN ('income', 'expense')),
    name TEXT NOT NULL,                       -- 'Paychecks', 'Food', etc.
    expected DECIMAL(12,2) NOT NULL DEFAULT 0,
    tags TEXT[] NOT NULL DEFAULT [],          -- Array of tag names
    require_all BOOLEAN NOT NULL DEFAULT FALSE,
    amount_sign TEXT CHECK (amount_sign IN ('positive', 'negative', NULL)),
    sort_order INTEGER NOT NULL DEFAULT 0,    -- For ordering in UI
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- BUDGET MONTHS (monthly overrides)
-- ============================================================================

-- One row per category per month when there's an override
CREATE TABLE IF NOT EXISTS sys_plugin_budget_months (
    budget_month_id TEXT PRIMARY KEY,         -- 'income-Paychecks:2025-12'
    category_id TEXT NOT NULL,                -- FK to sys_plugin_budget_categories.category_id
    month TEXT NOT NULL,                      -- '2025-12'
    expected_override DECIMAL(12,2),          -- NULL = use template, non-NULL = override
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(category_id, month)
);

-- ============================================================================
-- BUDGET ROLLOVERS
-- ============================================================================

-- Rollovers between categories/months (e.g., leftover Dec Food -> Jan Food)
CREATE TABLE IF NOT EXISTS sys_plugin_budget_rollovers (
    rollover_id TEXT PRIMARY KEY,
    source_month TEXT NOT NULL,               -- Month the rollover is FROM (where it's stored)
    from_category TEXT NOT NULL,              -- Category name (not id, for flexibility)
    to_category TEXT NOT NULL,                -- Category name
    to_month TEXT NOT NULL,                   -- Usually source_month or next month
    amount DECIMAL(12,2) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- INDEXES
-- ============================================================================

CREATE INDEX IF NOT EXISTS idx_budget_categories_type ON sys_plugin_budget_categories(type);
CREATE INDEX IF NOT EXISTS idx_budget_months_month ON sys_plugin_budget_months(month);
CREATE INDEX IF NOT EXISTS idx_budget_months_category ON sys_plugin_budget_months(category_id);
CREATE INDEX IF NOT EXISTS idx_budget_rollovers_source ON sys_plugin_budget_rollovers(source_month);
CREATE INDEX IF NOT EXISTS idx_budget_rollovers_target ON sys_plugin_budget_rollovers(to_month);

-- ============================================================================
-- CONVENIENCE VIEW
-- ============================================================================

-- View that joins categories with their monthly overrides for easy querying
CREATE OR REPLACE VIEW budget_categories AS
SELECT
    c.category_id,
    c.type,
    c.name,
    c.expected AS template_expected,
    c.tags,
    c.require_all,
    c.amount_sign,
    c.sort_order
FROM sys_plugin_budget_categories c;
