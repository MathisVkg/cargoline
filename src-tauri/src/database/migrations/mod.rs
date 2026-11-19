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
pub const LATEST_SCHEMA_VERSION: i64 = 3;

/// All schema migrations, in application order (index + 1 = version).
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("0001_initial_schema.sql")),
        M::up_with_hook(
            include_str!("0002_location_catalog.sql"),
            seed_location_catalog,
        ),
        M::up_with_hook(
            include_str!("0003_reference_catalogs.sql"),
            seed_commodity_catalog,
        ),
    ])
}

/// Runs in the 0002 migration transaction after its SQL.
fn seed_location_catalog(tx: &Transaction) -> HookResult {
    crate::locations::populate_from_stops(tx, crate::database::now_utc_ms())
        .map(|_| ())
        .map_err(HookError::RusqliteError)
}

/// Runs in the 0003 migration transaction after its SQL: seeds the
/// `commodities` table with the distinct names already used by cargo
/// requirements (oldest spelling wins). Migration 0003 is applied on existing
/// installs and must stay unchanged; the app does not read these tables.
fn seed_commodity_catalog(tx: &Transaction) -> HookResult {
    seed_commodities(tx, crate::database::now_utc_ms()).map_err(HookError::RusqliteError)
}

fn seed_commodities(conn: &Transaction, now_ms: i64) -> rusqlite::Result<()> {
    let names: Vec<String> = conn
        .prepare("SELECT commodity_name FROM cargo_requirements ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut insert = conn.prepare(
        "INSERT INTO commodities (display_name, normalized_name, source, created_at) VALUES (?1, ?2, 'user', ?3)
         ON CONFLICT (normalized_name) DO NOTHING",
    )?;
    for name in names {
        let display = crate::locations::normalize_display(&name);
        if !display.is_empty() {
            insert.execute(rusqlite::params![display, display.to_lowercase(), now_ms])?;
        }
    }
    Ok(())
}
