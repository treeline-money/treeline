//! Diagnostic test to determine if Linux encryption failure is caused by
//! enable_autoload_extension(false) or a deeper bundled build issue.
//!
//! Run with: cargo test --test aaa_encryption_diagnostic -- --nocapture

use duckdb::Connection;
use tempfile::TempDir;

/// Test 1: Try encryption with autoloading DISABLED (current behavior)
#[test]
fn test_encrypt_autoload_disabled() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_no_autoload.duckdb");

    let config = duckdb::Config::default()
        .enable_autoload_extension(false)
        .unwrap();
    let conn = Connection::open_in_memory_with_flags(config).unwrap();

    let result = conn.execute(
        &format!(
            "ATTACH '{}' AS enc (ENCRYPTION_KEY '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef')",
            db_path.display()
        ),
        [],
    );

    match &result {
        Ok(_) => println!("AUTOLOAD=false: encryption ATTACH succeeded"),
        Err(e) => println!("AUTOLOAD=false: encryption ATTACH failed: {e}"),
    }

    // Don't assert — we want to see the output on all platforms
    let autoload_disabled_works = result.is_ok();

    // Test 2: Try with autoloading ENABLED
    let temp_dir2 = TempDir::new().unwrap();
    let db_path2 = temp_dir2.path().join("test_with_autoload.duckdb");

    let config2 = duckdb::Config::default()
        .enable_autoload_extension(true)
        .unwrap();
    let conn2 = Connection::open_in_memory_with_flags(config2).unwrap();

    let result2 = conn2.execute(
        &format!(
            "ATTACH '{}' AS enc (ENCRYPTION_KEY '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef')",
            db_path2.display()
        ),
        [],
    );

    match &result2 {
        Ok(_) => println!("AUTOLOAD=true: encryption ATTACH succeeded"),
        Err(e) => println!("AUTOLOAD=true: encryption ATTACH failed: {e}"),
    }

    let autoload_enabled_works = result2.is_ok();

    // Test 3: Try with autoload disabled BUT explicitly install+load httpfs first
    let temp_dir3 = TempDir::new().unwrap();
    let db_path3 = temp_dir3.path().join("test_explicit_httpfs.duckdb");

    let config3 = duckdb::Config::default()
        .enable_autoload_extension(false)
        .unwrap();
    let conn3 = Connection::open_in_memory_with_flags(config3).unwrap();

    // Explicitly install and load httpfs
    let install_result = conn3.execute_batch("INSTALL httpfs; LOAD httpfs;");
    match &install_result {
        Ok(_) => println!("Explicit INSTALL+LOAD httpfs: succeeded"),
        Err(e) => println!("Explicit INSTALL+LOAD httpfs: failed: {e}"),
    }

    let result3 = conn3.execute(
        &format!(
            "ATTACH '{}' AS enc (ENCRYPTION_KEY '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef')",
            db_path3.display()
        ),
        [],
    );

    match &result3 {
        Ok(_) => println!("AUTOLOAD=false + explicit httpfs: encryption ATTACH succeeded"),
        Err(e) => println!("AUTOLOAD=false + explicit httpfs: encryption ATTACH failed: {e}"),
    }

    let explicit_httpfs_works = result3.is_ok();

    // Summary
    println!("\n=== ENCRYPTION DIAGNOSTIC SUMMARY ===");
    println!("Platform: {}", std::env::consts::OS);
    println!("Arch: {}", std::env::consts::ARCH);
    println!(
        "Autoload disabled:              {}",
        if autoload_disabled_works {
            "WORKS"
        } else {
            "BROKEN"
        }
    );
    println!(
        "Autoload enabled:               {}",
        if autoload_enabled_works {
            "WORKS"
        } else {
            "BROKEN"
        }
    );
    println!(
        "Explicit INSTALL+LOAD httpfs:    {}",
        if explicit_httpfs_works {
            "WORKS"
        } else {
            "BROKEN"
        }
    );

    if !autoload_disabled_works && explicit_httpfs_works {
        println!("DIAGNOSIS: Fix is to explicitly INSTALL+LOAD httpfs before encryption");
    } else if !autoload_disabled_works && autoload_enabled_works {
        println!("DIAGNOSIS: enable_autoload_extension(false) is the cause");
    } else if !autoload_disabled_works && !autoload_enabled_works {
        println!("DIAGNOSIS: Bundled DuckDB build lacks encryption support entirely");
    } else {
        println!("DIAGNOSIS: Encryption works (both modes)");
    }

    // Assert at least one works, otherwise encryption is fundamentally broken
    assert!(
        autoload_disabled_works || autoload_enabled_works || explicit_httpfs_works,
        "Encryption is completely broken on this platform"
    );
}
