//! Reference catalogs (locations, commodities): reusable names for the Add
//! Contract pickers. Rows are suggestions only; contracts store plain names.
//!
//! - `bundled` rows come from the offline community snapshot ([`snapshot`]),
//!   synced at startup ([`sync`]).
//! - `user` rows are names the user typed when saving a contract.
//!
//! Rows are never deleted. Retired bundled rows (gone from the snapshot) are
//! hidden from suggestions but still resolve when typed exactly.

pub mod snapshot;
pub mod sync;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Trims and collapses inner whitespace, keeping the capitalisation.
pub fn normalize_display(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Case- and whitespace-insensitive identity of a catalog name.
pub fn normalize_key(name: &str) -> String {
    normalize_display(name).to_lowercase()
}

/// The two catalog tables. Table names are fixed strings, never user input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogTable {
    Locations,
    Commodities,
}

impl CatalogTable {
    pub const fn table(self) -> &'static str {
        match self {
            Self::Locations => "locations",
            Self::Commodities => "commodities",
        }
    }
}

/// A suggestion row as sent to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: i64,
    pub name: String,
    pub kind: Option<String>,
    /// Parent planet or moon (locations only).
    pub body: Option<String>,
    /// Star system (locations only).
    pub system: Option<String>,
    /// `bundled` or `user`.
    pub source: String,
    pub seen_in_hauling: bool,
}

/// Returns the canonical spelling of `name`, inserting it as a `user` entry
/// if new. A retired entry typed exactly is reactivated as a `user` entry.
/// Call inside the transaction that uses the name, so a rollback also
/// removes any entry created here.
pub fn resolve_or_insert(
    conn: &Connection,
    table: CatalogTable,
    name: &str,
    now_ms: i64,
) -> rusqlite::Result<String> {
    let display = normalize_display(name);
    let key = display.to_lowercase();
    let t = table.table();
    let existing: Option<(String, Option<i64>)> = conn
        .prepare_cached(&format!(
            "SELECT display_name, retired_at FROM {t} WHERE normalized_name = ?1"
        ))?
        .query_row([&key], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    match existing {
        Some((canonical, None)) => Ok(canonical),
        Some((canonical, Some(_))) => {
            conn.prepare_cached(&format!(
                "UPDATE {t} SET retired_at = NULL, source = 'user' WHERE normalized_name = ?1"
            ))?
            .execute([&key])?;
            Ok(canonical)
        }
        None => {
            conn.prepare_cached(&format!(
                "INSERT INTO {t} (display_name, normalized_name, source, created_at) VALUES (?1, ?2, 'user', ?3)"
            ))?
            .execute(params![display, key, now_ms])?;
            Ok(display)
        }
    }
}

fn map_entry(table: CatalogTable) -> impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<CatalogEntry> {
    move |r| {
        let has_place = table == CatalogTable::Locations;
        Ok(CatalogEntry {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            body: if has_place { r.get(3)? } else { None },
            system: if has_place { r.get(4)? } else { None },
            source: r.get(5)?,
            seen_in_hauling: r.get(6)?,
        })
    }
}

fn select_columns(table: CatalogTable) -> &'static str {
    match table {
        CatalogTable::Locations => {
            "id, display_name, kind, body_name, system_name, source, seen_in_hauling"
        }
        CatalogTable::Commodities => "id, display_name, kind, NULL, NULL, source, seen_in_hauling",
    }
}

/// Active (non-retired) entries, alphabetical by normalized name.
pub fn list_entries(conn: &Connection, table: CatalogTable) -> rusqlite::Result<Vec<CatalogEntry>> {
    let sql = format!(
        "SELECT {} FROM {} WHERE retired_at IS NULL ORDER BY normalized_name, id",
        select_columns(table),
        table.table()
    );
    conn.prepare_cached(&sql)?
        .query_map([], map_entry(table))?
        .collect()
}

