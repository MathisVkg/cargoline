//! Tests: the bundled ship catalog (validity, storage sync, favourites, selection, stale
//! selection), the "Complete contracts finished in game" settings and backend safeguards, and
//! `reactivate_contracts` (Undo). Temporary databases only; no desktop, no real Game.log.

use cargo_line_lib::contracts::finalize::{finalize_contracts, FinalStatus};
use cargo_line_lib::contracts::reactivate::{
    reactivate_contracts, ReactivateError, REACTIVATE_WINDOW_MS,
};
use cargo_line_lib::contracts::repository::{create_contract_linked, SaveLinks};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::models::{ContractSource, ContractStatus};
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::gamelog::patterns::parse_line;
use cargo_line_lib::gamelog::service::{ended_before_accept, log_time_ms, to_app_events};
use cargo_line_lib::gamelog::tracker::Tracker;
use cargo_line_lib::gamelog::{CompleteFromGame, GameLogEvent};
use cargo_line_lib::settings::{self, AppSettings, CompleteFromGameMode};
use cargo_line_lib::ships::{self, CatalogShip, ShipError};
use rusqlite::Connection;

fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    (dir, db)
}

const T0: i64 = 1_795_176_000_000; // 2026-11-20T12:00:00Z

/* ---------------------------------------------------------------- catalog */

