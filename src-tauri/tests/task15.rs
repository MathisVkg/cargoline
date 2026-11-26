//! Task 15 tests: custom ships (validation, catalog sync never touches them, favourites,
//! selection, deletion) and aUEC per hour per run (interval union, rounding, null cases).
//! Temporary databases and synthetic data only.

use cargo_line_lib::contracts::finalize::{finalize_contracts, FinalStatus};
use cargo_line_lib::contracts::repository::{create_contract_linked, SaveLinks};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::models::ContractSource;
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::history::{self, auec_per_hour, union_duration_ms, MIN_RATE_DURATION_MS};
use cargo_line_lib::reference::ReferenceList;
use cargo_line_lib::settings;
use cargo_line_lib::ships::{self, CatalogShip, CustomShipInput, ShipError, CUSTOM_PREFIX};
use rusqlite::Connection;

const T0: i64 = 1_795_176_000_000; // 2026-11-20T12:00:00Z
const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;

fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    (dir, db)
}

fn input(name: &str, scu: i64) -> CustomShipInput {
    CustomShipInput {
        name: name.into(),
        cargo_scu: scu,
    }
}

fn fields(err: ShipError) -> Vec<&'static str> {
    match err {
        ShipError::Invalid(issues) => issues.into_iter().map(|i| i.field).collect(),
        other => panic!("expected a validation error, got {other:?}"),
    }
}

fn catalog(ids: &[(&str, &str, u32)], version: &str) -> ReferenceList<CatalogShip> {
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
    ReferenceList {
        source: "test".into(),
        attribution: "test".into(),
        game_version: version.into(),
        generated_at: "2026-11-20".into(),
        items,
    }
}

/* ------------------------------------------------------------ custom ships */

#[test]
fn create_update_delete_with_validation() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let ship = ships::create_custom_ship(&mut conn, &input("  Retaliator Cargo  ", 74)).unwrap();
    assert!(ship.id.starts_with(CUSTOM_PREFIX), "{}", ship.id);
    assert_eq!(
        (
            ship.name.as_str(),
            ship.cargo_scu,
            ship.manufacturer.as_str(),
            ship.is_custom,
            ship.max_container_scu
        ),
        ("Retaliator Cargo", 74, "Custom", true, None)
    );

    // Name: required, at most 60 characters, unique among custom ships ignoring case/spacing.
    assert_eq!(
        fields(ships::create_custom_ship(&mut conn, &input("   ", 10)).unwrap_err()),
        ["name"]
    );
    let long = "x".repeat(61);
    assert_eq!(
        fields(ships::create_custom_ship(&mut conn, &input(&long, 10)).unwrap_err()),
        ["name"]
    );
    ships::create_custom_ship(&mut conn, &input(&"y".repeat(60), 10)).unwrap();
    assert_eq!(
        fields(ships::create_custom_ship(&mut conn, &input("retaliator  CARGO", 10)).unwrap_err()),
        ["name"]
    );
    // SCU: whole number 1–100,000.
    assert_eq!(
        fields(ships::create_custom_ship(&mut conn, &input("A", 0)).unwrap_err()),
        ["cargoScu"]
    );
    assert_eq!(
        fields(ships::create_custom_ship(&mut conn, &input("A", 100_001)).unwrap_err()),
        ["cargoScu"]
    );
    assert_eq!(
        fields(ships::create_custom_ship(&mut conn, &input("", -5)).unwrap_err()),
        ["name", "cargoScu"]
    );
    ships::create_custom_ship(&mut conn, &input("Big", 100_000)).unwrap();

    // A custom ship may share a catalog ship's name.
    ships::sync_catalog(
        &mut conn,
        &catalog(&[("MK_Hauler", "Hauler", 64)], "v1"),
        T0,
    )
    .unwrap();
    let twin = ships::create_custom_ship(&mut conn, &input("Hauler", 70)).unwrap();
    let haulers: Vec<bool> = ships::list_ships(&conn)
        .unwrap()
        .into_iter()
        .filter(|s| s.name == "Hauler")
        .map(|s| s.is_custom)
        .collect();
    assert_eq!(haulers.len(), 2);
    assert!(haulers.contains(&true) && haulers.contains(&false));

    // Update: keeps its own name, refuses another custom ship's name, catalog ids are not editable.
    let edited =
        ships::update_custom_ship(&mut conn, &ship.id, &input("Retaliator cargo", 80)).unwrap();
    assert_eq!(
        (edited.name.as_str(), edited.cargo_scu),
        ("Retaliator cargo", 80)
    );
    assert_eq!(
        fields(ships::update_custom_ship(&mut conn, &ship.id, &input("big", 5)).unwrap_err()),
        ["name"]
    );
    assert_eq!(
        ships::update_custom_ship(&mut conn, "MK_Hauler", &input("X", 5)),
        Err(ShipError::Unknown("MK_Hauler".into()))
    );
    assert_eq!(
        ships::update_custom_ship(&mut conn, "custom:nope", &input("X", 5)),
        Err(ShipError::Unknown("custom:nope".into()))
    );

    // Delete: only custom ships.
    assert_eq!(
        ships::delete_custom_ship(&mut conn, "MK_Hauler"),
        Err(ShipError::Unknown("MK_Hauler".into()))
    );
    assert_eq!(ships::delete_custom_ship(&mut conn, &twin.id), Ok(false));
    assert!(ships::listed_ship(&conn, &twin.id).unwrap().is_none());
    assert!(
        ships::listed_ship(&conn, "MK_Hauler").unwrap().is_some(),
        "the catalog ship stays"
    );
}

