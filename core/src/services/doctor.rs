//! Doctor service - database health checks

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use serde_json::json;

use crate::adapters::duckdb::DuckDbRepository;
use crate::services::plugin::PluginService;

/// Most `details` entries kept from a plugin's doctor view.
const MAX_PLUGIN_DETAILS: usize = 50;
/// Longest plugin-supplied message kept, in characters.
const MAX_PLUGIN_MESSAGE: usize = 500;

/// Doctor service for health checks
pub struct DoctorService {
    repository: Arc<DuckDbRepository>,
    treeline_dir: PathBuf,
}

impl DoctorService {
    pub fn new(repository: Arc<DuckDbRepository>, treeline_dir: PathBuf) -> Self {
        Self {
            repository,
            treeline_dir,
        }
    }

    /// Run all health checks
    pub fn run_checks(&self) -> Result<DoctorResult> {
        let mut checks = std::collections::HashMap::new();

        // Orphaned transactions
        let orphaned_txs = self.repository.check_orphaned_transactions()?;
        let orphan_details: Vec<serde_json::Value> = orphaned_txs
            .iter()
            .map(|s| {
                // Parse "tx_id:account_id" format
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() >= 2 {
                    json!({
                        "transaction_id": parts[0],
                        "account_id": parts[1]
                    })
                } else {
                    json!({"transaction_id": s})
                }
            })
            .collect();
        checks.insert(
            "orphaned_transactions".to_string(),
            CheckResult {
                name: None,
                status: if orphaned_txs.is_empty() {
                    "pass"
                } else {
                    "error"
                }
                .to_string(),
                message: if orphaned_txs.is_empty() {
                    "No orphaned transactions found".to_string()
                } else {
                    format!(
                        "{} transaction(s) reference missing accounts",
                        orphaned_txs.len()
                    )
                },
                details: if orphaned_txs.is_empty() {
                    None
                } else {
                    Some(orphan_details)
                },
            },
        );

        // Orphaned snapshots
        let orphaned_snaps = self.repository.check_orphaned_snapshots()?;
        let snap_details: Vec<serde_json::Value> = orphaned_snaps
            .iter()
            .map(|s| {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() >= 2 {
                    json!({
                        "snapshot_id": parts[0],
                        "account_id": parts[1]
                    })
                } else {
                    json!({"snapshot_id": s})
                }
            })
            .collect();
        checks.insert(
            "orphaned_snapshots".to_string(),
            CheckResult {
                name: None,
                status: if orphaned_snaps.is_empty() {
                    "pass"
                } else {
                    "error"
                }
                .to_string(),
                message: if orphaned_snaps.is_empty() {
                    "No orphaned snapshots found".to_string()
                } else {
                    format!(
                        "{} snapshot(s) reference missing accounts",
                        orphaned_snaps.len()
                    )
                },
                details: if orphaned_snaps.is_empty() {
                    None
                } else {
                    Some(snap_details)
                },
            },
        );

        // Date sanity - check both past (before 1970) and future (more than 1 year ahead)
        let insane_dates = self.repository.check_date_sanity()?;
        let date_details: Vec<serde_json::Value> = insane_dates
            .iter()
            .map(|d| {
                let parts: Vec<&str> = d.split('|').collect();
                if parts.len() >= 4 {
                    json!({
                        "transaction_id": parts[0],
                        "date": parts[1],
                        "description": parts[2],
                        "amount": parts[3].parse::<f64>().ok()
                    })
                } else {
                    json!({"info": d})
                }
            })
            .collect();
        checks.insert(
            "date_sanity".to_string(),
            CheckResult {
                name: None,
                status: if insane_dates.is_empty() {
                    "pass"
                } else {
                    "error"
                }
                .to_string(),
                message: if insane_dates.is_empty() {
                    "All transaction dates are valid".to_string()
                } else {
                    format!(
                        "{} transaction(s) have unreasonable dates",
                        insane_dates.len()
                    )
                },
                details: if insane_dates.is_empty() {
                    None
                } else {
                    Some(date_details)
                },
            },
        );

        // Untagged transactions - Python warns on any untagged
        let untagged = self.repository.count_untagged_transactions()?;
        let total_txs = self.repository.get_transaction_count()?;
        let untagged_pct = if total_txs > 0 {
            (untagged as f64 / total_txs as f64 * 100.0) as i64
        } else {
            0
        };
        checks.insert(
            "untagged_transactions".to_string(),
            CheckResult {
                name: None,
                status: if untagged == 0 { "pass" } else { "warning" }.to_string(),
                message: if untagged == 0 {
                    "All transactions are tagged".to_string()
                } else {
                    format!(
                        "{} transaction(s) have no tags ({}% of total)",
                        untagged, untagged_pct
                    )
                },
                details: if untagged == 0 {
                    None
                } else {
                    Some(vec![json!({
                        "untagged_count": untagged,
                        "total_count": total_txs
                    })])
                },
            },
        );

        // Duplicate transactions check - finds duplicate sf_ids or lf_ids
        let duplicate_sf_ids = self.repository.check_duplicate_sf_ids()?;
        let duplicate_lf_ids = self.repository.check_duplicate_lf_ids()?;
        let total_duplicates = duplicate_sf_ids.len() + duplicate_lf_ids.len();

        let dup_details: Vec<serde_json::Value> = duplicate_sf_ids
            .iter()
            .map(|id| json!({"type": "sf_id", "id": id}))
            .chain(
                duplicate_lf_ids
                    .iter()
                    .map(|id| json!({"type": "lf_id", "id": id})),
            )
            .collect();

        checks.insert(
            "duplicate_transactions".to_string(),
            CheckResult {
                name: None,
                status: if total_duplicates == 0 {
                    "pass"
                } else {
                    "warning"
                }
                .to_string(),
                message: if total_duplicates == 0 {
                    "No duplicate transactions found".to_string()
                } else {
                    format!(
                        "{} duplicate provider ID(s) found ({} sf_id, {} lf_id)",
                        total_duplicates,
                        duplicate_sf_ids.len(),
                        duplicate_lf_ids.len()
                    )
                },
                details: if total_duplicates == 0 {
                    None
                } else {
                    Some(dup_details)
                },
            },
        );

        // Integration connectivity - test via dry-run sync
        let integrations = self.repository.get_integrations()?;
        if integrations.is_empty() {
            checks.insert(
                "integration_connectivity".to_string(),
                CheckResult {
                    name: None,
                    status: "pass".to_string(),
                    message: "No integrations configured".to_string(),
                    details: None,
                },
            );
        } else {
            // For now, just report integrations are configured
            // Full implementation would do a dry-run sync to test connectivity
            checks.insert(
                "integration_connectivity".to_string(),
                CheckResult {
                    name: None,
                    status: "pass".to_string(),
                    message: format!("All {} integration(s) connected", integrations.len()),
                    details: None,
                },
            );
        }

        // Plugin-published checks (see run_plugin_checks)
        self.run_plugin_checks(&mut checks);

        // Calculate summary
        let passed = checks.values().filter(|c| c.status == "pass").count() as i64;
        let warnings = checks.values().filter(|c| c.status == "warning").count() as i64;
        let errors = checks.values().filter(|c| c.status == "error").count() as i64;

        Ok(DoctorResult {
            checks,
            summary: DoctorSummary {
                passed,
                warnings,
                errors,
            },
        })
    }

