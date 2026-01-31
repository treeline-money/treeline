//! Sync service - synchronize accounts and transactions from integrations

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use chrono::{Duration, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::adapters::demo::DemoDataProvider;
use crate::adapters::duckdb::DuckDbRepository;
use crate::adapters::lunchflow::LunchflowProvider;
use crate::adapters::simplefin::SimpleFINProvider;
use crate::ports::{DataAggregationProvider, IntegrationProvider};
use crate::services::TagService;

/// Sync service for account and transaction synchronization
pub struct SyncService {
    repository: Arc<DuckDbRepository>,
    tag_service: TagService,
    _treeline_dir: PathBuf,
    providers: HashMap<String, Arc<dyn DataAggregationProvider>>,
    integration_providers: HashMap<String, Arc<dyn IntegrationProvider>>,
}

impl SyncService {
    pub fn new(repository: Arc<DuckDbRepository>, treeline_dir: PathBuf) -> Self {
        let mut providers: HashMap<String, Arc<dyn DataAggregationProvider>> = HashMap::new();
        let mut integration_providers: HashMap<String, Arc<dyn IntegrationProvider>> =
            HashMap::new();

        // Register built-in providers
        let demo = Arc::new(DemoDataProvider::new());
        providers.insert("demo".to_string(), demo.clone());
        integration_providers.insert("demo".to_string(), demo);

        let simplefin = Arc::new(SimpleFINProvider::new());
        providers.insert("simplefin".to_string(), simplefin.clone());
        integration_providers.insert("simplefin".to_string(), simplefin);

        // Register Lunchflow provider (global bank connections)
        let lunchflow = Arc::new(LunchflowProvider::new());
        providers.insert("lunchflow".to_string(), lunchflow.clone());
        integration_providers.insert("lunchflow".to_string(), lunchflow);

        let tag_service = TagService::new(repository.clone());

        Self {
            repository,
            tag_service,
            _treeline_dir: treeline_dir,
            providers,
            integration_providers,
        }
    }

    /// Sync from all integrations or a specific one
    ///
    /// If `balances_only` is true, skips transaction fetching entirely.
    /// This is useful for users who just want to track account balances.
    pub fn sync(
        &self,
        integration: Option<&str>,
        dry_run: bool,
        balances_only: bool,
    ) -> Result<SyncResult> {
        let integrations = self.repository.get_integrations()?;
        let mut results = Vec::new();

        let integrations_to_sync: Vec<_> = if let Some(name) = integration {
            integrations.iter().filter(|i| i.name == name).collect()
        } else {
            integrations.iter().collect()
        };

        if integrations_to_sync.is_empty() {
            anyhow::bail!("No integrations configured");
        }

        for int in integrations_to_sync {
            let result = self.sync_integration(&int.name, &int.settings, dry_run, balances_only)?;
            results.push(result);
        }

        Ok(SyncResult {
            results,
            new_accounts_without_type: Vec::new(),
        })
    }

    fn sync_integration(
        &self,
        name: &str,
        settings: &serde_json::Value,
        dry_run: bool,
        balances_only: bool,
    ) -> Result<IntegrationSyncResult> {
        // Look up provider by name
        let provider = self
            .providers
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", name))?;

        let now = Utc::now();
        let end_date = now.naive_utc().date();

        // Calculate start date based on sync type
        let max_tx_date = self.repository.get_max_transaction_date()?;
        let (start_date, is_incremental) = match max_tx_date {
            Some(max_date) => (max_date - Duration::days(7), true),
            None => ((now - Duration::days(90)).naive_utc().date(), false),
        };

        let sync_type = if is_incremental {
            "incremental"
        } else {
            "initial"
        };

        // Fetch accounts from provider
        let accounts_result = provider.get_accounts(settings)?;
        let mut provider_warnings = accounts_result.warnings;

        // Build map of provider external ID to internal account ID
        // Use provider-specific columns (sf_id/lf_id) for mapping
        let existing_accounts = self.repository.get_accounts()?;
        let mut external_to_internal: HashMap<String, Uuid> = HashMap::new();

        for existing in &existing_accounts {
            // Get provider-specific ID column
            let ext_id = match name {
                "simplefin" => existing.sf_id.clone(),
                "lunchflow" => existing.lf_id.clone(),
                // Demo mode: use the account name as the external ID (stable across syncs)
                "demo" => Some(existing.name.clone()),
                _ => None,
            };

            if let Some(id) = ext_id {
                external_to_internal.insert(id, existing.id);
            }
        }

        // Track original account IDs for balance snapshot mapping
        let mut orig_to_ext: HashMap<Uuid, String> = HashMap::new();
        for account in &accounts_result.accounts {
            let ext_id = match name {
                "simplefin" => account.sf_id.clone(),
                "lunchflow" => account.lf_id.clone(),
                // Demo mode: use the account name as the external ID
                "demo" => Some(account.name.clone()),
                _ => None,
            };

            if let Some(id) = ext_id {
                orig_to_ext.insert(account.id, id);
            }
        }

        // Process accounts
        let mut accounts_synced = 0i64;
        for mut account in accounts_result.accounts {
            // Get external ID from provider-specific column
            let ext_id = match name {
                "simplefin" => account.sf_id.clone(),
                "lunchflow" => account.lf_id.clone(),
                // Demo mode: use the account name as the external ID
                "demo" => Some(account.name.clone()),
                _ => None,
            };
            let ext_id = ext_id.unwrap_or_default();

            if let Some(&existing_id) = external_to_internal.get(&ext_id) {
                // Existing account - update ID
                account.id = existing_id;
                if !dry_run {
                    self.repository.upsert_account(&account)?;
                }
            } else {
                // New account
                external_to_internal.insert(ext_id, account.id);
                accounts_synced += 1;
                if !dry_run {
                    self.repository.upsert_account(&account)?;
                }
            }
        }

        // Save balance snapshots
        if !dry_run {
            for snapshot in accounts_result.balance_snapshots {
                if let Some(ext_id) = orig_to_ext.get(&snapshot.account_id) {
                    if let Some(&internal_id) = external_to_internal.get(ext_id) {
                        let mut updated = snapshot;
                        updated.account_id = internal_id;
                        let _ = self.repository.add_balance_snapshot(&updated);
                    }
                }
            }
        }

        // Skip transaction fetching entirely if balances_only mode
        let (discovered, new_count, skipped_count) = if balances_only {
            (0, 0, 0)
        } else {
            // Fetch transactions (excluding per-account balances-only settings)
            // Check accountSettings for balancesOnly flag on each account
            let account_settings = settings.get("accountSettings").and_then(|v| v.as_object());

            let ext_account_ids: Vec<String> = external_to_internal
                .keys()
                .filter(|ext_id| {
                    // Include account only if NOT marked as balancesOnly
                    if let Some(settings_map) = account_settings {
                        if let Some(acc_settings) = settings_map.get(*ext_id) {
                            // Default to false if balancesOnly not present
                            return !acc_settings
                                .get("balancesOnly")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                        }
                    }
                    // No settings for this account = include it
                    true
                })
                .cloned()
                .collect();

            let txs_result =
                provider.get_transactions(start_date, end_date, &ext_account_ids, settings)?;
            provider_warnings.extend(txs_result.warnings);

            // Process transactions with deduplication
            let (new_count, skipped_count) = self.process_transactions(
                name,
                txs_result.transactions,
                &external_to_internal,
                dry_run,
            )?;

            let discovered = new_count + skipped_count;
            (discovered, new_count, skipped_count)
        };

        Ok(IntegrationSyncResult {
            integration: name.to_string(),
            accounts_synced,
            transactions_synced: new_count,
            transaction_stats: TransactionStats {
                discovered,
                new: new_count,
                skipped: skipped_count,
            },
            sync_type: sync_type.to_string(),
            start_date: start_date.format("%Y-%m-%d").to_string(),
            end_date: end_date.format("%Y-%m-%d").to_string(),
            provider_warnings,
            error: None,
        })
    }

    /// Process transactions with deduplication logic
    ///
    /// Deduplication strategy:
    /// 1. Check by provider-specific ID column (sf_id or lf_id) - indexed, fast
    /// 2. Check by fingerprint (account + date + amount + description hash)
    ///
    /// If either exists, skip the transaction to preserve user edits.
    fn process_transactions(
        &self,
        provider_name: &str,
        transactions: Vec<(String, crate::domain::Transaction)>,
        external_to_internal: &HashMap<String, Uuid>,
        dry_run: bool,
    ) -> Result<(i64, i64)> {
        let mut new_count = 0i64;
        let mut skipped_count = 0i64;
        let mut new_tx_ids: Vec<Uuid> = Vec::new();

        for (ext_account_id, mut tx) in transactions {
            // Map to internal account ID
            let internal_account_id = match external_to_internal.get(&ext_account_id) {
                Some(&id) => id,
                None => continue,
            };
            tx.account_id = internal_account_id;

            // Check if exists by provider-specific ID column (indexed, fast)
            // Note: csv_fingerprint is only for CSV imports, not provider syncs
            let already_exists = match provider_name {
                "simplefin" => {
                    if let Some(ref sf_id) = tx.sf_id {
                        self.repository.transaction_exists_by_sf_id(sf_id)?
                    } else {
                        false
                    }
                }
                "lunchflow" => {
                    if let Some(ref lf_id) = tx.lf_id {
                        self.repository.transaction_exists_by_lf_id(lf_id)?
                    } else {
                        false
                    }
                }
                // Demo mode: no provider ID, always insert (demo has its own DB)
                _ => false,
            };

            if already_exists {
                skipped_count += 1;
            } else {
                new_count += 1;
                if !dry_run {
                    new_tx_ids.push(tx.id);
                    self.repository.upsert_transaction(&tx)?;
                }
            }
        }

        // Apply auto-tag rules to newly synced transactions
        if !dry_run && !new_tx_ids.is_empty() {
            // Best-effort tagging - don't fail sync if rules fail
            let _ = self.tag_service.apply_auto_tag_rules(&new_tx_ids);
        }

        Ok((new_count, skipped_count))
    }

    /// List configured integrations
    pub fn list_integrations(&self) -> Result<Vec<IntegrationInfo>> {
        let integrations = self.repository.get_integrations()?;
        Ok(integrations
            .iter()
            .map(|i| IntegrationInfo {
                name: i.name.clone(),
                provider: i.name.clone(),
            })
            .collect())
    }

    /// Remove an integration
    pub fn remove_integration(&self, name: &str) -> Result<()> {
        if !self.repository.delete_integration(name)? {
            anyhow::bail!("Integration not found: {}", name);
        }
        Ok(())
    }

    /// Set up a new integration using the appropriate provider
    pub fn setup_integration(
        &self,
        provider_name: &str,
        options: &serde_json::Value,
    ) -> Result<()> {
        let provider = self
            .integration_providers
            .get(provider_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", provider_name))?;

        let settings = provider.setup(options)?;
        self.repository
            .upsert_integration(provider_name, &settings)?;
        Ok(())
    }

    /// Set up demo integration (convenience method)
    pub fn setup_demo(&self) -> Result<()> {
        self.setup_integration("demo", &serde_json::json!({}))
    }

    /// Set up SimpleFIN integration (convenience method)
    pub fn setup_simplefin(&self, setup_token: &str) -> Result<()> {
        self.setup_integration(
            "simplefin",
            &serde_json::json!({
                "setupToken": setup_token
            }),
        )
    }

    /// Set up Lunchflow integration (convenience method)
    ///
    /// # Arguments
    /// * `api_key` - The Lunchflow API key from the user's dashboard
    /// * `base_url` - Optional custom base URL for testing (None = production)
    pub fn setup_lunchflow(&self, api_key: &str, base_url: Option<&str>) -> Result<()> {
        let mut options = serde_json::json!({
            "apiKey": api_key
        });
        if let Some(url) = base_url {
            options["baseUrl"] = serde_json::json!(url);
        }
        self.setup_integration("lunchflow", &options)
    }
}

#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub results: Vec<IntegrationSyncResult>,
    pub new_accounts_without_type: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct IntegrationSyncResult {
    pub integration: String,
    pub accounts_synced: i64,
    pub transactions_synced: i64,
    pub transaction_stats: TransactionStats,
    pub sync_type: String,
    pub start_date: String,
    pub end_date: String,
    pub provider_warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TransactionStats {
    pub discovered: i64,
    pub new: i64,
    pub skipped: i64,
}

#[derive(Debug, Serialize)]
pub struct IntegrationInfo {
    pub name: String,
    pub provider: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Transaction;
    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    #[test]
    fn test_transaction_fingerprint_consistency() {
        // Same transaction data should produce same fingerprint
        let tx1 = Transaction::new(
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            Decimal::new(1234, 2), // $12.34
            NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
        );

        let tx2 = Transaction::new(
            Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap(), // Different ID
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(), // Same account
            Decimal::new(1234, 2),                                            // Same amount
            NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),                    // Same date
        );

        // Fingerprints should match (same account, amount, date)
        assert_eq!(tx1.calculate_fingerprint(), tx2.calculate_fingerprint());
    }

    #[test]
    fn test_transaction_fingerprint_differs_by_amount() {
        let tx1 = Transaction::new(
            Uuid::new_v4(),
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            Decimal::new(1234, 2),
            NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
        );

        let tx2 = Transaction::new(
            Uuid::new_v4(),
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            Decimal::new(5678, 2), // Different amount
            NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
        );

        assert_ne!(tx1.calculate_fingerprint(), tx2.calculate_fingerprint());
    }

    #[test]
    fn test_transaction_fingerprint_differs_by_date() {
        let tx1 = Transaction::new(
            Uuid::new_v4(),
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            Decimal::new(1234, 2),
            NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
        );

        let tx2 = Transaction::new(
            Uuid::new_v4(),
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            Decimal::new(1234, 2),
            NaiveDate::from_ymd_opt(2024, 1, 16).unwrap(), // Different date
        );

        assert_ne!(tx1.calculate_fingerprint(), tx2.calculate_fingerprint());
    }

    #[test]
    fn test_transaction_fingerprint_differs_by_account() {
        let tx1 = Transaction::new(
            Uuid::new_v4(),
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            Decimal::new(1234, 2),
            NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
        );

        let tx2 = Transaction::new(
            Uuid::new_v4(),
            Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap(), // Different account
            Decimal::new(1234, 2),
            NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
        );

        assert_ne!(tx1.calculate_fingerprint(), tx2.calculate_fingerprint());
    }
}