#[test]
fn catalog_sync_never_touches_custom_ships() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    ships::sync_catalog(&mut conn, &catalog(&[("MK_A", "Alpha", 10)], "v1"), T0).unwrap();
    let custom = ships::create_custom_ship(&mut conn, &input("Alpha", 99)).unwrap();
    ships::set_favorite(&conn, &custom.id, true, T0).unwrap();
    let before = ships::listed_ship(&conn, &custom.id).unwrap().unwrap();

    for (version, list) in [
        ("v1", vec![("MK_A", "Alpha", 10)]),
        ("v1", vec![("MK_A", "Alpha", 10)]),
        ("v2", vec![("MK_B", "Bravo", 20)]), // Alpha leaves the catalog, the version changes
        ("v3", vec![]),
    ] {
        let report = ships::sync_catalog(&mut conn, &catalog(&list, version), T0 + 1).unwrap();
        let after = ships::listed_ship(&conn, &custom.id).unwrap();
        assert_eq!(
            after.as_ref(),
            Some(&before),
            "after sync {version}: {report:?}"
        );
    }
    let status: String = conn
        .query_row(
            "SELECT availability_status FROM ships WHERE id = ?1",
            [&custom.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "custom");
    let catalog_status: String = conn
        .query_row(
            "SELECT availability_status FROM ships WHERE id = 'MK_A'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        catalog_status, "removed",
        "catalog rows are still marked removed"
    );
    // The bundled catalog may not use the custom prefix.
    let bad = r#"{"source":"s","attribution":"a","game_version":"v","generated_at":"2026-11-20","items":[{"id":"custom:x","name":"X","manufacturer":"M","manufacturer_code":"M","cargo_scu":1,"max_container_scu":null}]}"#;
    assert!(ships::parse_ships(bad).is_err());
}

