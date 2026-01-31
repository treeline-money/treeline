//! Concurrent database access tests
//!
//! These tests verify that the database handles concurrent access safely.
//! They are designed to FAIL before implementing the filesystem lock fix,
//! demonstrating the race conditions that can cause database corruption.
//!
//! Run with: cargo test --test concurrent_access_test -- --nocapture
//! Run specific test: cargo test --test concurrent_access_test test_name -- --nocapture

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use uuid::Uuid;

use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::domain::Account;

/// Number of concurrent threads for stress tests.
/// Keep this realistic - in production we'd have at most a few processes
/// (app + CLI + maybe another CLI command) competing for the lock.
const THREAD_COUNT: usize = 6;

/// Number of iterations per thread
const ITERATIONS_PER_THREAD: usize = 5;

/// Helper to create a test account
fn create_test_account(suffix: &str) -> Account {
    Account::new(Uuid::new_v4(), format!("Test Account {}", suffix))
}

/// Test: Multiple threads creating separate DuckDbRepository instances
/// and writing to the same database file simultaneously.
///
/// This simulates the Tauri app behavior where each command creates
/// a new TreelineContext with its own DuckDbRepository.
///
/// Expected behavior BEFORE fix: Race conditions, potential corruption
/// Expected behavior AFTER fix: All operations serialize via file lock
#[test]
fn test_concurrent_repository_instances_writing() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_concurrent.duckdb");

    // Create initial database with schema
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
    }

    let barrier = Arc::new(Barrier::new(THREAD_COUNT));
    let db_path = Arc::new(db_path);
    let success_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..THREAD_COUNT {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let success_count = Arc::clone(&success_count);
        let error_count = Arc::clone(&error_count);

        let handle = thread::spawn(move || {
            // Wait for all threads to be ready
            barrier.wait();

            let start = Instant::now();

            // Each thread creates its OWN repository instance (simulating Tauri commands)
            match DuckDbRepository::new(&db_path, None) {
                Ok(repo) => {
                    // Perform multiple write operations
                    for i in 0..ITERATIONS_PER_THREAD {
                        let account = create_test_account(&format!("t{}_i{}", thread_id, i));
                        match repo.upsert_account(&account) {
                            Ok(_) => {
                                success_count.fetch_add(1, Ordering::SeqCst);
                            }
                            Err(e) => {
                                eprintln!(
                                    "Thread {}: Write error at iteration {}: {}",
                                    thread_id, i, e
                                );
                                error_count.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                    }
                    println!(
                        "Thread {}: Completed {} iterations in {:?}",
                        thread_id,
                        ITERATIONS_PER_THREAD,
                        start.elapsed()
                    );
                }
                Err(e) => {
                    eprintln!("Thread {}: Failed to open repository: {}", thread_id, e);
                    error_count.fetch_add(ITERATIONS_PER_THREAD, Ordering::SeqCst);
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    let total_successes = success_count.load(Ordering::SeqCst);
    let total_errors = error_count.load(Ordering::SeqCst);
    let expected_total = THREAD_COUNT * ITERATIONS_PER_THREAD;

    println!("\n=== Results ===");
    println!("Total operations: {}", expected_total);
    println!("Successes: {}", total_successes);
    println!("Errors: {}", total_errors);

    // Verify database integrity by reading all accounts
    let repo = DuckDbRepository::new(&db_path, None).unwrap();
    let accounts = repo.get_accounts().unwrap();
    println!("Accounts in database: {}", accounts.len());

    // All operations should succeed with proper locking
    assert_eq!(
        total_errors, 0,
        "Expected 0 errors but got {}. This indicates race conditions.",
        total_errors
    );
    assert_eq!(
        total_successes, expected_total,
        "Expected {} successful operations but got {}",
        expected_total, total_successes
    );
}

/// Test: Interleaved reads and writes from multiple repository instances
///
/// This simulates the app scenario where one command is syncing (writing)
/// while another command is reading accounts for display.
#[test]
fn test_concurrent_read_write_operations() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_read_write.duckdb");

    // Create initial database with some data
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
        for i in 0..10 {
            let account = create_test_account(&format!("initial_{}", i));
            repo.upsert_account(&account).unwrap();
        }
    }

    let barrier = Arc::new(Barrier::new(THREAD_COUNT));
    let db_path = Arc::new(db_path);
    let write_errors = Arc::new(AtomicUsize::new(0));
    let read_errors = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..THREAD_COUNT {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let write_errors = Arc::clone(&write_errors);
        let read_errors = Arc::clone(&read_errors);

        let handle = thread::spawn(move || {
            barrier.wait();

            // Each thread creates its own repository
            let repo = match DuckDbRepository::new(&db_path, None) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Thread {}: Failed to open: {}", thread_id, e);
                    return;
                }
            };

            for i in 0..ITERATIONS_PER_THREAD {
                // Alternate between reads and writes
                if i % 2 == 0 {
                    // Write operation
                    let account = create_test_account(&format!("rw_t{}_i{}", thread_id, i));
                    if let Err(e) = repo.upsert_account(&account) {
                        eprintln!("Thread {}: Write error: {}", thread_id, e);
                        write_errors.fetch_add(1, Ordering::SeqCst);
                    }
                } else {
                    // Read operation
                    if let Err(e) = repo.get_accounts() {
                        eprintln!("Thread {}: Read error: {}", thread_id, e);
                        read_errors.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let total_write_errors = write_errors.load(Ordering::SeqCst);
    let total_read_errors = read_errors.load(Ordering::SeqCst);

    println!("\n=== Read/Write Results ===");
    println!("Write errors: {}", total_write_errors);
    println!("Read errors: {}", total_read_errors);

    assert_eq!(
        total_write_errors, 0,
        "Write operations should not fail with proper locking"
    );
    assert_eq!(
        total_read_errors, 0,
        "Read operations should not fail with proper locking"
    );
}

/// Test: Rapid connection open/close cycles
///
/// This simulates the current Tauri pattern where every command
/// creates and destroys a TreelineContext.
#[test]
fn test_rapid_connection_churn() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_churn.duckdb");

    // Create initial database
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
    }

    let barrier = Arc::new(Barrier::new(THREAD_COUNT));
    let db_path = Arc::new(db_path);
    let error_count = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..THREAD_COUNT {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let error_count = Arc::clone(&error_count);

        let handle = thread::spawn(move || {
            barrier.wait();

            // Rapidly open, use, and close connections
            for i in 0..ITERATIONS_PER_THREAD {
                match DuckDbRepository::new(&db_path, None) {
                    Ok(repo) => {
                        // Do a quick operation
                        let account = create_test_account(&format!("churn_t{}_i{}", thread_id, i));
                        if let Err(e) = repo.upsert_account(&account) {
                            eprintln!("Thread {}: Operation error: {}", thread_id, e);
                            error_count.fetch_add(1, Ordering::SeqCst);
                        }
                        // Connection dropped here
                    }
                    Err(e) => {
                        eprintln!("Thread {}: Connection error: {}", thread_id, e);
                        error_count.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let total_errors = error_count.load(Ordering::SeqCst);
    println!("\n=== Connection Churn Results ===");
    println!("Errors: {}", total_errors);

    assert_eq!(
        total_errors, 0,
        "Rapid connection churn should not cause errors with proper locking"
    );
}

/// Test: Schema operations (migrations) concurrent with data operations
///
/// This simulates the startup race condition where migrations run
/// while sync tries to access the database.
#[test]
fn test_schema_operations_during_data_access() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_schema.duckdb");

    // Create database WITHOUT running migrations
    // (We'll run ensure_schema from multiple threads)
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
    }

    let barrier = Arc::new(Barrier::new(THREAD_COUNT));
    let db_path = Arc::new(db_path);
    let schema_errors = Arc::new(AtomicUsize::new(0));
    let data_errors = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..THREAD_COUNT {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let schema_errors = Arc::clone(&schema_errors);
        let data_errors = Arc::clone(&data_errors);

        let handle = thread::spawn(move || {
            barrier.wait();

            let repo = match DuckDbRepository::new(&db_path, None) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Thread {}: Open failed: {}", thread_id, e);
                    schema_errors.fetch_add(1, Ordering::SeqCst);
                    return;
                }
            };

            // Half the threads run migrations, half do data operations
            if thread_id % 2 == 0 {
                // Run migrations (should be idempotent)
                for _ in 0..3 {
                    if let Err(e) = repo.ensure_schema() {
                        eprintln!("Thread {}: Schema error: {}", thread_id, e);
                        schema_errors.fetch_add(1, Ordering::SeqCst);
                    }
                }
            } else {
                // Data operations
                for i in 0..ITERATIONS_PER_THREAD {
                    let account = create_test_account(&format!("schema_t{}_i{}", thread_id, i));
                    if let Err(e) = repo.upsert_account(&account) {
                        eprintln!("Thread {}: Data error: {}", thread_id, e);
                        data_errors.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let total_schema_errors = schema_errors.load(Ordering::SeqCst);
    let total_data_errors = data_errors.load(Ordering::SeqCst);

    println!("\n=== Schema Concurrent Results ===");
    println!("Schema errors: {}", total_schema_errors);
    println!("Data errors: {}", total_data_errors);

    assert_eq!(
        total_schema_errors, 0,
        "Schema operations should not fail with proper locking"
    );
    assert_eq!(
        total_data_errors, 0,
        "Data operations should not fail during schema operations"
    );
}

/// Test: High-contention write scenario
///
/// All threads write to overlapping data (same account IDs)
/// to maximize contention.
#[test]
fn test_high_contention_writes() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_contention.duckdb");

    // Create initial database
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
    }

    // Pre-generate account IDs that all threads will compete to update
    let shared_account_ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    let shared_account_ids = Arc::new(shared_account_ids);

    let barrier = Arc::new(Barrier::new(THREAD_COUNT));
    let db_path = Arc::new(db_path);
    let error_count = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..THREAD_COUNT {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let error_count = Arc::clone(&error_count);
        let shared_account_ids = Arc::clone(&shared_account_ids);

        let handle = thread::spawn(move || {
            barrier.wait();

            let repo = match DuckDbRepository::new(&db_path, None) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Thread {}: Open failed: {}", thread_id, e);
                    error_count.fetch_add(ITERATIONS_PER_THREAD, Ordering::SeqCst);
                    return;
                }
            };

            for i in 0..ITERATIONS_PER_THREAD {
                // All threads update the same set of accounts
                let account_id = shared_account_ids[i % shared_account_ids.len()];
                let mut account = Account::new(account_id, format!("Contested Account {}", i));
                account.id = account_id; // Use shared ID

                if let Err(e) = repo.upsert_account(&account) {
                    eprintln!("Thread {}: Contention error: {}", thread_id, e);
                    error_count.fetch_add(1, Ordering::SeqCst);
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let total_errors = error_count.load(Ordering::SeqCst);

    // Verify final state
    let repo = DuckDbRepository::new(&db_path, None).unwrap();
    let accounts = repo.get_accounts().unwrap();

    println!("\n=== High Contention Results ===");
    println!("Errors: {}", total_errors);
    println!("Final account count: {}", accounts.len());

    assert_eq!(
        total_errors, 0,
        "High-contention writes should not fail with proper locking"
    );

    // Should have exactly 5 accounts (the shared IDs)
    assert_eq!(
        accounts.len(),
        5,
        "Should have exactly 5 accounts after upserts"
    );
}

/// Test: Database integrity after concurrent operations
///
/// Runs many concurrent operations then verifies the database
/// is still readable and consistent.
#[test]
fn test_database_integrity_after_concurrent_ops() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_integrity.duckdb");

    // Create initial database
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
    }

    let barrier = Arc::new(Barrier::new(THREAD_COUNT));
    let db_path = Arc::new(db_path);

    let mut handles = vec![];

    for thread_id in 0..THREAD_COUNT {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);

        let handle = thread::spawn(move || {
            barrier.wait();

            // Each thread does various operations
            if let Ok(repo) = DuckDbRepository::new(&db_path, None) {
                for i in 0..ITERATIONS_PER_THREAD {
                    let account = create_test_account(&format!("integrity_t{}_i{}", thread_id, i));
                    let _ = repo.upsert_account(&account);
                    let _ = repo.get_accounts();
                    let _ = repo.get_accounts();
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Now verify database integrity
    println!("\n=== Integrity Check ===");

    let repo = DuckDbRepository::new(&db_path, None).unwrap();

    // Should be able to read accounts
    let accounts = repo.get_accounts();
    assert!(accounts.is_ok(), "Should be able to read accounts");

    let account_list = accounts.unwrap();
    println!("Accounts readable: {}", account_list.len());

    // Verify count is consistent
    let count = account_list.len();
    println!("Account count: {}", count);

    // Should be able to run a query
    let result = repo.execute_query("SELECT COUNT(*) FROM sys_accounts");
    assert!(
        result.is_ok(),
        "Should be able to execute queries after concurrent ops"
    );

    println!("Database integrity verified!");
}

/// Test: Simulates the exact startup race condition
///
/// One thread runs migrations while another attempts sync-like operations.
#[test]
fn test_startup_race_condition() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_startup.duckdb");

    // DON'T initialize the database - let threads race to do it
    let barrier = Arc::new(Barrier::new(2));
    let db_path = Arc::new(db_path);
    let migration_error = Arc::new(AtomicUsize::new(0));
    let sync_error = Arc::new(AtomicUsize::new(0));

    // Thread 1: Simulates migration at startup
    let migration_handle = {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let migration_error = Arc::clone(&migration_error);

        thread::spawn(move || {
            barrier.wait();

            println!("Migration thread: Starting...");
            match DuckDbRepository::new(&db_path, None) {
                Ok(repo) => {
                    if let Err(e) = repo.ensure_schema() {
                        eprintln!("Migration thread: Schema error: {}", e);
                        migration_error.fetch_add(1, Ordering::SeqCst);
                    } else {
                        println!("Migration thread: Schema created successfully");
                    }
                    // Hold connection briefly to simulate migration duration
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    eprintln!("Migration thread: Connection error: {}", e);
                    migration_error.fetch_add(1, Ordering::SeqCst);
                }
            }
        })
    };

    // Thread 2: Simulates sync starting immediately after
    let sync_handle = {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let sync_error = Arc::clone(&sync_error);

        thread::spawn(move || {
            barrier.wait();

            // Small delay to let migration start first (but not finish)
            thread::sleep(Duration::from_millis(10));

            println!("Sync thread: Starting...");
            match DuckDbRepository::new(&db_path, None) {
                Ok(repo) => {
                    // Try to do sync-like operations
                    match repo.get_accounts() {
                        Ok(accounts) => {
                            println!("Sync thread: Read {} accounts", accounts.len());
                        }
                        Err(e) => {
                            eprintln!("Sync thread: Read error: {}", e);
                            sync_error.fetch_add(1, Ordering::SeqCst);
                        }
                    }

                    // Try to write
                    let account = create_test_account("sync_test");
                    if let Err(e) = repo.upsert_account(&account) {
                        eprintln!("Sync thread: Write error: {}", e);
                        sync_error.fetch_add(1, Ordering::SeqCst);
                    }
                }
                Err(e) => {
                    eprintln!("Sync thread: Connection error: {}", e);
                    sync_error.fetch_add(1, Ordering::SeqCst);
                }
            }
        })
    };

    migration_handle.join().unwrap();
    sync_handle.join().unwrap();

    let migration_errors = migration_error.load(Ordering::SeqCst);
    let sync_errors = sync_error.load(Ordering::SeqCst);

    println!("\n=== Startup Race Results ===");
    println!("Migration errors: {}", migration_errors);
    println!("Sync errors: {}", sync_errors);

    assert_eq!(
        migration_errors, 0,
        "Migrations should complete without error"
    );
    assert_eq!(sync_errors, 0, "Sync should complete without error");
}

/// Test: Multiple iterations to catch intermittent failures
///
/// Runs the concurrent test multiple times to increase chances
/// of catching race conditions.
#[test]
fn test_stress_repeated_concurrent_access() {
    const STRESS_ITERATIONS: usize = 5;

    for iteration in 0..STRESS_ITERATIONS {
        println!("\n=== Stress Iteration {} ===", iteration + 1);

        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir
            .path()
            .join(format!("test_stress_{}.duckdb", iteration));

        // Initialize
        {
            let repo = DuckDbRepository::new(&db_path, None).unwrap();
            repo.ensure_schema().unwrap();
        }

        let barrier = Arc::new(Barrier::new(10));
        let db_path = Arc::new(db_path);
        let errors = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..10)
            .map(|thread_id| {
                let barrier = Arc::clone(&barrier);
                let db_path = Arc::clone(&db_path);
                let errors = Arc::clone(&errors);

                thread::spawn(move || {
                    barrier.wait();

                    if let Ok(repo) = DuckDbRepository::new(&db_path, None) {
                        for i in 0..5 {
                            let account = create_test_account(&format!(
                                "stress_{}_t{}_i{}",
                                iteration, thread_id, i
                            ));
                            if repo.upsert_account(&account).is_err() {
                                errors.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                    } else {
                        errors.fetch_add(5, Ordering::SeqCst);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let error_count = errors.load(Ordering::SeqCst);
        assert_eq!(
            error_count,
            0,
            "Stress iteration {} had {} errors",
            iteration + 1,
            error_count
        );
    }

    println!(
        "\n=== All {} stress iterations passed ===",
        STRESS_ITERATIONS
    );
}

/// Test: Heavy concurrent writes with bulk inserts
///
/// Multiple threads perform bulk write operations simultaneously.
/// This tests the lock under heavy write load.
#[test]
fn test_concurrent_bulk_writes() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_bulk_writes.duckdb");

    // Create initial database
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
    }

    let thread_count = 4;
    let writes_per_thread = 20;
    let expected_total = thread_count * writes_per_thread;

    let barrier = Arc::new(Barrier::new(thread_count));
    let db_path = Arc::new(db_path);
    let total_writes = Arc::new(AtomicUsize::new(0));
    let write_errors = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..thread_count {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let total_writes = Arc::clone(&total_writes);
        let write_errors = Arc::clone(&write_errors);

        let handle = thread::spawn(move || {
            barrier.wait();

            let repo = match DuckDbRepository::new(&db_path, None) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Thread {}: Failed to open: {}", thread_id, e);
                    write_errors.fetch_add(writes_per_thread, Ordering::SeqCst);
                    return;
                }
            };

            // Each thread writes accounts in rapid succession
            for i in 0..writes_per_thread {
                let account = create_test_account(&format!("bulk_t{}_i{}", thread_id, i));
                match repo.upsert_account(&account) {
                    Ok(_) => {
                        total_writes.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(e) => {
                        eprintln!("Thread {}: Write error at {}: {}", thread_id, i, e);
                        write_errors.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let total = total_writes.load(Ordering::SeqCst);
    let errors = write_errors.load(Ordering::SeqCst);

    // Verify final state
    let repo = DuckDbRepository::new(&db_path, None).unwrap();
    let accounts = repo.get_accounts().unwrap();

    println!("\n=== Bulk Write Results ===");
    println!("Total writes attempted: {}", expected_total);
    println!("Successful writes: {}", total);
    println!("Write errors: {}", errors);
    println!("Accounts in database: {}", accounts.len());

    assert_eq!(
        errors, 0,
        "No write errors should occur with proper locking"
    );
    assert_eq!(total, expected_total, "All writes should succeed");
    assert_eq!(
        accounts.len(),
        expected_total,
        "Database should contain all accounts"
    );
}

/// Test: Write-heavy vs read-heavy thread mix
///
/// Half the threads do heavy writes, half do heavy reads.
/// Simulates real-world usage where UI reads while sync writes.
#[test]
fn test_mixed_heavy_workload() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_mixed_workload.duckdb");

    let prepop_count = 20;
    let writer_count = 3;
    let reader_count = 3;
    let ops_per_thread = 10;

    // Create initial database with some data
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
        // Pre-populate with accounts
        for i in 0..prepop_count {
            let account = create_test_account(&format!("prepop_{}", i));
            repo.upsert_account(&account).unwrap();
        }
    }

    let barrier = Arc::new(Barrier::new(writer_count + reader_count));
    let db_path = Arc::new(db_path);
    let write_count = Arc::new(AtomicUsize::new(0));
    let read_count = Arc::new(AtomicUsize::new(0));
    let write_errors = Arc::new(AtomicUsize::new(0));
    let read_errors = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    // Spawn writer threads
    for thread_id in 0..writer_count {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let write_count = Arc::clone(&write_count);
        let write_errors = Arc::clone(&write_errors);

        let handle = thread::spawn(move || {
            barrier.wait();

            let repo = match DuckDbRepository::new(&db_path, None) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Writer {}: Failed to open: {}", thread_id, e);
                    write_errors.fetch_add(ops_per_thread, Ordering::SeqCst);
                    return;
                }
            };

            for i in 0..ops_per_thread {
                let account = create_test_account(&format!("writer{}_acc{}", thread_id, i));
                match repo.upsert_account(&account) {
                    Ok(_) => {
                        write_count.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(e) => {
                        eprintln!("Writer {}: Error at {}: {}", thread_id, i, e);
                        write_errors.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });

        handles.push(handle);
    }

    // Spawn reader threads
    for thread_id in 0..reader_count {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let read_count = Arc::clone(&read_count);
        let read_errors = Arc::clone(&read_errors);

        let handle = thread::spawn(move || {
            barrier.wait();

            let repo = match DuckDbRepository::new(&db_path, None) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Reader {}: Failed to open: {}", thread_id, e);
                    read_errors.fetch_add(ops_per_thread, Ordering::SeqCst);
                    return;
                }
            };

            for i in 0..ops_per_thread {
                match repo.get_accounts() {
                    Ok(accounts) => {
                        read_count.fetch_add(1, Ordering::SeqCst);
                        // Verify we can read the accounts (at least pre-populated ones)
                        assert!(
                            accounts.len() >= prepop_count,
                            "Should have at least {} pre-populated accounts, got {}",
                            prepop_count,
                            accounts.len()
                        );
                    }
                    Err(e) => {
                        eprintln!("Reader {}: Error at {}: {}", thread_id, i, e);
                        read_errors.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let writes = write_count.load(Ordering::SeqCst);
    let reads = read_count.load(Ordering::SeqCst);
    let w_errors = write_errors.load(Ordering::SeqCst);
    let r_errors = read_errors.load(Ordering::SeqCst);

    let expected_writes = writer_count * ops_per_thread;
    let expected_reads = reader_count * ops_per_thread;
    let expected_final = prepop_count + expected_writes;

    println!("\n=== Mixed Workload Results ===");
    println!("Successful writes: {} / {}", writes, expected_writes);
    println!("Successful reads: {} / {}", reads, expected_reads);
    println!("Write errors: {}", w_errors);
    println!("Read errors: {}", r_errors);

    // Verify final state
    let repo = DuckDbRepository::new(&db_path, None).unwrap();
    let final_accounts = repo.get_accounts().unwrap();
    println!("Final account count: {}", final_accounts.len());

    assert_eq!(w_errors, 0, "No write errors should occur");
    assert_eq!(r_errors, 0, "No read errors should occur");
    assert_eq!(writes, expected_writes, "All writes should succeed");
    assert_eq!(reads, expected_reads, "All reads should succeed");
    assert_eq!(
        final_accounts.len(),
        expected_final,
        "Should have correct account total"
    );
}

/// Test: Rapid create-use-drop cycle (simulating Tauri commands)
///
/// Each operation opens a new connection, does work, closes it.
/// This is exactly how the Tauri app currently works.
#[test]
fn test_rapid_open_write_close_cycle() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_rapid_cycle.duckdb");

    let thread_count = 4;
    let cmds_per_thread = 8;
    let expected_total = thread_count * cmds_per_thread;

    // Create initial database
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
    }

    let barrier = Arc::new(Barrier::new(thread_count));
    let db_path = Arc::new(db_path);
    let success_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..thread_count {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);
        let success_count = Arc::clone(&success_count);
        let error_count = Arc::clone(&error_count);

        let handle = thread::spawn(move || {
            barrier.wait();

            // Each "command" is: open connection, write, close
            for cmd in 0..cmds_per_thread {
                match DuckDbRepository::new(&db_path, None) {
                    Ok(repo) => {
                        let account =
                            create_test_account(&format!("rapid_t{}_cmd{}", thread_id, cmd));
                        match repo.upsert_account(&account) {
                            Ok(_) => {
                                success_count.fetch_add(1, Ordering::SeqCst);
                            }
                            Err(e) => {
                                eprintln!("Thread {} cmd {}: Write error: {}", thread_id, cmd, e);
                                error_count.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                        // Connection (and lock) dropped here
                    }
                    Err(e) => {
                        eprintln!("Thread {} cmd {}: Open error: {}", thread_id, cmd, e);
                        error_count.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let successes = success_count.load(Ordering::SeqCst);
    let errors = error_count.load(Ordering::SeqCst);

    // Verify
    let repo = DuckDbRepository::new(&db_path, None).unwrap();
    let accounts = repo.get_accounts().unwrap();

    println!("\n=== Rapid Cycle Results ===");
    println!("Total commands: {}", expected_total);
    println!("Successful: {}", successes);
    println!("Errors: {}", errors);
    println!("Accounts in database: {}", accounts.len());

    assert_eq!(errors, 0, "No errors should occur with proper locking");
    assert_eq!(successes, expected_total, "All commands should succeed");
    assert_eq!(
        accounts.len(),
        expected_total,
        "Should have correct account count"
    );
}
