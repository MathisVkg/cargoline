//! Headless tests for manual contract creation. Temporary databases only.

use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;

use cargo_line_lib::contracts::repository::{create_contract, list_active_contracts};
use cargo_line_lib::contracts::validation::{
    parse_reward_auec, parse_scu_milli, validate_new_contract,
};
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::connection::open_connection;
use cargo_line_lib::database::models::{ContractSource, ContractStatus};
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use rusqlite::Connection;
use tempfile::TempDir;

fn temp_db() -> (TempDir, PathBuf, Database) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE_NAME);
    let db = Database::open(&path).unwrap();
    (dir, path, db)
}

fn day(date: &str, utc_ms: i64) -> AcceptanceTime {
    AcceptanceTime {
        utc_ms,
        local_date: date.into(),
        utc_offset_minutes: 60,
    }
}

fn stop(location: &str, qty: Option<&str>) -> NewStopInput {
    NewStopInput {
        location_name: location.into(),
        quantity_scu: qty.map(Into::into),
    }
}

fn req(
    commodity: &str,
    qty: &str,
    pickups: Vec<NewStopInput>,
    deliveries: Vec<NewStopInput>,
) -> NewRequirementInput {
    NewRequirementInput {
        commodity_name: commodity.into(),
        quantity_scu: qty.into(),
        max_box_size_scu: None,
        pickups,
        deliveries,
    }
}

fn contract(title: &str, reward: &str, requirements: Vec<NewRequirementInput>) -> NewContractInput {
    NewContractInput {
        title: title.into(),
        reward_auec: reward.into(),
        requirements,
    }
}

/// Example A: one pickup -> one destination.
fn example_a() -> NewContractInput {
    contract(
        "Direct Small Cargo Haul",
        "18,500",
        vec![req(
            "Stims",
            "12",
            vec![stop("Port Tressler", None)],
            vec![stop("Devlin Scrap & Salvage", None)],
        )],
    )
}

/// Example B: two commodities, two pickups each (quantities unknown), one destination.
fn example_b() -> NewContractInput {
    let pickups = || {
        vec![
            stop("microTech Logistics Depot S4LD01", None),
            stop("Sakura Sun Goldenrod Workcenter", None),
        ]
    };
    contract(
        "Small Cargo Haul",
        "24250",
        vec![
            req("Waste", "4", pickups(), vec![stop("Port Tressler", None)]),
            req("Scrap", "6", pickups(), vec![stop("Port Tressler", None)]),
        ],
    )
}

/// Example C: one pickup -> two destinations with known split.
fn example_c() -> NewContractInput {
    contract(
        "Extra Small Cargo Haul",
        "9000",
        vec![req(
            "Aluminum",
            "9",
            vec![stop("Port Tressler", None)],
            vec![
                stop("Sakura Sun Goldenrod Workcenter", Some("5")),
                stop("Greycat Stanton IV Production Complex-A", Some("4")),
            ],
        )],
    )
}

fn save(conn: &mut Connection, input: &NewContractInput, at: &AcceptanceTime) -> ContractDetail {
    let valid = validate_new_contract(input).expect("valid input");
    create_contract(conn, &valid, at, ContractSource::Manual).expect("saved")
}

