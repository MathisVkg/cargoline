//! The bundled ship catalog (`resources/catalog/ships.json`, Star Citizen Wiki API) and the
//! user's favourites and selected ship.
//!
//! Storage (Task 11, option A): the Task 02 tables fit. `ship_favorites` has a foreign key to
//! `ships`, so the JSON is upserted into `ships` at startup (idempotent, one transaction) and its
//! version recorded in `catalog_metadata`. Ships that leave the list are **never deleted** (a
//! favourite would cascade away): they are marked `availability_status = 'removed'` and hidden.
//! The selection is the existing typed setting `selected_ship_id`.
//!
//! Custom ships (Task 15) live in the same table so favourites keep their foreign key: rows with
//! `availability_status = 'custom'`, an id `custom:<16 hex>` and manufacturer "Custom". The catalog
//! sync only ever touches `'listed'` rows (and catalog ids may not start with `custom:`), so it can
//! never overwrite, mark removed or delete a custom ship. No migration: the column has no CHECK.
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
const CUSTOM: &str = "custom";
/// Id prefix of custom ships; catalog ids may not use it.
pub const CUSTOM_PREFIX: &str = "custom:";
/// Manufacturer shown for custom ships.
pub const CUSTOM_MANUFACTURER: &str = "Custom";
pub const CUSTOM_NAME_MAX_CHARS: usize = 60;
pub const CUSTOM_SCU_MAX: u32 = 100_000;

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
        if ship.id.starts_with(CUSTOM_PREFIX) {
            return Err(format!("catalog id {:?} uses the custom prefix", ship.id));
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

/// Whether the database already holds this catalog: the recorded version (game build +
/// generation date) is the bundled one and the number of listed ships matches. Read-only.
pub fn catalog_up_to_date(
    conn: &Connection,
    list: &ReferenceList<CatalogShip>,
) -> rusqlite::Result<bool> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT catalog_version FROM catalog_metadata WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if stored.as_deref() != Some(catalog_version(list).as_str()) {
        return Ok(false);
    }
    let listed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM ships WHERE availability_status = ?1",
        [LISTED],
        |r| r.get(0),
    )?;
    Ok(listed == list.items.len() as i64)
}

/// The startup sync (Task 18): checks first and only writes when the bundled catalog differs
/// from the stored one. `None` = already up to date, nothing written (no write lock taken).
pub fn sync_catalog_if_changed(
    conn: &mut Connection,
    list: &ReferenceList<CatalogShip>,
    now_ms: i64,
) -> rusqlite::Result<Option<SyncReport>> {
    if catalog_up_to_date(conn, list)? {
        return Ok(None);
    }
    sync_catalog(conn, list, now_ms).map(Some)
}

/// Upserts the list into `ships` in one transaction. Idempotent: running it again changes nothing.
/// Catalog rows missing from the list are marked removed, never deleted. Custom ships are never
/// touched: catalog ids cannot collide with theirs, and only `'listed'` rows can be marked removed.
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
        .prepare("SELECT id FROM ships WHERE availability_status = ?1")?
        .query_map([LISTED], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in known.iter().filter(|id| !ids.contains(id.as_str())) {
        report.removed += tx.execute(
            "UPDATE ships SET availability_status = ?2 WHERE id = ?1 AND availability_status = ?3",
            params![id, REMOVED, LISTED],
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
    /// Added by the user (Task 15).
    pub is_custom: bool,
}

const SELECT_SHIP: &str =
    "SELECT s.id, s.name, s.manufacturer, s.cargo_capacity_scu_milli, s.max_box_size_scu_milli,
        f.ship_id IS NOT NULL, s.availability_status = 'custom'
     FROM ships s LEFT JOIN ship_favorites f ON f.ship_id = s.id
     WHERE s.availability_status IN ('listed', 'custom')";

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
        is_custom: row.get(6)?,
        id,
    })
}

/// Listed catalog ships and custom ships, alphabetical, each with its favourite and custom flags.
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
    /// Custom ship input rejected; nothing was saved.
    #[error("Invalid custom ship")]
    Invalid(Vec<ShipIssue>),
    #[error(transparent)]
    Sqlite(#[from] SqliteError),
}

/// One rejected field of a custom ship (`name` or `cargoScu`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShipIssue {
    pub field: &'static str,
    pub message: String,
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

/* ------------------------------------------------------------ custom ships */

/// Name and capacity of a custom ship, as typed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomShipInput {
    pub name: String,
    /// Whole SCU, 1–100,000.
    pub cargo_scu: i64,
}

const fn issue(field: &'static str, message: String) -> ShipIssue {
    ShipIssue { field, message }
}