/// Active entries whose name contains `query` (case/whitespace-insensitive),
/// prefix matches first.
pub fn search_entries(
    conn: &Connection,
    table: CatalogTable,
    query: &str,
    limit: u32,
) -> rusqlite::Result<Vec<CatalogEntry>> {
    let key = normalize_key(query);
    let escaped = key
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let sql = format!(
        "SELECT {} FROM {} WHERE retired_at IS NULL AND normalized_name LIKE '%' || ?1 || '%' ESCAPE '\\'
         ORDER BY (substr(normalized_name, 1, length(?2)) <> ?2), normalized_name, id LIMIT ?3",
        select_columns(table),
        table.table()
    );
    conn.prepare_cached(&sql)?
        .query_map(params![escaped, key, limit], map_entry(table))?
        .collect()
}

/// Migration 0003 hook: seeds `commodities` with user entries from the
/// distinct names already used by cargo requirements (oldest spelling wins).
pub fn seed_commodities_from_requirements(
    conn: &Connection,
    now_ms: i64,
) -> rusqlite::Result<usize> {
    let names: Vec<String> = conn
        .prepare("SELECT commodity_name FROM cargo_requirements ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut insert = conn.prepare(
        "INSERT INTO commodities (display_name, normalized_name, source, created_at) VALUES (?1, ?2, 'user', ?3)
         ON CONFLICT (normalized_name) DO NOTHING",
    )?;
    let mut added = 0;
    for name in names {
        let display = normalize_display(&name);
        if !display.is_empty() {
            added += insert.execute(params![display, display.to_lowercase(), now_ms])?;
        }
    }
    Ok(added)
}

/// Provenance and counts of the reference catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceCatalogInfo {
    /// Version recorded by the last successful sync (None if never synced).
    pub synced_version: Option<String>,
    /// Version embedded in this build.
    pub bundled_version: Option<String>,
    pub source_name: Option<String>,
    pub source_url: Option<String>,
    pub game_version: Option<String>,
    pub fetched_on: Option<String>,
    pub attribution: Option<String>,
    pub locations: CatalogCounts,
    pub commodities: CatalogCounts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCounts {
    pub bundled: i64,
    pub user: i64,
    pub retired: i64,
}

fn counts(conn: &Connection, table: CatalogTable) -> rusqlite::Result<CatalogCounts> {
    conn.query_row(
        &format!(
            "SELECT
               COUNT(*) FILTER (WHERE source = 'bundled' AND retired_at IS NULL),
               COUNT(*) FILTER (WHERE source = 'user' AND retired_at IS NULL),
               COUNT(*) FILTER (WHERE retired_at IS NOT NULL)
             FROM {}",
            table.table()
        ),
        [],
        |r| {
            Ok(CatalogCounts {
                bundled: r.get(0)?,
                user: r.get(1)?,
                retired: r.get(2)?,
            })
        },
    )
}

/// Recorded sync metadata, falling back to the embedded snapshot's
/// provenance when no sync has succeeded yet.
pub fn reference_catalog_info(conn: &Connection) -> rusqlite::Result<ReferenceCatalogInfo> {
    let embedded = snapshot::Snapshot::embedded().ok();
    let recorded = conn
        .query_row(
            "SELECT catalog_version, game_version, source_name, source_url, fetched_on, attribution
             FROM catalog_metadata WHERE catalog = ?1",
            [sync::REFERENCE_CATALOG],
            |r| {
                Ok(ReferenceCatalogInfo {
                    synced_version: r.get(0)?,
                    bundled_version: None,
                    game_version: r.get(1)?,
                    source_name: r.get(2)?,
                    source_url: r.get(3)?,
                    fetched_on: r.get(4)?,
                    attribution: r.get(5)?,
                    locations: CatalogCounts::default(),
                    commodities: CatalogCounts::default(),
                })
            },
        )
        .optional()?;
    let e = embedded.as_ref();
    let mut info = recorded.unwrap_or_else(|| ReferenceCatalogInfo {
        synced_version: None,
        bundled_version: None,
        game_version: e.map(|s| s.game_version.clone()),
        source_name: e.map(|s| s.source.name.clone()),
        source_url: e.map(|s| s.source.url.clone()),
        fetched_on: e.map(|s| s.fetched_at.clone()),
        attribution: e.map(|s| s.attribution.clone()),
        locations: CatalogCounts::default(),
        commodities: CatalogCounts::default(),
    });
    info.bundled_version = e.map(|s| s.catalog_version.clone());
    info.locations = counts(conn, CatalogTable::Locations)?;
    info.commodities = counts(conn, CatalogTable::Commodities)?;
    Ok(info)
}