fn issue_paths(input: &NewContractInput) -> Vec<String> {
    let mut paths: Vec<String> = validate_new_contract(input)
        .unwrap_err()
        .into_iter()
        .map(|i| i.path)
        .collect();
    paths.sort();
    paths
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[test]
fn scu_text_is_parsed_exactly_without_floating_point() {
    assert_eq!(parse_scu_milli("12"), Ok(12_000));
    assert_eq!(parse_scu_milli(" 4.5 "), Ok(4_500));
    assert_eq!(parse_scu_milli("0,125"), Ok(125));
    assert_eq!(parse_scu_milli("0.1"), Ok(100));
    assert_eq!(parse_scu_milli("007"), Ok(7_000));
    assert_eq!(parse_scu_milli("0"), Ok(0));
    assert_eq!(parse_scu_milli("1000000"), Ok(1_000_000_000));
    for bad in [
        "",
        "abc",
        "-1",
        "+1",
        "1.",
        ".5",
        "1.2.3",
        "1e3",
        "1 000",
        "1.0001",
        "1000001",
        "99999999999999999999",
    ] {
        assert!(parse_scu_milli(bad).is_err(), "{bad:?} must be rejected");
    }
}

#[test]
fn reward_text_is_parsed_as_whole_auec() {
    assert_eq!(parse_reward_auec("48250"), Ok(48_250));
    assert_eq!(parse_reward_auec("48,250"), Ok(48_250));
    assert_eq!(parse_reward_auec("1 250 000"), Ok(1_250_000));
    assert_eq!(parse_reward_auec("0"), Ok(0));
    for bad in [
        "",
        "-5",
        "12.5",
        "48,25",
        "4,8250",
        "1,000 000",
        "abc",
        "10000000000000",
    ] {
        assert!(parse_reward_auec(bad).is_err(), "{bad:?} must be rejected");
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[test]
fn required_fields_are_reported_with_field_paths() {
    let input = contract(
        "  ",
        "-1",
        vec![req("", "0", vec![stop(" ", None)], vec![])],
    );
    assert_eq!(
        issue_paths(&input),
        [
            "requirements.0.commodityName",
            "requirements.0.deliveries",
            "requirements.0.pickups.0.locationName",
            "requirements.0.quantityScu",
            "rewardAuec",
            "title",
        ]
    );
    assert_eq!(issue_paths(&contract("T", "1", vec![])), ["requirements"]);
}

#[test]
fn optional_quantities_and_box_sizes_are_validated() {
    let mut r = req(
        "Gold",
        "10",
        vec![stop("A", Some("0"))],
        vec![stop("B", Some("abc"))],
    );
    r.max_box_size_scu = Some("0".into());
    assert_eq!(
        issue_paths(&contract("T", "1", vec![r])),
        [
            "requirements.0.deliveries.0.quantityScu",
            "requirements.0.maxBoxSizeScu",
            "requirements.0.pickups.0.quantityScu"
        ]
    );

    // Empty optional text means "unknown", not zero.
    let mut r = req(
        "Gold",
        "10",
        vec![stop("A", Some(""))],
        vec![stop("B", Some("  "))],
    );
    r.max_box_size_scu = Some("".into());
    let valid = validate_new_contract(&contract("T", "1", vec![r])).unwrap();
    assert_eq!(valid.requirements[0].pickups[0].quantity_scu_milli, None);
    assert_eq!(valid.requirements[0].max_box_size_scu_milli, None);
}

#[test]
fn known_allocations_must_not_contradict_the_total() {
    // All delivery quantities known but adding up to 8 of 9 SCU.
    let input = contract(
        "T",
        "1",
        vec![req(
            "Al",
            "9",
            vec![stop("P", None)],
            vec![stop("D1", Some("5")), stop("D2", Some("3"))],
        )],
    );
    assert_eq!(issue_paths(&input), ["requirements.0.deliveries"]);

    // A single stop with a quantity must carry the whole total.
    let input = contract(
        "T",
        "1",
        vec![req(
            "Al",
            "9",
            vec![stop("P", Some("8"))],
            vec![stop("D", None)],
        )],
    );
    assert_eq!(issue_paths(&input), ["requirements.0.pickups"]);

    // Known part already covers everything: unknown stops would be empty.
    let input = contract(
        "T",
        "1",
        vec![req(
            "Al",
            "9",
            vec![stop("P1", Some("9")), stop("P2", None)],
            vec![stop("D", None)],
        )],
    );
    assert_eq!(issue_paths(&input), ["requirements.0.pickups"]);

    // Partial knowledge is kept as-is without inventing the rest.
    let input = contract(
        "T",
        "1",
        vec![req(
            "Al",
            "9",
            vec![stop("P1", Some("4")), stop("P2", None)],
            vec![stop("D", Some("9"))],
        )],
    );
    let valid = validate_new_contract(&input).unwrap();
    let pickups: Vec<_> = valid.requirements[0]
        .pickups
        .iter()
        .map(|s| s.quantity_scu_milli)
        .collect();
    assert_eq!(pickups, [Some(4_000), None]);
}

#[test]
fn duplicate_locations_within_one_operation_are_rejected() {
    let input = contract(
        "T",
        "1",
        vec![req(
            "Gold",
            "2",
            vec![stop("Port Tressler", None), stop("port tressler ", None)],
            vec![stop("Port Tressler", None)],
        )],
    );
    // Same location for pickup and delivery is allowed; twice as pickup is not.
    assert_eq!(
        issue_paths(&input),
        ["requirements.0.pickups.1.locationName"]
    );
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

#[test]
fn simple_contract_is_saved_and_returned_with_nested_cargo() {
    let (_dir, _path, db) = temp_db();
    let mut conn = db.connection().unwrap();
    let saved = save(&mut conn, &example_a(), &day("2026-11-19", 1_000));

    assert_eq!(saved.title, "Direct Small Cargo Haul");
    assert_eq!(saved.reward_auec, 18_500);
    assert_eq!(saved.status, ContractStatus::Active);
    assert_eq!(saved.source, ContractSource::Manual);
    assert_eq!(saved.accepted_at, 1_000);
    assert_eq!(saved.total_scu_milli, 12_000);
    assert_eq!(saved.run.run_number, 1);
    assert_eq!(saved.run.run_date, "2026-11-19");
    let r = &saved.requirements[0];
    assert_eq!(
        (r.commodity_name.as_str(), r.quantity_scu_milli),
        ("Stims", 12_000)
    );
    assert_eq!(r.pickups[0].location_name, "Port Tressler");
    assert_eq!(r.deliveries[0].location_name, "Devlin Scrap & Salvage");
    assert_eq!(r.pickups[0].quantity_scu_milli, None);

    assert_eq!(list_active_contracts(&conn).unwrap(), vec![saved]);
}

#[test]
fn multi_commodity_multi_pickup_contract_keeps_unknown_quantities_and_totals_once() {
    let (_dir, _path, db) = temp_db();
    let mut conn = db.connection().unwrap();
    let saved = save(&mut conn, &example_b(), &day("2026-11-19", 1_000));

    assert_eq!(saved.requirements.len(), 2);
    // 4 + 6 SCU: requirements only, never pickups + deliveries.
    assert_eq!(saved.total_scu_milli, 10_000);
    for r in &saved.requirements {
        let pickups: Vec<_> = r
            .pickups
            .iter()
            .map(|s| (s.location_name.as_str(), s.quantity_scu_milli))
            .collect();
        assert_eq!(
            pickups,
            [
                ("microTech Logistics Depot S4LD01", None),
                ("Sakura Sun Goldenrod Workcenter", None)
            ]
        );
        assert_eq!(r.deliveries.len(), 1);
    }
    assert_eq!(count(&conn, "cargo_stops"), 6);
}

#[test]
fn multi_destination_split_is_preserved_in_order() {
    let (_dir, _path, db) = temp_db();
    let mut conn = db.connection().unwrap();
    let saved = save(&mut conn, &example_c(), &day("2026-11-19", 1_000));
    let deliveries: Vec<_> = saved.requirements[0]
        .deliveries
        .iter()
        .map(|s| (s.location_name.as_str(), s.quantity_scu_milli))
        .collect();
    assert_eq!(
        deliveries,
        [
            ("Sakura Sun Goldenrod Workcenter", Some(5_000)),
            ("Greycat Stanton IV Production Complex-A", Some(4_000))
        ]
    );
    assert_eq!(saved.total_scu_milli, 9_000);
}

#[test]
fn multiple_pickups_to_multiple_destinations_with_decimal_scu() {
    let (_dir, _path, db) = temp_db();
    let mut conn = db.connection().unwrap();
    let mut r = req(
        "Quantainium",
        "7.5",
        vec![stop("HUR-L1", Some("2.25")), stop("HUR-L2", None)],
        vec![
            stop("Lorville", None),
            stop("Area18", None),
            stop("Orison", Some("0.5")),
        ],
    );
    r.max_box_size_scu = Some("2".into());
    let saved = save(
        &mut conn,
        &contract("Mixed", "0", vec![r]),
        &day("2026-11-19", 1_000),
    );
    let r = &saved.requirements[0];
    assert_eq!(r.quantity_scu_milli, 7_500);
    assert_eq!(r.max_box_size_scu_milli, Some(2_000));
    assert_eq!((r.pickups.len(), r.deliveries.len()), (2, 3));
    assert_eq!(r.pickups[0].quantity_scu_milli, Some(2_250));
    assert_eq!(r.deliveries[2].quantity_scu_milli, Some(500));
}

#[test]
fn runs_are_assigned_by_local_date_and_reused() {
    let (_dir, _path, db) = temp_db();
    let mut conn = db.connection().unwrap();
    let first = save(&mut conn, &example_a(), &day("2026-11-19", 1_000));
    let same_day = save(&mut conn, &example_b(), &day("2026-11-19", 2_000));
    let next_day = save(&mut conn, &example_c(), &day("2026-11-20", 3_000));

    assert_eq!(first.run, same_day.run);
    assert_eq!(first.run.run_number, 1);
    assert_eq!(next_day.run.run_number, 2);
    assert_eq!(next_day.run.run_date, "2026-11-20");
    assert_eq!(count(&conn, "runs"), 2);

    // Newest first.
    let titles: Vec<_> = list_active_contracts(&conn)
        .unwrap()
        .into_iter()
        .map(|c| c.title)
        .collect();
    assert_eq!(
        titles,
        [
            "Extra Small Cargo Haul",
            "Small Cargo Haul",
            "Direct Small Cargo Haul"
        ]
    );
}

#[test]
fn failed_save_rolls_back_everything_including_a_new_run() {
    let (_dir, _path, db) = temp_db();
    let mut conn = db.connection().unwrap();
    // Force the last insert of the transaction to fail.
    conn.execute_batch(
        "CREATE TEMP TRIGGER fail_stops BEFORE INSERT ON cargo_stops
         WHEN NEW.location_name = 'Fail here' BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;",
    )
    .unwrap();
    let input = contract(
        "Doomed",
        "100",
        vec![req(
            "Gold",
            "2",
            vec![stop("Port Tressler", None)],
            vec![stop("Fail here", None)],
        )],
    );
    let valid = validate_new_contract(&input).unwrap();
    let err = create_contract(
        &mut conn,
        &valid,
        &day("2026-11-19", 1_000),
        ContractSource::Manual,
    )
    .unwrap_err();
    assert!(err.to_string().contains("simulated failure"));

    for table in ["runs", "contracts", "cargo_requirements", "cargo_stops"] {
        assert_eq!(
            count(&conn, table),
            0,
            "{table} must be empty after rollback"
        );
    }

    // The connection stays usable and the next contract creates run #1.
    let saved = save(&mut conn, &example_a(), &day("2026-11-19", 2_000));
    assert_eq!(saved.run.run_number, 1);
}

#[test]
fn saved_contracts_survive_reopening_the_database() {
    let (_dir, path, db) = temp_db();
    let saved = {
        let mut conn = db.connection().unwrap();
        vec![
            save(&mut conn, &example_c(), &day("2026-11-19", 1_000)),
            save(&mut conn, &example_b(), &day("2026-11-19", 2_000)),
        ]
    };
    drop(db);
    let db = Database::open(&path).unwrap();
    let listed = list_active_contracts(&db.connection().unwrap()).unwrap();
    assert_eq!(listed, saved.into_iter().rev().collect::<Vec<_>>());
}

#[test]
fn only_active_contracts_are_listed() {
    let (_dir, _path, db) = temp_db();
    let mut conn = db.connection().unwrap();
    let done = save(&mut conn, &example_a(), &day("2026-11-19", 1_000));
    save(&mut conn, &example_b(), &day("2026-11-19", 2_000));
    conn.execute(
        "UPDATE contracts SET status = 'completed', finished_at = 5000, updated_at = 5000 WHERE id = ?1",
        [done.id],
    )
    .unwrap();
    let titles: Vec<_> = list_active_contracts(&conn)
        .unwrap()
        .into_iter()
        .map(|c| c.title)
        .collect();
    assert_eq!(titles, ["Small Cargo Haul"]);
}

#[test]
fn concurrent_creations_on_one_date_share_a_single_run() {
    let (_dir, path, db) = temp_db();
    drop(db);
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let path = path.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let mut conn = open_connection(&path).unwrap();
                barrier.wait();
                save(&mut conn, &example_a(), &day("2026-11-19", 1_000 + i)).run
            })
        })
        .collect();
    let runs: Vec<RunSummary> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(
        runs.windows(2).all(|w| w[0] == w[1]),
        "all contracts share one run"
    );
    let conn = open_connection(&path).unwrap();
    assert_eq!(count(&conn, "runs"), 1);
    assert_eq!(count(&conn, "contracts"), 8);
}

