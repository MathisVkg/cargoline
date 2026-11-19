//! Location catalog helpers (table `locations`), kept for the 0002 migration
//! hook and existing callers. General catalog logic lives in [`crate::catalog`].

use rusqlite::{params, Connection, OptionalExtension};

use crate::catalog::{self, CatalogTable};
pub use crate::catalog::{normalize_display, normalize_key, CatalogEntry};

/// Canonical spelling of a location name, adding it as a user entry if new.
pub fn resolve_or_insert(conn: &Connection, name: &str, now_ms: i64) -> rusqlite::Result<String> {
    catalog::resolve_or_insert(conn, CatalogTable::Locations, name, now_ms)
}

/// Active catalog locations, alphabetical.
pub fn list_locations(conn: &Connection) -> rusqlite::Result<Vec<CatalogEntry>> {
    catalog::list_entries(conn, CatalogTable::Locations)
}

/// Active catalog locations containing `query`, prefix matches first.
pub fn search_locations(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> rusqlite::Result<Vec<CatalogEntry>> {
    catalog::search_entries(conn, CatalogTable::Locations, query, limit)
}

/// Migration 0002 hook: seeds the catalog from names already used by stops
/// (oldest spelling wins). Uses only the columns that exist in 0002.
pub fn populate_from_stops(conn: &Connection, now_ms: i64) -> rusqlite::Result<usize> {
    let names: Vec<String> = conn
        .prepare("SELECT location_name FROM cargo_stops ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut added = 0;
    for name in names {
        let display = normalize_display(&name);
        if display.is_empty() {
            continue;
        }
        let key = display.to_lowercase();
        let exists = conn
            .query_row(
                "SELECT 1 FROM locations WHERE normalized_name = ?1",
                [&key],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            conn.execute(
                "INSERT INTO locations (display_name, normalized_name, created_at) VALUES (?1, ?2, ?3)",
                params![display, key, now_ms],
            )?;
            added += 1;
        }
    }
    Ok(added)
}