/// Case- and whitespace-insensitive identity of a custom ship name.
fn name_key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Checks the input and returns the trimmed name and the capacity. `except` is the ship being
/// edited (it may keep its own name).
fn validate_custom(
    conn: &Connection,
    input: &CustomShipInput,
    except: Option<&str>,
) -> Result<(String, u32), ShipError> {
    let mut issues = Vec::new();
    let name = input.name.trim().to_string();
    let chars = name.chars().count();
    if chars == 0 {
        issues.push(issue("name", "Enter a name.".into()));
    } else if chars > CUSTOM_NAME_MAX_CHARS {
        issues.push(issue(
            "name",
            format!("Use at most {CUSTOM_NAME_MAX_CHARS} characters."),
        ));
    } else if name.chars().any(char::is_control) {
        issues.push(issue(
            "name",
            "Remove line breaks and control characters.".into(),
        ));
    } else {
        let key = name_key(&name);
        let mut stmt =
            conn.prepare_cached("SELECT id, name FROM ships WHERE availability_status = ?1")?;
        let taken = stmt
            .query_map([CUSTOM], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .any(|(id, other)| Some(id.as_str()) != except && name_key(&other) == key);
        if taken {
            issues.push(issue(
                "name",
                "You already have a custom ship with this name.".into(),
            ));
        }
    }
    let scu = u32::try_from(input.cargo_scu)
        .ok()
        .filter(|v| (1..=CUSTOM_SCU_MAX).contains(v));
    if scu.is_none() {
        issues.push(issue(
            "cargoScu",
            format!("Enter a whole number of SCU from 1 to {}.", "100,000"),
        ));
    }
    if !issues.is_empty() {
        return Err(ShipError::Invalid(issues));
    }
    Ok((name, scu.expect("checked")))
}

fn new_custom_id(conn: &Connection) -> rusqlite::Result<String> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    loop {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        let id = format!("{CUSTOM_PREFIX}{:016x}", h.finish());
        let exists: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM ships WHERE id = ?1)",
            [&id],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(id);
        }
    }
}

fn custom_exists(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM ships WHERE id = ?1 AND availability_status = ?2)",
        params![id, CUSTOM],
        |r| r.get(0),
    )
}

/// Adds a custom ship. Returns it (not yet selected; the caller selects it).
pub fn create_custom_ship(
    conn: &mut Connection,
    input: &CustomShipInput,
) -> Result<ShipEntry, ShipError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (name, scu) = validate_custom(&tx, input, None)?;
    let id = new_custom_id(&tx)?;
    tx.execute(
        "INSERT INTO ships (id, manufacturer, name, variant, cargo_capacity_scu_milli, max_box_size_scu_milli,
                            source_url, verified_at, availability_status)
         VALUES (?1, ?2, ?3, NULL, ?4, NULL, NULL, NULL, ?5)",
        params![id, CUSTOM_MANUFACTURER, name, i64::from(scu) * 1000, CUSTOM],
    )?;
    let entry = listed_ship(&tx, &id)?.expect("just inserted");
    tx.commit()?;
    Ok(entry)
}

/// Renames or resizes a custom ship. Catalog ships cannot be edited (`Unknown`).
pub fn update_custom_ship(
    conn: &mut Connection,
    id: &str,
    input: &CustomShipInput,
) -> Result<ShipEntry, ShipError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !custom_exists(&tx, id)? {
        return Err(ShipError::Unknown(id.to_string()));
    }
    let (name, scu) = validate_custom(&tx, input, Some(id))?;
    tx.execute(
        "UPDATE ships SET name = ?2, cargo_capacity_scu_milli = ?3 WHERE id = ?1 AND availability_status = ?4",
        params![id, name, i64::from(scu) * 1000, CUSTOM],
    )?;
    let entry = listed_ship(&tx, id)?.expect("still there");
    tx.commit()?;
    Ok(entry)
}

/// Deletes a custom ship in one transaction: its favourite goes with it (foreign key cascade) and
/// the selection is cleared if it was selected. Contracts never reference ships.
/// Returns whether the deleted ship was the selected one.
pub fn delete_custom_ship(conn: &mut Connection, id: &str) -> Result<bool, ShipError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !custom_exists(&tx, id)? {
        return Err(ShipError::Unknown(id.to_string()));
    }
    tx.execute(
        "DELETE FROM ships WHERE id = ?1 AND availability_status = ?2",
        params![id, CUSTOM],
    )?;
    let settings =
        crate::settings::load(&tx).map_err(|e| ShipError::Sqlite(SqliteError(e.to_string())))?;
    let was_selected = settings.selected_ship_id.as_deref() == Some(id);
    if was_selected {
        crate::settings::update_in(&tx, |s| s.selected_ship_id = None)
            .map_err(|e| ShipError::Sqlite(SqliteError(e.to_string())))?;
    }
    tx.commit()?;
    Ok(was_selected)
}