#[test]
fn database_errors_surface_without_partial_writes() {
    let (_dir, _path, db) = temp_db();
    let mut conn = db.connection().unwrap();
    // Simulate a damaged schema: the stops table is missing.
    conn.execute_batch("DROP TABLE cargo_box_groups;").unwrap();
    conn.execute_batch("ALTER TABLE cargo_stops RENAME TO cargo_stops_gone;")
        .unwrap();
    let valid = validate_new_contract(&example_a()).unwrap();
    assert!(create_contract(
        &mut conn,
        &valid,
        &day("2026-11-19", 1_000),
        ContractSource::Manual
    )
    .is_err());
    assert_eq!(count(&conn, "contracts"), 0);
    assert_eq!(count(&conn, "runs"), 0);
}

// ---------------------------------------------------------------------------
// Frontend contract (JSON shape)
// ---------------------------------------------------------------------------

#[test]
fn frontend_payload_shape_deserializes_and_output_is_camel_case() {
    let payload = r#"{
        "title": "Small Cargo Haul",
        "rewardAuec": "24,250",
        "requirements": [{
            "commodityName": "Waste",
            "quantityScu": "4",
            "maxBoxSizeScu": null,
            "pickups": [{ "locationName": "Depot S4LD01", "quantityScu": null },
                        { "locationName": "Goldenrod", "quantityScu": "" }],
            "deliveries": [{ "locationName": "Port Tressler", "quantityScu": "4" }]
        }]
    }"#;
    let input: NewContractInput = serde_json::from_str(payload).unwrap();
    let valid = validate_new_contract(&input).unwrap();

    let (_dir, _path, db) = temp_db();
    let saved = create_contract(
        &mut db.connection().unwrap(),
        &valid,
        &day("2026-11-19", 1),
        ContractSource::Manual,
    )
    .unwrap();
    let json = serde_json::to_value(&saved).unwrap();
    for key in [
        "id",
        "run",
        "title",
        "rewardAuec",
        "status",
        "source",
        "acceptedAt",
        "totalScuMilli",
        "requirements",
    ] {
        assert!(json.get(key).is_some(), "missing {key}");
    }
    assert_eq!(json["status"], "active");
    assert_eq!(json["source"], "manual");
    assert_eq!(json["run"]["runNumber"], 1);
    let req = &json["requirements"][0];
    assert_eq!(req["quantityScuMilli"], 4_000);
    assert!(req["maxBoxSizeScuMilli"].is_null());
    assert_eq!(req["pickups"][1]["locationName"], "Goldenrod");
    assert!(req["pickups"][1]["quantityScuMilli"].is_null());
    assert_eq!(req["deliveries"][0]["quantityScuMilli"], 4_000);

    // Unknown fields are rejected rather than silently ignored.
    let extra = payload.replacen("\"title\"", "\"shipId\": \"x\", \"title\"", 1);
    assert!(serde_json::from_str::<NewContractInput>(&extra).is_err());
}

