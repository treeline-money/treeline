//! The query cookbook (docs/src/content/docs/ai-agents/query-cookbook.mdx)
//! promises that every SQL block on the page runs against the demo database.
//! This test extracts each ```sql block from the page and runs it, so the
//! docs can't drift from the schema.

use std::path::PathBuf;
use tempfile::TempDir;
use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::services::DemoService;

fn cookbook_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/src/content/docs/ai-agents/query-cookbook.mdx")
}

fn extract_sql_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        match &mut current {
            None if line.trim() == "```sql" => current = Some(String::new()),
            Some(block) if line.trim() == "```" => {
                blocks.push(std::mem::take(block));
                current = None;
            }
            Some(block) => {
                block.push_str(line);
                block.push('\n');
            }
            None => {}
        }
    }
    blocks
}

#[test]
fn every_cookbook_query_runs_against_the_demo_database() {
    let markdown =
        std::fs::read_to_string(cookbook_path()).expect("cookbook page exists at documented path");
    let blocks = extract_sql_blocks(&markdown);
    assert!(
        blocks.len() >= 10,
        "expected the cookbook to contain at least 10 sql blocks, found {}",
        blocks.len()
    );

    let dir = TempDir::new().unwrap();
    DemoService::new(dir.path()).enable().unwrap();
    let repo = DuckDbRepository::new(&dir.path().join("demo.duckdb"), None).unwrap();

    let mut empty = Vec::new();
    for (i, sql) in blocks.iter().enumerate() {
        // The month-setup INSERT is a write, so it goes through the write path.
        // Demo data already includes the current budget month, so its guard
        // makes it a no-op — it must still parse and execute cleanly.
        if sql.trim_start().to_ascii_uppercase().starts_with("INSERT") {
            repo.execute_sql(sql).unwrap_or_else(|e| {
                panic!("cookbook write #{} failed: {}\n---\n{}", i + 1, e, sql)
            });
            continue;
        }
        let result = repo
            .execute_query(sql)
            .unwrap_or_else(|e| panic!("cookbook query #{} failed: {}\n---\n{}", i + 1, e, sql));
        if result.row_count == 0 {
            empty.push(i + 1);
        }
    }

    // Demo data is generated relative to today, so nearly every query should
    // return rows. The untagged-transactions queue is legitimately empty on
    // demo data; anything beyond that means a query stopped matching reality.
    assert!(
        empty.len() <= 1,
        "cookbook queries returned no rows (1-indexed): {:?}",
        empty
    );
}

#[test]
fn cookbook_net_worth_matches_demo_snapshots() {
    let dir = TempDir::new().unwrap();
    DemoService::new(dir.path()).enable().unwrap();
    let repo = DuckDbRepository::new(&dir.path().join("demo.duckdb"), None).unwrap();

    // The cookbook's first query: latest snapshot per account, raw sum.
    let cookbook = repo
        .execute_query(
            "WITH latest AS (
               SELECT account_id, balance FROM balance_snapshots
               QUALIFY ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY snapshot_time DESC) = 1
             ) SELECT SUM(balance) FROM latest",
        )
        .unwrap();

    // Independent arithmetic: assets minus liability magnitudes, computed from
    // the same latest snapshots but with explicit classification handling.
    // Liability balances are stored negative, so the two must agree — this
    // pins the sign convention the cookbook documents.
    let by_class = repo
        .execute_query(
            "WITH latest AS (
               SELECT account_id, balance FROM balance_snapshots
               QUALIFY ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY snapshot_time DESC) = 1
             )
             SELECT SUM(CASE WHEN a.classification = 'asset' THEN l.balance ELSE 0 END)
                  - SUM(CASE WHEN a.classification = 'liability' THEN ABS(l.balance) ELSE 0 END)
             FROM latest l JOIN accounts a USING (account_id)",
        )
        .unwrap();

    let net = cookbook.rows[0][0].as_f64().unwrap();
    let expected = by_class.rows[0][0].as_f64().unwrap();
    assert!(
        (net - expected).abs() < 0.01,
        "cookbook net worth {} != classification-based {}",
        net,
        expected
    );
}
