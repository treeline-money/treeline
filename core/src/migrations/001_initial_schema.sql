-- Initial schema for Treeline Money
-- System tables (sys_*) contain all data with technical details
-- User-facing views provide convenient access with joins

-- ============================================================================
-- SYSTEM TABLES (sys_*)
-- ============================================================================

CREATE TABLE IF NOT EXISTS sys_accounts (
    account_id VARCHAR PRIMARY KEY,
    name VARCHAR NOT NULL,
    nickname VARCHAR,
    account_type VARCHAR,
    currency VARCHAR NOT NULL DEFAULT 'USD',
    balance DECIMAL(15,2),
    external_ids JSON DEFAULT '{}',
    institution_name VARCHAR,
    institution_url VARCHAR,
    institution_domain VARCHAR,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sys_transactions (
    transaction_id VARCHAR PRIMARY KEY,
    account_id VARCHAR NOT NULL,
    amount DECIMAL(15,2) NOT NULL,
    description VARCHAR,
    transaction_date DATE NOT NULL,
    posted_date DATE NOT NULL,
    tags VARCHAR[],
    external_ids JSON DEFAULT '{}',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES sys_accounts(account_id)
);

CREATE TABLE IF NOT EXISTS sys_balance_snapshots (
    snapshot_id VARCHAR PRIMARY KEY,
    account_id VARCHAR NOT NULL,
    balance DECIMAL(15,2) NOT NULL,
    snapshot_time TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES sys_accounts(account_id)
);

CREATE TABLE IF NOT EXISTS sys_integrations (
    integration_name VARCHAR PRIMARY KEY,
    integration_settings JSON NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- INDEXES
-- ============================================================================

CREATE INDEX IF NOT EXISTS idx_sys_transactions_account_id ON sys_transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_sys_transactions_date ON sys_transactions(transaction_date);
CREATE INDEX IF NOT EXISTS idx_sys_balance_snapshots_account_id ON sys_balance_snapshots(account_id);
CREATE INDEX IF NOT EXISTS idx_sys_balance_snapshots_time ON sys_balance_snapshots(snapshot_time);

-- ============================================================================
-- USER-FRIENDLY VIEWS
-- ============================================================================

-- Transactions view with account details joined
CREATE OR REPLACE VIEW transactions AS
SELECT
    t.transaction_id,
    t.account_id,
    t.amount,
    t.description,
    t.transaction_date,
    t.posted_date,
    t.tags,
    -- Account details
    a.name AS account_name,
    a.account_type,
    a.currency,
    a.institution_name
FROM sys_transactions t
LEFT JOIN sys_accounts a ON t.account_id = a.account_id;

-- Accounts view (wrapper for consistency)
CREATE OR REPLACE VIEW accounts AS
SELECT * FROM sys_accounts;

-- Balance snapshots view with account details joined
CREATE OR REPLACE VIEW balance_snapshots AS
SELECT
    s.snapshot_id,
    s.account_id,
    s.balance,
    s.snapshot_time,
    s.created_at,
    s.updated_at,
    -- Account details
    a.name AS account_name,
    a.institution_name
FROM sys_balance_snapshots s
LEFT JOIN sys_accounts a ON s.account_id = a.account_id;