// ---------------------------------------------------------------------------
// Box groups in contract details, grouping preference
// ---------------------------------------------------------------------------

#[test]
fn stored_box_groups_are_returned_and_absent_ones_stay_empty() {
    use cargo_line_lib::database::models::BoxSourceQuality;
    let (_dir, _path, db) = temp_db();
    let mut conn = db.connection().unwrap();
    let saved = save(&mut conn, &example_b(), &day("2026-11-19", 1_000));
    assert!(saved.requirements.iter().all(|r| r.box_groups.is_empty()));

    let waste = saved.requirements[0].id;
    conn.execute(
        "INSERT INTO cargo_box_groups (requirement_id, box_size_scu_milli, box_count, source_quality)
         VALUES (?1, 1000, 2, 'exact'), (?1, 2000, 1, 'exact')",
        [waste],
    )
    .unwrap();
    let listed = list_active_contracts(&conn).unwrap();
    let groups: Vec<_> = listed[0].requirements[0]
        .box_groups
        .iter()
        .map(|b| (b.box_size_scu_milli, b.box_count, b.source_quality))
        .collect();
    assert_eq!(
        groups,
        [
            (2_000, 1, BoxSourceQuality::Exact),
            (1_000, 2, BoxSourceQuality::Exact)
        ]
    );
    assert!(listed[0].requirements[1].box_groups.is_empty());

    let json = serde_json::to_value(&listed[0]).unwrap();
    assert_eq!(
        json["requirements"][0]["boxGroups"][0]["boxSizeScuMilli"],
        2_000
    );
    assert_eq!(
        json["requirements"][0]["boxGroups"][0]["sourceQuality"],
        "exact"
    );
}

