//! Backup domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Metadata for a backup file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// Backup filename (e.g., "treeline-2025-01-15T10-30-00.duckdb")
    pub name: String,
    /// When the backup was created
    pub created_at: DateTime<Utc>,
    /// File size in bytes
    pub size_bytes: u64,
}

impl BackupMetadata {
    pub fn new(name: impl Into<String>, created_at: DateTime<Utc>, size_bytes: u64) -> Self {
        Self {
            name: name.into(),
            created_at,
            size_bytes,
        }
    }

    /// Format size for human display
    pub fn size_display(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if self.size_bytes >= GB {
            format!("{:.1} GB", self.size_bytes as f64 / GB as f64)
        } else if self.size_bytes >= MB {
            format!("{:.1} MB", self.size_bytes as f64 / MB as f64)
        } else if self.size_bytes >= KB {
            format!("{:.1} KB", self.size_bytes as f64 / KB as f64)
        } else {
            format!("{} bytes", self.size_bytes)
        }
    }
}

/// COMMENT: again, test code? Is this a common Rust pattern?
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_display() {
        let meta = BackupMetadata::new("test.duckdb", Utc::now(), 1536);
        assert_eq!(meta.size_display(), "1.5 KB");

        let meta = BackupMetadata::new("test.duckdb", Utc::now(), 2 * 1024 * 1024);
        assert_eq!(meta.size_display(), "2.0 MB");
    }
}
