-- Provider-specific columns for transactions and accounts
-- Stores ALL raw provider fields + explicit columns for dedup
-- Note: No transaction wrapper needed - migration runner handles transactions

-- =============================================================================
-- TRANSACTIONS: Add columns
-- =============================================================================

-- CSV Import tracking
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS csv_fingerprint VARCHAR;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS csv_batch_id VARCHAR;

-- Manual flag
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS is_manual BOOLEAN DEFAULT FALSE;

-- SimpleFIN: ALL fields from API (https://www.simplefin.org/protocol.html)
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS sf_id VARCHAR;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS sf_posted BIGINT;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS sf_amount VARCHAR;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS sf_description VARCHAR;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS sf_transacted_at BIGINT;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS sf_pending BOOLEAN;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS sf_extra JSON;

-- Lunchflow: ALL fields from API
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS lf_id VARCHAR;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS lf_account_id VARCHAR;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS lf_amount DECIMAL(15,2);
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS lf_currency VARCHAR;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS lf_date DATE;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS lf_merchant VARCHAR;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS lf_description VARCHAR;
ALTER TABLE sys_transactions ADD COLUMN IF NOT EXISTS lf_is_pending BOOLEAN;

-- =============================================================================
-- TRANSACTIONS: Backfill IDs from external_ids (for dedup continuity)
-- Note: Raw provider fields cannot be backfilled - they weren't stored before
-- =============================================================================

UPDATE sys_transactions
SET
    sf_id = json_extract_string(external_ids, '$.simplefin'),
    lf_id = json_extract_string(external_ids, '$.lunchflow'),
    csv_fingerprint = CASE
        WHEN json_extract_string(external_ids, '$."csv_import.batch_id"') IS NOT NULL
        THEN json_extract_string(external_ids, '$.fingerprint')
        ELSE NULL
    END,
    csv_batch_id = json_extract_string(external_ids, '$."csv_import.batch_id"'),
    is_manual = CASE
        WHEN json_extract_string(external_ids, '$.manual') = 'true' THEN TRUE
        ELSE FALSE
    END
WHERE external_ids IS NOT NULL AND external_ids != '{}';

-- =============================================================================
-- TRANSACTIONS: Add indexes for dedup performance
-- =============================================================================

CREATE INDEX IF NOT EXISTS idx_sys_transactions_sf_id ON sys_transactions(sf_id);
CREATE INDEX IF NOT EXISTS idx_sys_transactions_lf_id ON sys_transactions(lf_id);
CREATE INDEX IF NOT EXISTS idx_sys_transactions_csv_fingerprint ON sys_transactions(csv_fingerprint);

-- =============================================================================
-- ACCOUNTS: Add columns
-- =============================================================================

-- Manual flag
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS is_manual BOOLEAN DEFAULT FALSE;

-- SimpleFIN: ALL fields from API
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_id VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_name VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_currency VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_balance VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_available_balance VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_balance_date BIGINT;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_org_name VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_org_url VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_org_domain VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS sf_extra JSON;

-- Lunchflow: ALL fields from API
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS lf_id VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS lf_name VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS lf_institution_name VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS lf_institution_logo VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS lf_provider VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS lf_currency VARCHAR;
ALTER TABLE sys_accounts ADD COLUMN IF NOT EXISTS lf_status VARCHAR;

-- =============================================================================
-- ACCOUNTS: Backfill IDs from external_ids (for mapping continuity)
-- =============================================================================

UPDATE sys_accounts
SET
    sf_id = json_extract_string(external_ids, '$.simplefin'),
    lf_id = json_extract_string(external_ids, '$.lunchflow')
WHERE external_ids IS NOT NULL AND external_ids != '{}';

-- =============================================================================
-- UPDATE VIEWS
-- =============================================================================

CREATE OR REPLACE VIEW transactions AS
SELECT
    -- Core fields (pass-through, already mapped by adapters)
    t.transaction_id,
    t.account_id,
    t.amount,
    t.description,
    t.transaction_date,
    t.posted_date,
    t.tags,
    t.parent_transaction_id,

    -- Computed: source identification
    -- Note: Demo mode uses its own database, so no 'demo' case needed here
    CASE
        WHEN t.sf_id IS NOT NULL THEN 'simplefin'
        WHEN t.lf_id IS NOT NULL THEN 'lunchflow'
        WHEN t.csv_batch_id IS NOT NULL THEN 'csv_import'
        WHEN t.parent_transaction_id IS NOT NULL THEN 'split'
        WHEN t.is_manual THEN 'manual'
        ELSE 'unknown'
    END AS source,

    -- Account info (joined)
    a.name AS account_name,
    a.account_type,
    a.currency,
    a.institution_name
FROM sys_transactions t
LEFT JOIN sys_accounts a ON t.account_id = a.account_id
WHERE t.deleted_at IS NULL;
