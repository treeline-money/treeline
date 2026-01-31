-- Migration tracking table
-- This must be the first migration (000) to run before all others
CREATE TABLE IF NOT EXISTS sys_migrations (
    migration_name VARCHAR PRIMARY KEY,
    applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
