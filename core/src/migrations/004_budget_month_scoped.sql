-- Migration: Budget categories - month-scoped redesign
--
-- Changes the budget schema so each month is self-contained:
-- - Each month has its own complete set of categories (no template/override concept)
-- - Category IDs are UUIDs (not derived from name)
-- - Same category name can exist in different months with different configs
-- - Simpler ad-hoc queries (just filter by month)

-- ============================================================================
-- DROP OLD STRUCTURE
-- ============================================================================

-- Drop the old view first
DROP VIEW IF EXISTS budget_categories;

-- Drop the months override table (no longer needed)
DROP TABLE IF EXISTS sys_plugin_budget_months;

-- Drop old indexes
DROP INDEX IF EXISTS idx_budget_categories_type;

-- ============================================================================
-- RECREATE CATEGORIES TABLE WITH MONTH COLUMN
-- ============================================================================

-- Drop and recreate to add month column
DROP TABLE IF EXISTS sys_plugin_budget_categories;

CREATE TABLE sys_plugin_budget_categories (
    category_id TEXT PRIMARY KEY,             -- UUID (unique per category instance)
    month TEXT NOT NULL,                      -- '2025-12' - the month this category belongs to
    type TEXT NOT NULL CHECK (type IN ('income', 'expense')),
    name TEXT NOT NULL,                       -- 'Paychecks', 'Food', etc.
    expected DECIMAL(12,2) NOT NULL DEFAULT 0,
    tags TEXT[] NOT NULL DEFAULT [],          -- Array of tag names
    require_all BOOLEAN NOT NULL DEFAULT FALSE,
    amount_sign TEXT CHECK (amount_sign IN ('positive', 'negative', NULL)),
    sort_order INTEGER NOT NULL DEFAULT 0,    -- For ordering in UI
    created_at TIMESTAMP DEFAULT now(),
    updated_at TIMESTAMP DEFAULT now()
);

-- Index for efficient month-based queries
CREATE INDEX idx_budget_categories_month ON sys_plugin_budget_categories(month);
CREATE INDEX idx_budget_categories_month_type ON sys_plugin_budget_categories(month, type);

-- ============================================================================
-- CONVENIENCE VIEW
-- ============================================================================

-- Simple view for easy querying
CREATE OR REPLACE VIEW budget_categories AS
SELECT
    category_id,
    month,
    type,
    name,
    expected,
    tags,
    require_all,
    amount_sign,
    sort_order
FROM sys_plugin_budget_categories;
