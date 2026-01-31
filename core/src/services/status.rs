//! Status service - account and transaction summaries

use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;

use crate::adapters::duckdb::DuckDbRepository;

/// Status service for account summaries
pub struct StatusService {
    repository: Arc<DuckDbRepository>,
}

impl StatusService {
    pub fn new(repository: Arc<DuckDbRepository>) -> Self {
        Self { repository }
    }

    /// Get overall status summary
    pub fn get_status(&self) -> Result<StatusSummary> {
        let accounts = self.repository.get_accounts()?;
        let transaction_count = self.repository.get_transaction_count()?;
        let snapshot_count = self.repository.get_balance_snapshot_count()?;
        let integrations = self.repository.get_integrations()?;
        let date_range = self.repository.get_transaction_date_range()?;

        Ok(StatusSummary {
            total_accounts: accounts.len() as i64,
            total_transactions: transaction_count,
            total_snapshots: snapshot_count,
            total_integrations: integrations.len() as i64,
            integration_names: integrations.iter().map(|i| i.name.clone()).collect(),
            accounts: accounts
                .into_iter()
                .map(|a| AccountSummary {
                    id: a.id.to_string(),
                    name: a.name,
                    institution_name: a.institution_name,
                })
                .collect(),
            date_range,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct StatusSummary {
    pub total_accounts: i64,
    pub total_transactions: i64,
    pub total_snapshots: i64,
    pub total_integrations: i64,
    pub integration_names: Vec<String>,
    pub accounts: Vec<AccountSummary>,
    pub date_range: DateRange,
}

#[derive(Debug, Serialize)]
pub struct AccountSummary {
    pub id: String,
    pub name: String,
    pub institution_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DateRange {
    pub earliest: Option<String>,
    pub latest: Option<String>,
}