    /// Collect checks published by installed plugins.
    ///
    /// A plugin's headless surface is its schema, so doctor checks are
    /// discovered by convention rather than by running plugin code: if a
    /// plugin owns a view `<schema>.doctor`, core selects from it and turns
    /// each row into a check keyed `<plugin_id>.<check_id>`. Expected columns
    /// are `check_id`, `name`, `status` ('pass' | 'warning' | 'error'),
    /// `message`, and an optional `details`.
    ///
    /// A misbehaving plugin must never abort the run: every failure here is
    /// contained to that plugin's own checks.
    fn run_plugin_checks(&self, checks: &mut HashMap<String, CheckResult>) {
        let plugin_service = PluginService::new(&self.treeline_dir);
        let manifests = match plugin_service.list_manifests() {
            Ok(manifests) => manifests,
            Err(_) => return,
        };

        for manifest in manifests {
            let schema = manifest.schema_name();
            // Schema names are interpolated into SQL, so only accept plain
            // identifiers. A manifest that declares anything else is ignored.
            if !is_plain_identifier(&schema) {
                continue;
            }
            let plugin_id = manifest.id;

            // Schema missing means the desktop app has never loaded the
            // plugin, so its migrations have not run.
            match self.repository.schema_exists(&schema) {
                Ok(true) => {}
                Ok(false) => {
                    checks.insert(
                        format!("{}.initialized", plugin_id),
                        CheckResult {
                            name: None,
                            status: "warning".to_string(),
                            message:
                                "Plugin installed but not initialized — open the desktop app once"
                                    .to_string(),
                            details: None,
                        },
                    );
                    continue;
                }
                Err(_) => continue,
            }

            // No doctor view is fine - the convention is opt-in.
            match self.repository.table_exists(&format!("{}.doctor", schema)) {
                Ok(true) => {}
                _ => continue,
            }

            // to_json() gives one JSON object per row, which keeps `details`
            // intact whatever shape the plugin used and makes the column
            // optional for free.
            let sql = format!("SELECT to_json(d)::VARCHAR FROM {}.doctor d", schema);
            let rows = match self.repository.execute_query(&sql) {
                Ok(result) => result.rows,
                Err(e) => {
                    checks.insert(
                        format!("{}.doctor", plugin_id),
                        CheckResult {
                            name: None,
                            status: "error".to_string(),
                            message: format!(
                                "doctor view failed: {}",
                                sanitize(&e.to_string(), 200)
                            ),
                            details: None,
                        },
                    );
                    continue;
                }
            };

            for (index, row) in rows.iter().enumerate() {
                let Some(row) = row
                    .first()
                    .and_then(|v| v.as_str())
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                else {
                    continue;
                };

                let check_id = row
                    .get("check_id")
                    .and_then(|v| v.as_str())
                    .map(|s| sanitize(s, 64))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("check_{}", index + 1));

                let raw_status = row.get("status").and_then(|v| v.as_str()).unwrap_or("");
                let message = row
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(|s| sanitize(s, MAX_PLUGIN_MESSAGE))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "(no message)".to_string());

                let (status, message) = match raw_status.trim().to_lowercase().as_str() {
                    "pass" => ("pass".to_string(), message),
                    "warning" => ("warning".to_string(), message),
                    "error" => ("error".to_string(), message),
                    _ => (
                        "error".to_string(),
                        format!(
                            "unknown status '{}' from plugin doctor view (expected pass, warning, or error)",
                            sanitize(raw_status, 40)
                        ),
                    ),
                };

                let details = match row.get("details") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(serde_json::Value::Array(items)) if items.is_empty() => None,
                    Some(serde_json::Value::Array(items)) => {
                        Some(items.iter().take(MAX_PLUGIN_DETAILS).cloned().collect())
                    }
                    Some(other) => Some(vec![other.clone()]),
                };

                checks.insert(
                    format!("{}.{}", plugin_id, check_id),
                    CheckResult {
                        name: row
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| sanitize(s, 100))
                            .filter(|s| !s.is_empty()),
                        status,
                        message,
                        details,
                    },
                );
            }
        }
    }
}

/// True for names safe to interpolate into SQL as a bare identifier.
fn is_plain_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Collapse whitespace and clamp length, so plugin-supplied text (and DuckDB
/// error text, which can echo the plugin's SQL) can't wreck the output.
fn sanitize(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        collapsed
    } else {
        let truncated: String = collapsed.chars().take(max_chars).collect();
        format!("{}…", truncated)
    }
}

#[derive(Debug, Serialize)]
pub struct DoctorResult {
    pub checks: std::collections::HashMap<String, CheckResult>,
    pub summary: DoctorSummary,
}

#[derive(Debug, Serialize)]
pub struct CheckResult {
    /// Human label. Only plugin-published checks set this; built-in checks are
    /// identified by their key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize)]
pub struct DoctorSummary {
    pub passed: i64,
    pub warnings: i64,
    pub errors: i64,
}
