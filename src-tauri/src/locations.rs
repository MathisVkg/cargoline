//! Local catalog of reusable location names typed by the user (table `locations`).
//!
//! Stops store their own location text; the catalog only offers names for
//! selection and gives every name one canonical spelling.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Trims and collapses inner whitespace, keeping the user's capitalisation.
pub fn normalize_display(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Case- and whitespace-insensitive identity of a location name.
pub fn normalize_key(name: &str) -> String {
    normalize_display(name).to_lowercase()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationEntry {
    pub id: i64,
    pub name: String,
}

/// Returns the canonical spelling of `name`, adding it to the catalog first
/// if it is new. Call inside the transaction that uses the name, so an
/// aborted save leaves no unused catalog entry behind.
pub fn resolve_or_insert(conn: &Connection, name: &str, now_ms: i64) -> rusqlite::Result<String> {
    let display = normalize_display(name);
    let key = display.to_lowercase();
    conn.prepare_cached(
        "INSERT INTO locations (display_name, normalized_name, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (normalized_name) DO NOTHING",
    )?
    .execute(params![display, key, now_ms])?;
    conn.prepare_cached("SELECT display_name FROM locations WHERE normalized_name = ?1")?
        .query_row([key], |r| r.get(0))
}

/// Every catalog location, alphabetical.
pub fn list_locations(conn: &Connection) -> rusqlite::Result<Vec<LocationEntry>> {
    conn.prepare_cached("SELECT id, display_name FROM locations ORDER BY normalized_name, id")?
        .query_map([], |r| {
            Ok(LocationEntry {
                id: r.get(0)?,
                name: r.get(1)?,
            })
        })?
        .collect()
}

/// Catalog locations whose name contains `query` (case/whitespace-insensitive),
/// names starting with the query first.
pub fn search_locations(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> rusqlite::Result<Vec<LocationEntry>> {
    let key = normalize_key(query);
    let escaped = key
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    conn.prepare_cached(
        "SELECT id, display_name FROM locations
         WHERE normalized_name LIKE '%' || ?1 || '%' ESCAPE '\\'
         ORDER BY (substr(normalized_name, 1, length(?2)) <> ?2), normalized_name, id
         LIMIT ?3",
    )?
    .query_map(params![escaped, key, limit], |r| {
        Ok(LocationEntry {
            id: r.get(0)?,
            name: r.get(1)?,
        })
    })?
    .collect()
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

/// Distinct commodity names already used in saved contracts (oldest spelling
/// per case/whitespace variant), alphabetical. Read-only; no catalog table.
pub fn list_used_commodities(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let names: Vec<String> = conn
        .prepare_cached("SELECT commodity_name FROM cargo_requirements ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<String> = names
        .into_iter()
        .map(|n| normalize_display(&n))
        .filter(|n| !n.is_empty() && seen.insert(n.to_lowercase()))
        .collect();
    unique.sort_by_key(|n| n.to_lowercase());
    Ok(unique)
}
