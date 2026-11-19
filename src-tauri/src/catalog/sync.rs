//! Startup sync of the bundled snapshot into the catalog tables.
//!
//! Runs after migrations, in one IMMEDIATE transaction. Data can change
//! without a schema migration: whenever the embedded `catalog_version`
//! differs from the one recorded in `catalog_metadata`, the snapshot is
//! upserted. Any failure rolls back and leaves the previous catalog intact.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use super::snapshot::Snapshot;
use super::{normalize_display, normalize_key, CatalogTable};

pub const REFERENCE_CATALOG: &str = "reference";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// Recorded version already matches the snapshot; nothing written.
    UpToDate,
    Applied(SyncStats),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncStats {
    pub inserted: usize,
    pub updated: usize,
    /// User entries that matched a bundled entry and became bundled.
    pub merged_user: usize,
    pub retired: usize,
}

/// One snapshot row, independent of table.
struct Row<'a> {
    source_id: &'a str,
    name: &'a str,
    kind: Option<&'a str>,
    body: Option<&'a str>,
    system: Option<&'a str>,
    seen: bool,
}

/// Applies `snapshot` if its version is not the one already recorded.
pub fn sync_reference_catalog(
    conn: &mut Connection,
    snapshot: &Snapshot,
    now_ms: i64,
) -> rusqlite::Result<SyncOutcome> {
    snapshot
        .validate()
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let recorded: Option<String> = tx
        .query_row(
            "SELECT catalog_version FROM catalog_metadata WHERE catalog = ?1",
            [REFERENCE_CATALOG],
            |r| r.get(0),
        )
        .optional()?;
    if recorded.as_deref() == Some(snapshot.catalog_version.as_str()) {
        return Ok(SyncOutcome::UpToDate);
    }

    let mut stats = SyncStats::default();
    let commodities = snapshot.commodities.iter().map(|c| Row {
        source_id: &c.source_id,
        name: &c.name,
        kind: c.kind.as_deref(),
        body: None,
        system: None,
        seen: c.seen_in_hauling,
    });
    apply_table(
        &tx,
        CatalogTable::Commodities,
        commodities,
        now_ms,
        &mut stats,
    )?;
    let locations = snapshot.locations.iter().map(|l| Row {
        source_id: &l.source_id,
        name: &l.name,
        kind: Some(&l.kind),
        body: l.body.as_deref(),
        system: l.system.as_deref(),
        seen: l.seen_in_hauling,
    });
    apply_table(&tx, CatalogTable::Locations, locations, now_ms, &mut stats)?;

    tx.execute(
        "INSERT INTO catalog_metadata
           (catalog, catalog_version, game_version, source_name, source_url, fetched_on, attribution, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (catalog) DO UPDATE SET
           catalog_version = excluded.catalog_version, game_version = excluded.game_version,
           source_name = excluded.source_name, source_url = excluded.source_url,
           fetched_on = excluded.fetched_on, attribution = excluded.attribution,
           imported_at = excluded.imported_at",
        params![
            REFERENCE_CATALOG,
            snapshot.catalog_version,
            snapshot.game_version,
            snapshot.source.name,
            snapshot.source.url,
            snapshot.fetched_at,
            snapshot.attribution,
            now_ms
        ],
    )?;
    tx.commit()?;
    Ok(SyncOutcome::Applied(stats))
}

fn apply_table<'a>(
    tx: &Transaction,
    table: CatalogTable,
    rows: impl Iterator<Item = Row<'a>>,
    now_ms: i64,
    stats: &mut SyncStats,
) -> rusqlite::Result<()> {
    let t = table.table();
    let place = table == CatalogTable::Locations;
    let mut keep: HashSet<i64> = HashSet::new();

    for row in rows {
        let display = normalize_display(row.name);
        let key = normalize_key(row.name);
        // 1. Match by source id, 2. by normalized name.
        let by_id: Option<(i64, String)> = tx
            .prepare_cached(&format!(
                "SELECT id, normalized_name FROM {t} WHERE source_id = ?1"
            ))?
            .query_row([row.source_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let by_name: Option<(i64, String, Option<String>)> = tx
            .prepare_cached(&format!(
                "SELECT id, source, source_id FROM {t} WHERE normalized_name = ?1"
            ))?
            .query_row([&key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;

        let target = match (by_id, by_name) {
            // The bundled row was renamed onto a name another row already holds:
            // that row takes over the source id; the old row is retired.
            (Some((old, _)), Some((other, source, _))) if old != other => {
                tx.execute(
                    &format!("UPDATE {t} SET source_id = NULL, retired_at = ?2 WHERE id = ?1"),
                    params![old, now_ms],
                )?;
                stats.retired += 1;
                if source == "user" {
                    stats.merged_user += 1;
                }
                Some(other)
            }
            (Some((id, _)), _) => Some(id),
            (None, Some((id, source, _))) => {
                if source == "user" {
                    stats.merged_user += 1;
                }
                Some(id)
            }
            (None, None) => None,
        };

        let id = match target {
            Some(id) => {
                // Matched by id: take the snapshot spelling (a bundled rename).
                // Matched by name: keys are equal, so names differ at most in
                // case/whitespace and the canonical spelling is applied.
                tx.prepare_cached(&format!(
                    "UPDATE {t} SET display_name = ?2, normalized_name = ?3, source = 'bundled', source_id = ?4,
                       kind = ?5, seen_in_hauling = ?6, retired_at = NULL{} WHERE id = ?1",
                    if place { ", body_name = ?7, system_name = ?8" } else { "" }
                ))?
                .execute(rusqlite::params_from_iter(
                    [
                        rusqlite::types::Value::from(id),
                        display.clone().into(),
                        key.clone().into(),
                        row.source_id.to_string().into(),
                        row.kind.map(str::to_string).into(),
                        i64::from(row.seen).into(),
                    ]
                    .into_iter()
                    .chain(place.then(|| row.body.map(str::to_string).into()))
                    .chain(place.then(|| row.system.map(str::to_string).into())),
                ))?;
                stats.updated += 1;
                id
            }
            None => {
                tx.prepare_cached(&format!(
                    "INSERT INTO {t} (display_name, normalized_name, source, source_id, kind, seen_in_hauling, created_at{})
                     VALUES (?1, ?2, 'bundled', ?3, ?4, ?5, ?6{})",
                    if place { ", body_name, system_name" } else { "" },
                    if place { ", ?7, ?8" } else { "" }
                ))?
                .execute(rusqlite::params_from_iter(
                    [
                        rusqlite::types::Value::from(display.clone()),
                        key.clone().into(),
                        row.source_id.to_string().into(),
                        row.kind.map(str::to_string).into(),
                        i64::from(row.seen).into(),
                        now_ms.into(),
                    ]
                    .into_iter()
                    .chain(place.then(|| row.body.map(str::to_string).into()))
                    .chain(place.then(|| row.system.map(str::to_string).into())),
                ))?;
                stats.inserted += 1;
                tx.last_insert_rowid()
            }
        };
        keep.insert(id);
    }

    // Bundled rows no longer in the snapshot are retired, never deleted.
    let stale: Vec<i64> = tx
        .prepare(&format!(
            "SELECT id FROM {t} WHERE source = 'bundled' AND retired_at IS NULL"
        ))?
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<i64>, _>>()?
        .into_iter()
        .filter(|id| !keep.contains(id))
        .collect();
    for id in stale {
        tx.execute(
            &format!("UPDATE {t} SET retired_at = ?2 WHERE id = ?1"),
            params![id, now_ms],
        )?;
        stats.retired += 1;
    }
    Ok(())
}
