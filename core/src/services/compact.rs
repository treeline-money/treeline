//! Compact service - database compaction

use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;

use crate::adapters::duckdb::DuckDbRepository;

/// Compact service for database maintenance
pub struct CompactService {
    repository: Arc<DuckDbRepository>,
}

impl CompactService {
    pub fn new(repository: Arc<DuckDbRepository>) -> Self {
        Self { repository }
    }

    /// Compact the database
    pub fn compact(&self) -> Result<CompactResult> {
        let original_size = self.repository.get_db_size()?;

        self.repository.compact()?;

        let compacted_size = self.repository.get_db_size()?;

        Ok(CompactResult {
            original_size,
            compacted_size,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct CompactResult {
    pub original_size: u64,
    pub compacted_size: u64,
}
