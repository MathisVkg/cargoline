//! Forward-only schema migrations, managed by `rusqlite_migration`.
//!
//! The schema version is tracked in SQLite's `user_version` header field.
//! All pending migrations are applied inside a single transaction, so a
//! failure leaves the database exactly as it was.
//!
//! Rules: never edit or reorder an applied migration; append a new numbered
//! SQL file and a new entry to [`migrations`], then bump
//! [`LATEST_SCHEMA_VERSION`].

use rusqlite::Transaction;
use rusqlite_migration::{HookError, HookResult, Migrations, M};

/// The schema version produced by applying every known migration.
pub const LATEST_SCHEMA_VERSION: i64 = 2;

/// All schema migrations, in application order (index + 1 = version).
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("0001_initial_schema.sql")),
        M::up_with_hook(
            include_str!("0002_location_catalog.sql"),
            seed_location_catalog,
        ),
    ])
}

/// Runs in the 0002 migration transaction after its SQL.
fn seed_location_catalog(tx: &Transaction) -> HookResult {
    crate::locations::populate_from_stops(tx, crate::database::now_utc_ms())
        .map(|_| ())
        .map_err(HookError::RusqliteError)
}