#[test]
fn favourites_selection_and_deletion() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    ships::sync_catalog(&mut conn, &catalog(&[("MK_A", "Alpha", 10)], "v1"), T0).unwrap();
    let a = ships::create_custom_ship(&mut conn, &input("Custom A", 40)).unwrap();
    let b = ships::create_custom_ship(&mut conn, &input("Custom B", 50)).unwrap();

    // Favourite and select like any ship; the selection survives a restart-like reload.
    assert!(
        ships::set_favorite(&conn, &a.id, true, T0)
            .unwrap()
            .is_favorite
    );
    let selected = ships::set_selected(&mut conn, Some(&a.id)).unwrap();
    assert_eq!(
        selected.ship.map(|s| (s.cargo_scu, s.is_custom)),
        Some((40, true))
    );
    let read = ships::get_selected(&mut conn).unwrap();
    assert_eq!(
        (read.ship.map(|s| s.id), read.stale),
        (Some(a.id.clone()), false)
    );

    // Deleting the selected favourite: the favourite goes, the selection is cleared (not "stale").
    assert_eq!(ships::delete_custom_ship(&mut conn, &a.id), Ok(true));
    let favs: i64 = conn
        .query_row("SELECT COUNT(*) FROM ship_favorites", [], |r| r.get(0))
        .unwrap();
    assert_eq!(favs, 0);
    assert_eq!(settings::load(&conn).unwrap().selected_ship_id, None);
    let after = ships::get_selected(&mut conn).unwrap();
    assert_eq!((after.ship, after.stale), (None, false));

    // Deleting a non-selected ship keeps the selection.
    ships::set_selected(&mut conn, Some("MK_A")).unwrap();
    assert_eq!(ships::delete_custom_ship(&mut conn, &b.id), Ok(false));
    assert_eq!(
        settings::load(&conn).unwrap().selected_ship_id.as_deref(),
        Some("MK_A")
    );
}

/* ------------------------------------------------------------- aUEC per hour */

#[test]
fn interval_union() {
    assert_eq!(union_duration_ms(&[]), 0);
    assert_eq!(
        union_duration_ms(&[(0, 30 * MIN), (10 * MIN, 50 * MIN)]),
        50 * MIN,
        "overlapping"
    );
    assert_eq!(
        union_duration_ms(&[(0, HOUR), (10 * MIN, 20 * MIN)]),
        HOUR,
        "nested"
    );
    assert_eq!(
        union_duration_ms(&[(0, 20 * MIN), (2 * HOUR, 2 * HOUR + 40 * MIN)]),
        HOUR,
        "disjoint: the gap is excluded"
    );
    assert_eq!(
        union_duration_ms(&[(0, 20 * MIN), (0, 20 * MIN), (0, 20 * MIN)]),
        20 * MIN,
        "identical"
    );
    assert_eq!(
        union_duration_ms(&[(0, 10 * MIN), (10 * MIN, 25 * MIN)]),
        25 * MIN,
        "touching"
    );
    assert_eq!(
        union_duration_ms(&[(50 * MIN, 60 * MIN), (0, 5 * MIN), (4 * MIN, 8 * MIN)]),
        18 * MIN,
        "unsorted"
    );
}

#[test]
fn rate_rounding_and_null_cases() {
    assert_eq!(
        auec_per_hour(802_750, 16, 2 * HOUR + 15 * MIN),
        Some(356_800),
        "356,777.8 → 356,800"
    );
    assert_eq!(auec_per_hour(100_000, 1, HOUR), Some(100_000));
    assert_eq!(
        auec_per_hour(10_049, 1, HOUR),
        Some(10_000),
        "rounds down below 50"
    );
    assert_eq!(
        auec_per_hour(10_050, 1, HOUR),
        Some(10_100),
        "half rounds up"
    );
    assert_eq!(
        auec_per_hour(50_000, 2, MIN_RATE_DURATION_MS - 1),
        None,
        "under 10 minutes"
    );
    assert_eq!(
        auec_per_hour(50_000, 2, MIN_RATE_DURATION_MS),
        Some(300_000),
        "exactly 10 minutes"
    );
    assert_eq!(auec_per_hour(0, 0, 3 * HOUR), None, "nothing completed");
}

fn contract(conn: &mut Connection, accepted: i64, local_date: &str, reward: &str) -> i64 {
    let valid = validate_new_contract(&NewContractInput {
        title: "Haul".into(),
        reward_auec: reward.into(),
        requirements: vec![NewRequirementInput {
            commodity_name: "Aluminum".into(),
            quantity_scu: "10".into(),
            max_box_size_scu: None,
            pickups: vec![NewStopInput {
                location_name: "A".into(),
                quantity_scu: None,
            }],
            deliveries: vec![NewStopInput {
                location_name: "B".into(),
                quantity_scu: None,
            }],
        }],
    })
    .unwrap();
    let at = AcceptanceTime {
        utc_ms: accepted,
        local_date: local_date.into(),
        utc_offset_minutes: 0,
    };
    create_contract_linked(
        conn,
        &valid,
        &at,
        ContractSource::Manual,
        &SaveLinks::default(),
    )
    .unwrap()
    .id
}

