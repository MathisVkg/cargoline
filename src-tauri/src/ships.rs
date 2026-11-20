//! The bundled ship catalog (`resources/catalog/ships.json`, Star Citizen Wiki API) and the
//! user's favourites and selected ship.
//!
//! Storage (Task 11, option A): the Task 02 tables fit. `ship_favorites` has a foreign key to
//! `ships`, so the JSON is upserted into `ships` at startup (idempotent, one transaction) and its
//! version recorded in `catalog_metadata`. Ships that leave the list are **never deleted** (a
//! favourite would cascade away): they are marked `availability_status = 'removed'` and hidden.
//! The selection is the existing typed setting `selected_ship_id`.
//!
//! The ship is informational only: no contract or run ever references it.

use std::collections::HashSet;
use std::sync::OnceLock;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::reference::{parse_list, Named, ReferenceList};

pub const SHIPS_JSON: &str = include_str!("../resources/catalog/ships.json");
const LISTED: &str = "listed";
const REMOVED: &str = "removed";

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogShip {
    /// Stable id: the game's class name (e.g. `CRUS_Starlifter_C2`).
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub manufacturer_code: String,
    /// Whole SCU.
    pub cargo_scu: u32,
    /// Largest container the ship's grids accept, when known.
    pub max_container_scu: Option<u32>,
}

impl Named for CatalogShip {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Parses and checks the list: the shared rules (names non-empty, unique, sorted) plus unique
/// non-empty ids and a capacity above 0.
pub fn parse_ships(json: &str) -> Result<ReferenceList<CatalogShip>, String> {
    let list: ReferenceList<CatalogShip> = parse_list(json)?;
    let mut ids = HashSet::new();
    for ship in &list.items {
        if ship.id.trim().is_empty() || ship.id.trim() != ship.id {
            return Err(format!("empty or untrimmed id for {:?}", ship.name));
        }
        if !ids.insert(ship.id.as_str()) {
            return Err(format!("duplicate id {:?}", ship.id));
        }
        if ship.cargo_scu == 0 {
            return Err(format!("{:?} has no cargo capacity", ship.name));
        }
        if ship.manufacturer.trim().is_empty() {
            return Err(format!("{:?} has no manufacturer", ship.name));
        }
        if ship.max_container_scu == Some(0) {
            return Err(format!("{:?} has a 0 SCU max container", ship.name));
        }
    }
    Ok(list)
}

static SHIPS: OnceLock<Result<ReferenceList<CatalogShip>, String>> = OnceLock::new();

pub fn bundled() -> Result<&'static ReferenceList<CatalogShip>, String> {
    SHIPS
        .get_or_init(|| parse_ships(SHIPS_JSON))
        .as_ref()
        .map_err(Clone::clone)
}

/// The catalog version recorded in `catalog_metadata` (game build + generation date).
pub fn catalog_version(list: &ReferenceList<CatalogShip>) -> String {
    format!("{} ({})", list.game_version, list.generated_at)
}

/// What a sync did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub upserted: usize,
    pub removed: usize,
    pub version_changed: bool,
}

/// Upserts the list into `ships` in one transaction. Idempotent: running it again changes nothing.
/// Rows missing from the list are marked removed, never deleted.
pub fn sync_catalog(
    conn: &mut Connection,
    list: &ReferenceList<CatalogShip>,
    now_ms: i64,
) -> rusqlite::Result<SyncReport> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut report = SyncReport::default();
    {
        let mut upsert = tx.prepare_cached(
            "INSERT INTO ships (id, manufacturer, name, variant, cargo_capacity_scu_milli,
                                max_box_size_scu_milli, source_url, verified_at, availability_status)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, NULL, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET
                manufacturer = excluded.manufacturer, name = excluded.name,
                cargo_capacity_scu_milli = excluded.cargo_capacity_scu_milli,
                max_box_size_scu_milli = excluded.max_box_size_scu_milli,
                verified_at = excluded.verified_at, availability_status = excluded.availability_status
             WHERE (manufacturer, name, cargo_capacity_scu_milli, max_box_size_scu_milli, verified_at, availability_status)
                IS NOT (excluded.manufacturer, excluded.name, excluded.cargo_capacity_scu_milli,
                        excluded.max_box_size_scu_milli, excluded.verified_at, excluded.availability_status)",
        )?;
        for ship in &list.items {
            report.upserted += upsert.execute(params![
                ship.id,
                ship.manufacturer,
                ship.name,
                i64::from(ship.cargo_scu) * 1000,
                ship.max_container_scu.map(|m| i64::from(m) * 1000),
                list.generated_at,
                LISTED
            ])?;
        }
    }
    let ids: HashSet<&str> = list.items.iter().map(|s| s.id.as_str()).collect();
    let known: Vec<String> = tx
        .prepare("SELECT id FROM ships WHERE availability_status IS NOT ?1")?
        .query_map([REMOVED], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in known.iter().filter(|id| !ids.contains(id.as_str())) {
        report.removed += tx.execute(
            "UPDATE ships SET availability_status = ?2 WHERE id = ?1",
            params![id, REMOVED],
        )?;
    }
    let version = catalog_version(list);
    report.version_changed = tx.execute(
        "INSERT INTO catalog_metadata (id, catalog_version, published_on, source_summary, imported_at)
         VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT (id) DO UPDATE SET catalog_version = excluded.catalog_version,
             published_on = excluded.published_on, source_summary = excluded.source_summary,
             imported_at = excluded.imported_at
         WHERE catalog_version IS NOT excluded.catalog_version",
        params![version, list.generated_at, list.source, now_ms],
    )? == 1;
    tx.commit()?;
    Ok(report)
}

