//! Tests that SyncBundle::extract respects the DuckDB filesystem lock.
//!
//! Without this guarantee, `tl hub watch` (or any pull path) could overwrite
//! `treeline.duckdb` mid-query from a concurrent DuckDB operation, corrupting
//! an open connection on the same machine (e.g. the desktop app).
//!
//! Run with: cargo test --test concurrent_extract_test -- --nocapture

use std::fs::OpenOptions;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fs2::FileExt;
use tempfile::TempDir;

use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::services::hub::SyncBundle;

fn make_bundle(source_dir: &TempDir) -> Vec<u8> {
    let db_path = source_dir.path().join("treeline.duckdb");
    let repo = DuckDbRepository::new(&db_path, None).expect("repo");
    repo.ensure_schema().expect("schema");
    repo.checkpoint().expect("checkpoint");
    drop(repo);
    SyncBundle::create(source_dir.path()).expect("bundle")
}

#[test]
fn extract_blocks_while_db_lock_is_held_then_completes_when_released() {
    let hub_dir = TempDir::new().unwrap();
    // Seed hub dir with an empty db so the lock file path exists.
    let _seed = DuckDbRepository::new(
        &hub_dir.path().join("treeline.duckdb"),
        None,
    )
    .unwrap();
    drop(_seed);

    let source = TempDir::new().unwrap();
    let bundle = Arc::new(make_bundle(&source));

    // Acquire the DuckDB lock file ourselves (simulating an in-flight DB op
    // in another process, e.g. the desktop app mid-query).
    let lock_path = hub_dir.path().join("treeline.duckdb.lock");
    let held_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    FileExt::lock_exclusive(&held_lock).unwrap();

    // Spawn extract in another thread — it should block until we release.
    let bundle_for_thread = bundle.clone();
    let hub_path = hub_dir.path().to_path_buf();
    let extract_handle = std::thread::spawn(move || {
        let started = Instant::now();
        SyncBundle::extract(&bundle_for_thread, &hub_path).expect("extract");
        started.elapsed()
    });

    // Let the thread reach the lock_exclusive call and block.
    std::thread::sleep(Duration::from_millis(250));

    // Release our lock. Extract should now proceed.
    FileExt::unlock(&held_lock).unwrap();
    drop(held_lock);

    let elapsed = extract_handle.join().unwrap();
    assert!(
        elapsed >= Duration::from_millis(200),
        "extract did not block on the DB lock; it completed in {:?}",
        elapsed
    );

    // And the file was actually written.
    assert!(hub_dir.path().join("treeline.duckdb").exists());
}

#[test]
fn extract_releases_lock_so_subsequent_db_ops_work() {
    let hub_dir = TempDir::new().unwrap();
    let source = TempDir::new().unwrap();
    let bundle = make_bundle(&source);

    SyncBundle::extract(&bundle, hub_dir.path()).unwrap();

    // A brand-new repo against the extracted path must be able to acquire
    // its own lock. If extract leaked the lock, this would hang.
    let repo = DuckDbRepository::new(
        &hub_dir.path().join("treeline.duckdb"),
        None,
    )
    .expect("must be able to open extracted DB");
    let accounts = repo.get_accounts().expect("query");
    assert_eq!(accounts.len(), 0);
}

#[test]
fn extract_with_no_contention_is_fast() {
    let hub_dir = TempDir::new().unwrap();
    let source = TempDir::new().unwrap();
    let bundle = make_bundle(&source);

    let start = Instant::now();
    SyncBundle::extract(&bundle, hub_dir.path()).unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "extract without contention was unexpectedly slow: {:?}",
        elapsed
    );
}