#[test]
fn bundled_catalog_is_valid() {
    let list = ships::bundled().expect("ships.json parses");
    assert_eq!(list.game_version, "4.10.3-LIVE.13165946");
    assert!(list.source.contains("Star Citizen Wiki"));
    assert!(!list.attribution.is_empty());
    assert_eq!(list.items.len(), 100);
    let get = |id: &str| {
        list.items
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    let c2 = get("CRUS_Starlifter_C2");
    assert_eq!(
        (c2.name.as_str(), c2.cargo_scu, c2.max_container_scu),
        ("C2 Hercules Starlifter", 696, Some(32))
    );
    assert_eq!(c2.manufacturer_code, "CRUS");
    assert_eq!(get("RSI_Galaxy_TEMP_Cargo").cargo_scu, 720);
    // The largest container is derived from grid sizes. Placeholder grids give null,
    // never a placeholder 1.
    assert_eq!(get("RSI_Hermes").max_container_scu, None);
    assert_eq!(get("DRAK_Ironclad").max_container_scu, None);
    assert_eq!(get("MISC_Hull_C").max_container_scu, Some(32));
    assert_eq!(get("ORIG_600i").max_container_scu, Some(4));
    for ship in &list.items {
        assert!(ship.cargo_scu > 0, "{}", ship.name);
        assert!(
            !ship.manufacturer.is_empty() && !ship.manufacturer_code.is_empty(),
            "{}",
            ship.name
        );
    }
    assert_eq!(T0, log_time_ms("2026-11-20T12:00:00Z").unwrap());
}

#[test]
fn catalog_rules_reject_bad_lists() {
    let wrap = |items: &str| {
        format!(
            r#"{{"source":"s","attribution":"a","game_version":"v","generated_at":"2026-11-20","items":[{items}]}}"#
        )
    };
    let ship = |id: &str, name: &str, scu: u32| {
        format!(
            r#"{{"id":"{id}","name":"{name}","manufacturer":"M","manufacturer_code":"M","cargo_scu":{scu},"max_container_scu":null}}"#
        )
    };
    assert!(ships::parse_ships(&wrap(&format!(
        "{},{}",
        ship("A", "Alpha", 1),
        ship("B", "Beta", 2)
    )))
    .is_ok());
    assert!(ships::parse_ships(&wrap(&format!(
        "{},{}",
        ship("A", "Alpha", 1),
        ship("A", "Beta", 2)
    )))
    .is_err());
    assert!(ships::parse_ships(&wrap(&ship("A", "Alpha", 0))).is_err());
    assert!(ships::parse_ships(&wrap(&ship(" A", "Alpha", 1))).is_err());
    // Unknown fields are refused.
    assert!(ships::parse_ships(&wrap(r#"{"id":"A","name":"A","manufacturer":"M","manufacturer_code":"M","cargo_scu":1,"max_container_scu":null,"x":1}"#)).is_err());
}

fn sample(ids: &[(&str, &str, u32)]) -> cargo_line_lib::reference::ReferenceList<CatalogShip> {
    let mut items: Vec<CatalogShip> = ids
        .iter()
        .map(|(id, name, scu)| CatalogShip {
            id: id.to_string(),
            name: name.to_string(),
            manufacturer: "Maker".into(),
            manufacturer_code: "MK".into(),
            cargo_scu: *scu,
            max_container_scu: Some(8),
        })
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    cargo_line_lib::reference::ReferenceList {
        source: "test".into(),
        attribution: "test".into(),
        game_version: "1".into(),
        generated_at: "2026-11-20".into(),
        items,
    }
}

#[test]
fn sync_is_idempotent_and_never_deletes() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let list = ships::bundled().unwrap();
    let first = ships::sync_catalog(&mut conn, list, T0).unwrap();
    assert_eq!(
        (first.upserted, first.removed, first.version_changed),
        (100, 0, true)
    );
    let again = ships::sync_catalog(&mut conn, list, T0 + 1).unwrap();
    assert_eq!(
        (again.upserted, again.removed, again.version_changed),
        (0, 0, false)
    );
    let listed = ships::list_ships(&conn).unwrap();
    assert_eq!(listed.len(), 100);
    let c2 = listed
        .iter()
        .find(|s| s.id == "CRUS_Starlifter_C2")
        .unwrap();
    assert_eq!(
        (c2.cargo_scu, c2.max_container_scu, c2.is_favorite),
        (696, Some(32), false)
    );
    assert_eq!(c2.manufacturer_code, "CRUS");
    // A ship without a known largest container (null in ships.json) stores, lists, selects and
    // serialises as null.
    let hermes = listed.iter().find(|s| s.id == "RSI_Hermes").unwrap();
    assert_eq!((hermes.cargo_scu, hermes.max_container_scu), (288, None));
    let json = serde_json::to_value(hermes).unwrap();
    assert!(json["maxContainerScu"].is_null(), "{json}");
    let selected = ships::set_selected(&mut conn, Some("RSI_Hermes")).unwrap();
    assert_eq!(selected.ship.map(|s| s.max_container_scu), Some(None));
    let version: String = conn
        .query_row(
            "SELECT catalog_version FROM catalog_metadata WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, ships::catalog_version(list));
    let info = ships::catalog_info().unwrap();
    assert_eq!(
        (info.count, info.game_version.as_str()),
        (100, "4.10.3-LIVE.13165946")
    );
}

#[test]
fn favourites_selection_and_stale_fallback() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let v1 = sample(&[("SHIP_A", "Alpha", 10), ("SHIP_B", "Bravo", 20)]);
    ships::sync_catalog(&mut conn, &v1, T0).unwrap();

    // Favourites: toggle never selects; repeated toggles are harmless.
    assert!(
        ships::set_favorite(&conn, "SHIP_B", true, T0)
            .unwrap()
            .is_favorite
    );
    assert!(
        ships::set_favorite(&conn, "SHIP_B", true, T0)
            .unwrap()
            .is_favorite
    );
    assert_eq!(ships::get_selected(&mut conn).unwrap().ship, None);
    assert!(
        !ships::set_favorite(&conn, "SHIP_A", false, T0)
            .unwrap()
            .is_favorite
    );
    assert_eq!(
        ships::set_favorite(&conn, "NOPE", true, T0),
        Err(ShipError::Unknown("NOPE".into()))
    );

    // Selection round trip (persisted as the existing setting), then No ship.
    let chosen = ships::set_selected(&mut conn, Some("SHIP_A")).unwrap();
    assert_eq!(chosen.ship.as_ref().map(|s| s.cargo_scu), Some(10));
    assert_eq!(
        settings::load(&conn).unwrap().selected_ship_id.as_deref(),
        Some("SHIP_A")
    );
    let read = ships::get_selected(&mut conn).unwrap();
    assert_eq!(
        (read.ship.map(|s| s.id), read.stale),
        (Some("SHIP_A".into()), false)
    );
    assert_eq!(
        ships::set_selected(&mut conn, Some("NOPE")),
        Err(ShipError::Unknown("NOPE".into()))
    );
    assert_eq!(
        settings::load(&conn).unwrap().selected_ship_id.as_deref(),
        Some("SHIP_A"),
        "unchanged on error"
    );
    assert_eq!(ships::set_selected(&mut conn, None).unwrap().ship, None);
    assert_eq!(settings::load(&conn).unwrap().selected_ship_id, None);

    // A new catalog without Alpha and Bravo: the selection falls back to none (stale, once),
    // the favourite row is kept but hidden, and the ship rows are kept as removed.
    ships::set_selected(&mut conn, Some("SHIP_B")).unwrap();
    let v2 = sample(&[("SHIP_C", "Charlie", 30)]);
    let report = ships::sync_catalog(&mut conn, &v2, T0 + 1).unwrap();
    assert_eq!(report.removed, 2);
    let stale = ships::get_selected(&mut conn).unwrap();
    assert_eq!((stale.ship, stale.stale), (None, true));
    assert_eq!(settings::load(&conn).unwrap().selected_ship_id, None);
    let after = ships::get_selected(&mut conn).unwrap();
    assert_eq!((after.ship, after.stale), (None, false), "reported once");
    let ids: Vec<String> = ships::list_ships(&conn)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, ["SHIP_C"]);
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM ships", [], |r| r.get(0))
        .unwrap();
    let favs: i64 = conn
        .query_row("SELECT COUNT(*) FROM ship_favorites", [], |r| r.get(0))
        .unwrap();
    assert_eq!((rows, favs), (3, 1));

    // Back in the list: listed again, favourite intact.
    ships::sync_catalog(&mut conn, &v1, T0 + 2).unwrap();
    let b = ships::list_ships(&conn)
        .unwrap()
        .into_iter()
        .find(|s| s.id == "SHIP_B")
        .unwrap();
    assert!(b.is_favorite);
}

#[test]
fn the_ship_never_touches_contracts() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    ships::sync_catalog(&mut conn, ships::bundled().unwrap(), T0).unwrap();
    let id = contract(&mut conn, Some(MA), T0);
    let before = snapshot(&conn);
    ships::set_selected(&mut conn, Some("CRUS_Starlifter_C2")).unwrap();
    ships::set_favorite(&conn, "CRUS_Starlifter_C2", true, T0).unwrap();
    assert_eq!(snapshot(&conn), before);
    assert!(id > 0);
}

/* --------------------------------------------------------------- settings */

#[test]
fn complete_from_game_settings() {
    let s = AppSettings::default();
    assert!(!s.complete_from_game);
    assert_eq!(s.complete_from_game_mode, CompleteFromGameMode::Ask);

    let mut on = AppSettings {
        complete_from_game: true,
        ..AppSettings::default()
    };
    let err = on.validate().unwrap_err();
    assert_eq!(err.field, "complete_from_game");
    on.game_log_monitoring_enabled = true;
    on.validate().unwrap();

    // Stored values from earlier versions (no keys) load with the defaults; modes are snake_case.
    let old: AppSettings = serde_json::from_str("{}").unwrap();
    assert!(!old.complete_from_game);
    let auto: AppSettings =
        serde_json::from_str(r#"{"complete_from_game_mode":"automatic"}"#).unwrap();
    assert_eq!(
        auto.complete_from_game_mode,
        CompleteFromGameMode::Automatic
    );
    assert!(
        serde_json::from_str::<AppSettings>(r#"{"complete_from_game_mode":"always"}"#).is_err()
    );

    assert_eq!(
        CompleteFromGame::from_settings(&AppSettings::default()),
        CompleteFromGame::Off
    );
    let ask = AppSettings {
        game_log_monitoring_enabled: true,
        complete_from_game: true,
        ..AppSettings::default()
    };
    assert_eq!(CompleteFromGame::from_settings(&ask), CompleteFromGame::Ask);
    let auto = AppSettings {
        complete_from_game_mode: CompleteFromGameMode::Automatic,
        ..ask
    };
    assert_eq!(
        CompleteFromGame::from_settings(&auto),
        CompleteFromGame::Automatic
    );

    // Persisted through the settings store.
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let saved = settings::update(&mut conn, |s| {
        s.game_log_monitoring_enabled = true;
        s.game_log_path = Some(r"C:\Games\StarCitizen\LIVE\Game.log".into());
        s.complete_from_game = true;
        s.complete_from_game_mode = CompleteFromGameMode::Automatic;
    })
    .unwrap();
    assert_eq!(settings::load(&conn).unwrap(), saved);
    assert!(settings::update(&mut conn, |s| s.game_log_monitoring_enabled = false).is_err());
}

/* --------------------------------------------------- Game.log safeguards */

const MA: &str = "6a1b2c3d-aaaa-4aaa-8bbb-0123456789ab";
const MB: &str = "6a1b2c3d-bbbb-4aaa-8bbb-0123456789ab";
const MC: &str = "6a1b2c3d-cccc-4aaa-8bbb-0123456789ab";
const MD: &str = "6a1b2c3d-dddd-4aaa-8bbb-0123456789ab";
const ME: &str = "6a1b2c3d-eeee-4aaa-8bbb-0123456789ab";

fn input(title: &str) -> NewContractInput {
    NewContractInput {
        title: title.into(),
        reward_auec: "59500".into(),
        requirements: vec![NewRequirementInput {
            commodity_name: "Aluminum".into(),
            quantity_scu: "84".into(),
            max_box_size_scu: None,
            pickups: vec![NewStopInput {
                location_name: "ARC-L2 Lively Pathway Station".into(),
                quantity_scu: None,
            }],
            deliveries: vec![NewStopInput {
                location_name: "Baijini Point".into(),
                quantity_scu: None,
            }],
        }],
    }
}

fn contract(conn: &mut Connection, mission: Option<&str>, accepted_at: i64) -> i64 {
    let valid =
        validate_new_contract(&input(&format!("Haul {}", mission.unwrap_or("manual")))).unwrap();
    let at = AcceptanceTime {
        utc_ms: accepted_at,
        local_date: "2026-11-20".into(),
        utc_offset_minutes: 60,
    };
    let links = SaveLinks {
        capture_id: None,
        game_mission_id: mission.map(Into::into),
        auto_capture: false,
    };
    create_contract_linked(conn, &valid, &at, ContractSource::Manual, &links)
        .unwrap()
        .id
}

fn ended(id: &str, state: &str, at: &str) -> String {
    format!(
        "<{at}> [Notice] <MissionEnded> Received MissionEnded push message for: mission_id {id} - mission_state {state} [Team_GameServices][Missions]"
    )
}

/// (id, status, finished_at, run + cargo + ticks fingerprint) of every contract.
fn snapshot(conn: &Connection) -> Vec<(i64, String, Option<i64>, String)> {
    conn.prepare(
        "SELECT c.id, c.status, c.finished_at,
                c.run_id || '/' || c.title || '/' || c.reward_auec || '/' ||
                (SELECT group_concat(r.commodity_name || ':' || r.quantity_scu_milli || ':' ||
                        (SELECT group_concat(s.operation || s.location_name || ':' || IFNULL(s.reminder_done_at, '-'), ',')
                           FROM cargo_stops s WHERE s.requirement_id = r.id), ';')
                   FROM cargo_requirements r WHERE r.contract_id = c.id)
         FROM contracts c ORDER BY c.id",
    )
    .unwrap()
    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn status(conn: &Connection, id: i64) -> String {
    conn.query_row("SELECT status FROM contracts WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .unwrap()
}

#[test]
fn ended_before_accept_rule() {
    assert!(ended_before_accept(Some("2026-11-20T11:59:59.999Z"), T0));
    assert!(!ended_before_accept(Some("2026-11-20T12:00:00.000Z"), T0));
    assert!(!ended_before_accept(Some("2026-11-20T12:30:00Z"), T0));
    assert!(!ended_before_accept(None, T0), "no timestamp: not dropped");
    assert!(
        !ended_before_accept(Some("garbage"), T0),
        "unreadable timestamp: not dropped"
    );
}

/// The real scenario: three contracts accepted with a MissionId, one manual contract, and a test
/// log. Only exact MissionIds of active contracts, ended after their acceptance, are reported;
/// each MissionId once. The frontend then acts on MISSION_STATE_COMPLETED only.
#[test]
fn scenario_three_contracts_and_a_test_log() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let a = contract(&mut conn, Some(MA), T0);
    let b = contract(&mut conn, Some(MB), T0);
    let c = contract(&mut conn, Some(MC), T0);
    let manual = contract(&mut conn, None, T0);
    // Accepted later than its log end (an earlier session's end with the same id).
    let late = contract(&mut conn, Some(ME), T0 + 3_600_000);

    let log = [
        ended(MA, "MISSION_STATE_COMPLETED", "2026-11-20T12:40:00.000Z"),
        ended(MB, "MISSION_STATE_COMPLETED", "2026-11-20T12:41:00.000Z"),
        ended(MB, "MISSION_STATE_COMPLETED", "2026-11-20T12:41:00.500Z"), // duplicate
        ended(MC, "MISSION_STATE_ABANDONED", "2026-11-20T12:42:00.000Z"),
        ended(MD, "MISSION_STATE_COMPLETED", "2026-11-20T12:43:00.000Z"), // unknown mission
        ended(ME, "MISSION_STATE_COMPLETED", "2026-11-20T12:44:00.000Z"), // before its accept
        ended(
            &MA.to_uppercase(),
            "MISSION_STATE_COMPLETED",
            "2026-11-20T12:45:00.000Z",
        ), // not exact
        "<2026-11-20T12:46:00.000Z> [Notice] noise without a mission".to_string(),
    ];
    let mut tracker = Tracker::default();
    for line in &log {
        if let Some(parsed) = parse_line(line) {
            tracker.feed(parsed);
        }
    }
    let events = to_app_events(&conn, tracker.flush(), false, CompleteFromGame::Automatic);
    let reported: Vec<(i64, &str, CompleteFromGame)> = events
        .iter()
        .map(|e| match e {
            GameLogEvent::Ended {
                contract_id,
                state,
                complete_from_game,
                ..
            } => (*contract_id, state.as_str(), *complete_from_game),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        reported,
        [
            (a, "MISSION_STATE_COMPLETED", CompleteFromGame::Automatic),
            (b, "MISSION_STATE_COMPLETED", CompleteFromGame::Automatic),
            (c, "MISSION_STATE_ABANDONED", CompleteFromGame::Automatic),
        ]
    );
    assert!(!reported.iter().any(|r| r.0 == manual || r.0 == late));

    // What the frontend finalises: completed states only (never abandon automatically).
    let to_complete: Vec<i64> = reported
        .iter()
        .filter(|r| r.1 == "MISSION_STATE_COMPLETED")
        .map(|r| r.0)
        .collect();
    let now = T0 + 3 * 3_600_000;
    finalize_contracts(&mut conn, &to_complete, FinalStatus::Completed, now).unwrap();
    assert_eq!(
        [
            status(&conn, a),
            status(&conn, b),
            status(&conn, c),
            status(&conn, manual),
            status(&conn, late)
        ],
        ["completed", "completed", "active", "active", "active"]
    );

    // Already finalised: the same log read again reports nothing for A and B.
    let mut again = Tracker::default();
    for line in &log {
        if let Some(parsed) = parse_line(line) {
            again.feed(parsed);
        }
    }
    let events = to_app_events(&conn, again.flush(), false, CompleteFromGame::Automatic);
    assert_eq!(
        events.len(),
        1,
        "only C (still active) is reported again: {events:?}"
    );

    // Undo. A Pick Up tick on A must survive.
    conn.execute(
        "UPDATE cargo_stops SET reminder_done_at = ?2 WHERE requirement_id IN (SELECT id FROM cargo_requirements WHERE contract_id = ?1) AND operation = 'pickup'",
        rusqlite::params![a, T0 + 60_000],
    )
    .unwrap();
    let before = snapshot(&conn);
    let restored = reactivate_contracts(&mut conn, &[a, b], now + 1_000).unwrap();
    assert!(restored
        .iter()
        .all(|d| d.status == ContractStatus::Active && d.finished_at.is_none()));
    let after = snapshot(&conn);
    for (x, y) in before.iter().zip(&after) {
        assert_eq!(x.3, y.3, "run and cargo untouched");
    }
    assert_eq!(status(&conn, a), "active");
}

/* ----------------------------------------------------------- reactivate */

#[test]
fn reactivate_rules() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let a = contract(&mut conn, Some(MA), T0);
    let b = contract(&mut conn, Some(MB), T0);
    let c = contract(&mut conn, None, T0);
    let active = contract(&mut conn, None, T0);
    let now = T0 + 3_600_000;
    finalize_contracts(&mut conn, &[a, b], FinalStatus::Completed, now).unwrap();
    finalize_contracts(&mut conn, &[c], FinalStatus::Abandoned, now).unwrap();

    assert_eq!(
        reactivate_contracts(&mut conn, &[], now),
        Err(ReactivateError::Empty)
    );
    assert_eq!(
        reactivate_contracts(&mut conn, &[999], now),
        Err(ReactivateError::NotFound(999))
    );
    assert!(matches!(
        reactivate_contracts(&mut conn, &[a, c], now),
        Err(ReactivateError::NotCompleted { id, ref status }) if id == c && status == "abandoned"
    ));
    assert!(matches!(
        reactivate_contracts(&mut conn, &[active], now),
        Err(ReactivateError::NotCompleted { id, .. }) if id == active
    ));
    assert_eq!(
        reactivate_contracts(&mut conn, &[a], now + REACTIVATE_WINDOW_MS + 1),
        Err(ReactivateError::TooOld(a))
    );
    // All or nothing: A was not reactivated by the failing calls above.
    let status_a: String = conn
        .query_row("SELECT status FROM contracts WHERE id = ?1", [a], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(status_a, "completed");

    // Within the window, duplicates collapse; finished_at cleared; ticks untouched.
    let ticks_before = snapshot(&conn);
    let done = reactivate_contracts(&mut conn, &[a, b, a], now + REACTIVATE_WINDOW_MS).unwrap();
    assert_eq!(done.iter().map(|d| d.id).collect::<Vec<_>>(), [a, b]);
    let finished: Option<i64> = conn
        .query_row(
            "SELECT finished_at FROM contracts WHERE id = ?1",
            [b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(finished, None);
    let ticks_after = snapshot(&conn);
    for (x, y) in ticks_before.iter().zip(&ticks_after) {
        assert_eq!(x.3, y.3, "run, cargo and ticks untouched");
    }
    // A second Undo of the same ids is refused (they are active again).
    assert!(matches!(
        reactivate_contracts(&mut conn, &[a], now),
        Err(ReactivateError::NotCompleted { .. })
    ));
}