/// A selectable ship, with the user's favourite flag.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShipEntry {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub manufacturer_code: String,
    pub cargo_scu: u32,
    pub max_container_scu: Option<u32>,
    pub is_favorite: bool,
}

const SELECT_SHIP: &str =
    "SELECT s.id, s.name, s.manufacturer, s.cargo_capacity_scu_milli, s.max_box_size_scu_milli,
        f.ship_id IS NOT NULL
     FROM ships s LEFT JOIN ship_favorites f ON f.ship_id = s.id
     WHERE s.availability_status = 'listed'";

fn entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ShipEntry> {
    let id: String = row.get(0)?;
    let code = bundled()
        .ok()
        .and_then(|l| {
            l.items
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.manufacturer_code.clone())
        })
        .unwrap_or_default();
    Ok(ShipEntry {
        name: row.get(1)?,
        manufacturer: row.get(2)?,
        manufacturer_code: code,
        cargo_scu: (row.get::<_, i64>(3)? / 1000) as u32,
        max_container_scu: row.get::<_, Option<i64>>(4)?.map(|m| (m / 1000) as u32),
        is_favorite: row.get(5)?,
        id,
    })
}

/// Listed ships, alphabetical, each with its favourite flag.
pub fn list_ships(conn: &Connection) -> rusqlite::Result<Vec<ShipEntry>> {
    conn.prepare(&format!("{SELECT_SHIP} ORDER BY s.name COLLATE NOCASE"))?
        .query_map([], entry)?
        .collect()
}

pub fn listed_ship(conn: &Connection, id: &str) -> rusqlite::Result<Option<ShipEntry>> {
    conn.query_row(&format!("{SELECT_SHIP} AND s.id = ?1"), [id], entry)
        .optional()
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShipError {
    #[error("Unknown ship: {0}")]
    Unknown(String),
    #[error(transparent)]
    Sqlite(#[from] SqliteError),
}

/// rusqlite::Error is not `Eq`; keep its text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SqliteError(pub String);

impl From<rusqlite::Error> for ShipError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(SqliteError(e.to_string()))
    }
}

/// Stars or unstars a listed ship (never selects it).
pub fn set_favorite(
    conn: &Connection,
    id: &str,
    favorite: bool,
    now_ms: i64,
) -> Result<ShipEntry, ShipError> {
    if listed_ship(conn, id)?.is_none() {
        return Err(ShipError::Unknown(id.to_string()));
    }
    if favorite {
        conn.execute(
            "INSERT INTO ship_favorites (ship_id, created_at) VALUES (?1, ?2) ON CONFLICT (ship_id) DO NOTHING",
            params![id, now_ms],
        )?;
    } else {
        conn.execute("DELETE FROM ship_favorites WHERE ship_id = ?1", [id])?;
    }
    Ok(listed_ship(conn, id)?.expect("still listed"))
}

/// The selected ship. A selection no longer in the list is cleared (`stale: true`, for the
/// "Your selected ship is no longer in the list" notice).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedShip {
    pub ship: Option<ShipEntry>,
    pub stale: bool,
}

pub fn get_selected(conn: &mut Connection) -> Result<SelectedShip, ShipError> {
    let settings =
        crate::settings::load(conn).map_err(|e| ShipError::Sqlite(SqliteError(e.to_string())))?;
    let Some(id) = settings.selected_ship_id else {
        return Ok(SelectedShip {
            ship: None,
            stale: false,
        });
    };
    match listed_ship(conn, &id)? {
        Some(ship) => Ok(SelectedShip {
            ship: Some(ship),
            stale: false,
        }),
        None => {
            crate::settings::update(conn, |s| s.selected_ship_id = None)
                .map_err(|e| ShipError::Sqlite(SqliteError(e.to_string())))?;
            Ok(SelectedShip {
                ship: None,
                stale: true,
            })
        }
    }
}

/// Selects a listed ship (or none). Never touches contracts or runs.
pub fn set_selected(conn: &mut Connection, id: Option<&str>) -> Result<SelectedShip, ShipError> {
    let ship = match id {
        Some(id) => Some(listed_ship(conn, id)?.ok_or_else(|| ShipError::Unknown(id.to_string()))?),
        None => None,
    };
    let selected = ship.as_ref().map(|s| s.id.clone());
    crate::settings::update(conn, |s| s.selected_ship_id = selected)
        .map_err(|e| ShipError::Sqlite(SqliteError(e.to_string())))?;
    Ok(SelectedShip { ship, stale: false })
}

/// `N ships · game <version> · Star Citizen Wiki` for Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInfo {
    pub count: usize,
    pub game_version: String,
    pub generated_at: String,
    pub source: String,
}

pub fn catalog_info() -> Result<CatalogInfo, String> {
    let list = bundled()?;
    Ok(CatalogInfo {
        count: list.items.len(),
        game_version: list.game_version.clone(),
        generated_at: list.generated_at.clone(),
        source: list.source.clone(),
    })
}
