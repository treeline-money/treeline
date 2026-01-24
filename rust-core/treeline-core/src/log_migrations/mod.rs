//! Log database migrations - embedded SQL files
//!
//! Migrations are compiled into the binary at build time using include_str!.
//! Each migration is a tuple of (name, sql_content).
//! Migrations are sorted by name and applied in order.

/// All log migrations, embedded at compile time.
/// Format: (filename, sql_content)
///
/// IMPORTANT: When adding a new migration:
/// 1. Create the SQL file: NNN_description.sql
/// 2. Add an entry here in order
pub const LOG_MIGRATIONS: &[(&str, &str)] = &[
    ("000_migrations.sql", include_str!("000_migrations.sql")),
    (
        "001_initial_schema.sql",
        include_str!("001_initial_schema.sql"),
    ),
];
