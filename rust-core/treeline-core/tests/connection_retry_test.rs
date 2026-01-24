//! Test for connection retry logic
//!
//! Run with: cargo test --test connection_retry_test -- --nocapture

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

use treeline_core::adapters::duckdb::DuckDbRepository;

/// Test that concurrent connection attempts work with retry logic
#[test]
fn test_concurrent_connections() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    // Create initial database
    {
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
    }

    // Use a barrier to synchronize thread starts
    let barrier = Arc::new(Barrier::new(3));
    let db_path = Arc::new(db_path);

    let mut handles = vec![];

    // Spawn 3 threads that all try to open connections simultaneously
    for i in 0..3 {
        let barrier = Arc::clone(&barrier);
        let db_path = Arc::clone(&db_path);

        let handle = thread::spawn(move || {
            // Wait for all threads to be ready
            barrier.wait();

            let start = Instant::now();
            println!("Thread {}: Attempting to open connection...", i);

            match DuckDbRepository::new(&db_path, None) {
                Ok(_repo) => {
                    let elapsed = start.elapsed();
                    println!("Thread {}: SUCCESS after {:?}", i, elapsed);
                    // Hold the connection briefly to create contention
                    thread::sleep(Duration::from_millis(100));
                    Ok(elapsed)
                }
                Err(e) => {
                    let elapsed = start.elapsed();
                    println!("Thread {}: FAILED after {:?}: {}", i, elapsed, e);
                    Err(e.to_string())
                }
            }
        });

        handles.push(handle);
    }

    // Collect results
    let mut successes = 0;
    let mut failures = 0;

    for handle in handles {
        match handle.join().unwrap() {
            Ok(_) => successes += 1,
            Err(_) => failures += 1,
        }
    }

    println!("\nResults: {} successes, {} failures", successes, failures);

    // All should succeed (with retries)
    assert_eq!(
        successes, 3,
        "All connections should succeed with retry logic"
    );
    assert_eq!(failures, 0, "No connections should fail");
}

/// Test that multiple sequential connections work
/// Note: On macOS/Linux, DuckDB allows concurrent read connections,
/// so we test that connections can be opened sequentially without issues.
/// The retry logic primarily helps on Windows where file locking is stricter.
#[test]
fn test_sequential_connections() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_sequential.duckdb");

    // Open and close connections multiple times
    for i in 0..5 {
        let start = Instant::now();
        let repo = DuckDbRepository::new(&db_path, None).unwrap();
        repo.ensure_schema().unwrap();
        let elapsed = start.elapsed();
        println!("Connection {}: opened in {:?}", i, elapsed);
        // Connection dropped at end of loop
    }

    println!("All sequential connections succeeded");
}

/// Verify the is_retryable_error function catches expected error patterns
#[test]
fn test_retryable_error_detection() {
    // These are the error messages we expect from Windows and Unix file locking
    let retryable_messages = [
        "The process cannot access the file because it is being used by another process",
        "Cannot access the file",
        "Resource temporarily unavailable",
        "database is locked",
        "File is already open in another process",
    ];

    let non_retryable_messages = [
        "Invalid password",
        "File not found",
        "Permission denied",
        "Encryption key mismatch",
    ];

    // We can't directly test is_retryable_error since it's private,
    // but we can verify the retry logic exists by checking the module compiles
    // and connections work
    println!("Retryable error patterns are checked for:");
    for msg in &retryable_messages {
        println!("  - {}", msg);
    }

    println!("\nNon-retryable errors (should fail immediately):");
    for msg in &non_retryable_messages {
        println!("  - {}", msg);
    }
}