#[test]
fn contract_grouping_defaults_to_by_location_persists_and_accepts_legacy_values() {
    use cargo_line_lib::settings::{self, ContractGrouping};
    let (_dir, path, db) = temp_db();
    {
        let mut conn = db.connection().unwrap();
        assert_eq!(
            settings::load(&conn).unwrap().contract_grouping,
            ContractGrouping::ByLocation
        );
        settings::update(&mut conn, |s| {
            s.contract_grouping = ContractGrouping::ByContract
        })
        .unwrap();
        let stored: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'contract_grouping'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, "\"by_contract\"");
        // Changing the display mode never touches contract data.
        assert_eq!(count(&conn, "contracts"), 0);
    }
    drop(db);
    let db = Database::open(&path).unwrap();
    let conn = db.connection().unwrap();
    assert_eq!(
        settings::load(&conn).unwrap().contract_grouping,
        ContractGrouping::ByContract
    );

    for legacy in ["\"by_stop\"", "\"locations\"", "\"by_location\""] {
        conn.execute(
            "UPDATE app_settings SET value = ?1 WHERE key = 'contract_grouping'",
            [legacy],
        )
        .unwrap();
        assert_eq!(
            settings::load(&conn).unwrap().contract_grouping,
            ContractGrouping::ByLocation,
            "{legacy}"
        );
    }
    assert_eq!(
        serde_json::to_string(&ContractGrouping::ByLocation).unwrap(),
        "\"by_location\""
    );
}