#[test]
fn runs_get_active_time_and_rate() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let contracts_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM contracts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(contracts_before, 0);
    // Run 1 (2026-11-20): two overlapping completed contracts, an abandoned one later the same
    // evening (time counts, money does not) and a contract accepted before midnight and finished
    // after it (stays in this run, its whole interval counts).
    let a = contract(&mut conn, T0, "2026-11-20", "60000"); // 12:00 → 13:00
    let b = contract(&mut conn, T0 + 30 * MIN, "2026-11-20", "40000"); // 12:30 → 13:30
    let c = contract(&mut conn, T0 + 6 * HOUR, "2026-11-20", "999999"); // 18:00 → 18:30, abandoned
    let d = contract(&mut conn, T0 + 11 * HOUR + 30 * MIN, "2026-11-20", "50000"); // 23:30 → 00:30
    finalize_contracts(&mut conn, &[a], FinalStatus::Completed, T0 + HOUR).unwrap();
    finalize_contracts(
        &mut conn,
        &[b],
        FinalStatus::Completed,
        T0 + HOUR + 30 * MIN,
    )
    .unwrap();
    finalize_contracts(
        &mut conn,
        &[c],
        FinalStatus::Abandoned,
        T0 + 6 * HOUR + 30 * MIN,
    )
    .unwrap();
    finalize_contracts(
        &mut conn,
        &[d],
        FinalStatus::Completed,
        T0 + 12 * HOUR + 30 * MIN,
    )
    .unwrap();
    // Run 2 (2026-11-21): one completed contract of 5 minutes → no rate.
    let e = contract(&mut conn, T0 + 24 * HOUR, "2026-11-21", "20000");
    finalize_contracts(
        &mut conn,
        &[e],
        FinalStatus::Completed,
        T0 + 24 * HOUR + 5 * MIN,
    )
    .unwrap();
    // Run 3 (2026-11-22): only abandoned → no rate.
    let f = contract(&mut conn, T0 + 48 * HOUR, "2026-11-22", "20000");
    finalize_contracts(&mut conn, &[f], FinalStatus::Abandoned, T0 + 50 * HOUR).unwrap();

    let h = history::get_history(&conn).unwrap();
    let run = |n: i64| h.runs.iter().find(|r| r.run_number == n).unwrap();
    let r1 = run(1);
    assert_eq!(
        r1.contracts.len(),
        4,
        "the after-midnight contract stays in its run"
    );
    // 12:00–13:30 (90 min) + 18:00–18:30 (30) + 23:30–00:30 (60) = 180 min.
    assert_eq!(r1.active_duration_ms, 3 * HOUR);
    assert_eq!(
        r1.earned_auec, 150_000,
        "the abandoned reward is not earned"
    );
    assert_eq!(r1.auec_per_hour, Some(50_000));
    let r2 = run(2);
    assert_eq!((r2.active_duration_ms, r2.auec_per_hour), (5 * MIN, None));
    let r3 = run(3);
    assert_eq!((r3.active_duration_ms, r3.auec_per_hour), (2 * HOUR, None));
    // Serialized names for the frontend.
    let json = serde_json::to_value(r1).unwrap();
    assert_eq!(json["activeDurationMs"], 3 * HOUR);
    assert_eq!(json["auecPerHour"], 50_000);
    assert!(serde_json::to_value(r2).unwrap()["auecPerHour"].is_null());
}

#[test]
fn custom_ships_never_touch_contracts() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let id = contract(&mut conn, T0, "2026-11-20", "1000");
    let before: String = conn
        .query_row(
            "SELECT title || status || run_id || updated_at FROM contracts WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let ship = ships::create_custom_ship(&mut conn, &input("Mine", 12)).unwrap();
    ships::set_selected(&mut conn, Some(&ship.id)).unwrap();
    ships::update_custom_ship(&mut conn, &ship.id, &input("Mine 2", 14)).unwrap();
    ships::delete_custom_ship(&mut conn, &ship.id).unwrap();
    let after: String = conn
        .query_row(
            "SELECT title || status || run_id || updated_at FROM contracts WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(before, after);
}
