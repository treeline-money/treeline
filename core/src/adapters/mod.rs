//! Adapter implementations
//!
//! Adapters implement the port traits with concrete technologies:
//! - DuckDB for the Repository port
//! - SimpleFIN HTTP client for DataAggregationProvider
//! - Lunchflow HTTP client for DataAggregationProvider (global banks)
//! - Demo data provider for testing
//! - Local filesystem for BackupStorageProvider

pub mod demo;
pub mod duckdb;
pub mod lunchflow;
pub mod simplefin;
