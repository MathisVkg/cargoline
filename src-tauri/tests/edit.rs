//! Headless tests for editing contracts and Pick Up / Turn In reminders.
//! Temporary databases only.

use cargo_line_lib::contracts::edit::{set_stop_reminders, update_contract};
use cargo_line_lib::contracts::finalize::{finalize_contract, FinalStatus, FinalizeError};
use cargo_line_lib::contracts::repository::{
    create_contract, list_active_contracts, load_contract,
};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::connection::open_connection;
use cargo_line_lib::database::migrations::{migrations, LATEST_SCHEMA_VERSION};
use cargo_line_lib::database::models::{ContractSource, ContractStatus};
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::locations::list_locations;
use rusqlite::Connection;
use tempfile::TempDir;

const T0: i64 = 1_795_600_000_000;

fn fresh() -> (TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    (dir, db)
}

fn stop(name: &str, qty: Option<&str>) -> NewStopInput {
    NewStopInput {
        location_name: name.into(),
        quantity_scu: qty.map(Into::into),
    }
}

fn req(
    commodity: &str,
    qty: &str,
    max_box: Option<&str>,
    pickups: Vec<NewStopInput>,
    deliveries: Vec<NewStopInput>,
) -> NewRequirementInput {
    NewRequirementInput {
        commodity_name: commodity.into(),
        quantity_scu: qty.into(),
        max_box_size_scu: max_box.map(Into::into),
        pickups,
        deliveries,
    }
}

/// Aluminum 9 (Port Tressler -> Sakura 5 / Greycat 4) + Waste 4 (HUR-L1, HUR-L2 -> Port Tressler).
fn base_input() -> NewContractInput {
    NewContractInput {
        title: "Original".into(),
        reward_auec: "10000".into(),
        requirements: vec![
            req(
                "Aluminum",
                "9",
                Some("4"),
                vec![stop("Port Tressler", None)],
                vec![
                    stop("Sakura Sun Goldenrod Workcenter", Some("5")),
                    stop("Greycat Stanton IV Production Complex-A", Some("4")),
                ],
            ),
            req(
                "Waste",
                "4",
                None,
                vec![stop("HUR-L1", None), stop("HUR-L2", None)],
                vec![stop("Port Tressler", None)],
            ),
        ],
    }
}

fn add(conn: &mut Connection) -> ContractDetail {
    let at = AcceptanceTime {
        utc_ms: T0,
        local_date: "2026-11-19".into(),
        utc_offset_minutes: 60,
    };
    let saved = create_contract(
        conn,
        &validate_new_contract(&base_input()).unwrap(),
        &at,
        ContractSource::Manual,
    )
    .unwrap();
    // Stored box data the form never shows.
    conn.execute(
        "INSERT INTO cargo_box_groups (requirement_id, box_size_scu_milli, box_count, source_quality)
         VALUES (?1, 4000, 2, 'exact'), (?1, 1000, 1, 'exact')",
        [saved.requirements[0].id],
    )
    .unwrap();
    load_contract(conn, saved.id).unwrap().unwrap()
}

fn edit(
    conn: &mut Connection,
    id: i64,
    input: &NewContractInput,
    now: i64,
) -> Result<ContractDetail, FinalizeError> {
    update_contract(conn, id, &validate_new_contract(input).unwrap(), now)
}

fn stop_ids(c: &ContractDetail) -> Vec<i64> {
    c.requirements
        .iter()
        .flat_map(|r| r.pickups.iter().chain(r.deliveries.iter()).map(|s| s.id))
        .collect()
}

/// (commodity, operation-tagged location, ticked?) for every stop.
fn ticks(c: &ContractDetail) -> Vec<(String, String, bool)> {
    c.requirements
        .iter()
        .flat_map(|r| {
            let p = r.pickups.iter().map(move |s| {
                (
                    r.commodity_name.clone(),
                    format!("P:{}", s.location_name),
                    s.reminder_done_at.is_some(),
                )
            });
            let d = r.deliveries.iter().map(move |s| {
                (
                    r.commodity_name.clone(),
                    format!("D:{}", s.location_name),
                    s.reminder_done_at.is_some(),
                )
            });
            p.chain(d)
        })
        .collect()
}

fn contract_row(
    conn: &Connection,
    id: i64,
) -> (String, i64, i64, i64, i64, String, Option<String>) {
    conn.query_row(
        "SELECT status, run_id, accepted_at, created_at, updated_at, source, game_mission_id FROM contracts WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
    )
    .unwrap()
}

/* ---------- update_contract ---------- */

#[test]
fn title_and_reward_only_keeps_cargo_ticks_and_box_groups() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let before = add(&mut conn);
    set_stop_reminders(
        &mut conn,
        &[before.requirements[0].pickups[0].id],
        true,
        T0 + 1,
    )
    .unwrap();
    let before = load_contract(&conn, before.id).unwrap().unwrap();
    let row_before = contract_row(&conn, before.id);

    let mut input = base_input();
    input.title = "Renamed".into();
    input.reward_auec = "12,500".into();
    let after = edit(&mut conn, before.id, &input, T0 + 10).unwrap();

    assert_eq!(after.id, before.id, "C01 stays C01");
    assert_eq!(
        (after.title.as_str(), after.reward_auec),
        ("Renamed", 12_500)
    );
    assert_eq!(
        (after.status, after.run.clone(), after.accepted_at),
        (
            ContractStatus::Active,
            before.run.clone(),
            before.accepted_at
        )
    );
    assert_eq!(after.total_scu_milli, before.total_scu_milli);
    assert_eq!(ticks(&after), ticks(&before), "ticks untouched");
    assert_eq!(
        after.requirements[0].box_groups, before.requirements[0].box_groups,
        "box groups untouched"
    );
    let strip_ids = |c: &ContractDetail| {
        c.requirements
            .iter()
            .map(|r| {
                let stops = |v: &Vec<StopDetail>| {
                    v.iter()
                        .map(|s| {
                            (
                                s.location_name.clone(),
                                s.quantity_scu_milli,
                                s.reminder_done_at,
                            )
                        })
                        .collect::<Vec<_>>()
                };
                (
                    r.commodity_name.clone(),
                    r.quantity_scu_milli,
                    r.max_box_size_scu_milli,
                    stops(&r.pickups),
                    stops(&r.deliveries),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(strip_ids(&after), strip_ids(&before));
    let row_after = contract_row(&conn, before.id);
    assert_eq!(
        (&row_after.0, row_after.1, row_after.2, row_after.3),
        (&row_before.0, row_before.1, row_before.2, row_before.3)
    );
    assert_eq!(row_after.4, T0 + 10, "updated_at moves forward");
    assert_eq!(
        (row_after.5.as_str(), row_after.6.as_deref()),
        ("manual", None),
        "source and mission id untouched"
    );
}

#[test]
fn add_remove_and_change_requirements_and_stops() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let before = add(&mut conn);
    let input = NewContractInput {
        title: "Original".into(),
        reward_auec: "10000".into(),
        requirements: vec![
            // Aluminum: total 9 -> 12, one delivery replaced, split changed.
            req(
                "Aluminum",
                "12",
                Some("4"),
                vec![stop("Port Tressler", None)],
                vec![
                    stop("Sakura Sun Goldenrod Workcenter", Some("6")),
                    stop("Area18", Some("6")),
                ],
            ),
            // Waste removed; Stims added.
            req(
                "Stims",
                "2.5",
                None,
                vec![stop("Everus Harbor", None)],
                vec![stop("Seraphim Station", None)],
            ),
        ],
    };
    let after = edit(&mut conn, before.id, &input, T0 + 10).unwrap();
    assert_eq!(after.id, before.id);
    assert_eq!(after.requirements.len(), 2);
    assert_eq!(after.requirements[0].quantity_scu_milli, 12_000);
    assert_eq!(after.requirements[1].commodity_name, "Stims");
    assert_eq!(after.total_scu_milli, 14_500);
    let deliveries: Vec<_> = after.requirements[0]
        .deliveries
        .iter()
        .map(|s| (s.location_name.as_str(), s.quantity_scu_milli))
        .collect();
    assert_eq!(
        deliveries,
        [
            ("Sakura Sun Goldenrod Workcenter", Some(6_000)),
            ("Area18", Some(6_000))
        ]
    );
    let counts: (i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM cargo_requirements), (SELECT COUNT(*) FROM cargo_stops)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (2, 5), "old rows replaced, nothing left over");
    assert_eq!(after.run, before.run);
}

#[test]
fn ticks_kept_on_unchanged_stops_and_dropped_on_changed_ones() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let before = add(&mut conn);
    // Tick everything.
    set_stop_reminders(&mut conn, &stop_ids(&before), true, T0 + 1).unwrap();

    let mut input = base_input();
    // Aluminum: Greycat delivery replaced by Area18 (changed stop); Sakura and the pickup unchanged.
    input.requirements[0].deliveries[1] = stop("Area18", Some("4"));
    // Waste renamed to Scrap: every Waste stop counts as changed.
    input.requirements[1].commodity_name = "Scrap".into();
    // Quantity change on an unchanged location keeps the tick (key = commodity, location, operation).
    input.requirements[0].deliveries[0] = stop("sakura sun goldenrod workcenter", Some("5"));
    let after = edit(&mut conn, before.id, &input, T0 + 10).unwrap();
    assert_eq!(
        ticks(&after),
        [
            ("Aluminum".into(), "P:Port Tressler".into(), true),
            (
                "Aluminum".into(),
                "D:Sakura Sun Goldenrod Workcenter".into(),
                true
            ),
            ("Aluminum".into(), "D:Area18".into(), false),
            ("Scrap".into(), "P:HUR-L1".into(), false),
            ("Scrap".into(), "P:HUR-L2".into(), false),
            ("Scrap".into(), "D:Port Tressler".into(), false),
        ]
    );
    // The original tick timestamp is kept, not reset.
    assert_eq!(
        after.requirements[0].pickups[0].reminder_done_at,
        Some(T0 + 1)
    );
}

#[test]
fn box_groups_kept_on_unchanged_requirements_only() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let before = add(&mut conn);
    assert_eq!(before.requirements[0].box_groups.len(), 2);

    // Stops changed but commodity / total / max box unchanged: kept.
    let mut input = base_input();
    input.requirements[0].pickups[0] = stop("Everus Harbor", None);
    let after = edit(&mut conn, before.id, &input, T0 + 10).unwrap();
    assert_eq!(
        after.requirements[0].box_groups,
        before.requirements[0].box_groups
    );

    // Total changed: the stored breakdown no longer applies and is dropped.
    input.requirements[0].quantity_scu = "10".into();
    input.requirements[0].deliveries[1] =
        stop("Greycat Stanton IV Production Complex-A", Some("5"));
    let changed = edit(&mut conn, before.id, &input, T0 + 20).unwrap();
    assert!(changed.requirements[0].box_groups.is_empty());
}

#[test]
fn finished_unknown_and_invalid_updates_are_rejected() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let c = add(&mut conn);
    assert!(matches!(
        edit(&mut conn, 4_242, &base_input(), T0),
        Err(FinalizeError::NotFound(4_242))
    ));
    let mut bad = base_input();
    bad.title = " ".into();
    bad.requirements[0].quantity_scu = "0".into();
    let issues = validate_new_contract(&bad).unwrap_err();
    assert_eq!(
        issues.iter().map(|i| i.path.as_str()).collect::<Vec<_>>(),
        ["title", "requirements.0.quantityScu"]
    );
    finalize_contract(&mut conn, c.id, FinalStatus::Completed, T0 + 5).unwrap();
    match edit(&mut conn, c.id, &base_input(), T0 + 6) {
        Err(FinalizeError::NotActive { id, status }) => {
            assert_eq!((id, status), (c.id, "completed"))
        }
        other => panic!("expected NotActive, got {other:?}"),
    }
    assert_eq!(
        load_contract(&conn, c.id).unwrap().unwrap().title,
        "Original",
        "nothing changed"
    );
}

#[test]
fn updated_at_never_goes_backwards() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let c = add(&mut conn);
    edit(&mut conn, c.id, &base_input(), T0 + 100).unwrap();
    edit(&mut conn, c.id, &base_input(), T0 + 50).unwrap(); // clock went back
    assert_eq!(contract_row(&conn, c.id).4, T0 + 100);
}

#[test]
fn new_locations_are_catalogued_and_rolled_back_on_failure() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let c = add(&mut conn);
    let mut input = base_input();
    input.requirements[1].deliveries[0] = stop("Brand New Station", None);
    edit(&mut conn, c.id, &input, T0 + 10).unwrap();
    let names = |conn: &Connection| {
        list_locations(conn)
            .unwrap()
            .into_iter()
            .map(|l| l.name)
            .collect::<Vec<_>>()
    };
    assert!(names(&conn).contains(&"Brand New Station".to_string()));

    conn.execute_batch(
        "CREATE TEMP TRIGGER fail BEFORE INSERT ON cargo_stops WHEN NEW.location_name = 'Doomed'
         BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;",
    )
    .unwrap();
    let mut doomed = base_input();
    doomed.title = "Should not stick".into();
    doomed.requirements[0].pickups[0] = stop("Another New Place", None);
    doomed.requirements[1].deliveries[0] = stop("Doomed", None);
    assert!(edit(&mut conn, c.id, &doomed, T0 + 20).is_err());
    assert!(
        !names(&conn).contains(&"Another New Place".to_string()),
        "catalog entry rolled back"
    );
    let still = load_contract(&conn, c.id).unwrap().unwrap();
    assert_eq!(still.title, "Original");
    assert_eq!(
        still.requirements[1].deliveries[0].location_name, "Brand New Station",
        "previous cargo intact"
    );
}

/* ---------- set_stop_reminders ---------- */

#[test]
fn ticking_and_unticking_several_stops() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let c = add(&mut conn);
    let ids = stop_ids(&c);
    let ticked = set_stop_reminders(&mut conn, &ids[..2], true, T0 + 1).unwrap();
    assert_eq!(
        ticked
            .iter()
            .map(|s| (s.id, s.reminder_done_at))
            .collect::<Vec<_>>(),
        [(ids[0], Some(T0 + 1)), (ids[1], Some(T0 + 1))]
    );
    // Ticking again keeps the first timestamp.
    let again = set_stop_reminders(&mut conn, &ids[..1], true, T0 + 99).unwrap();
    assert_eq!(again[0].reminder_done_at, Some(T0 + 1));
    let unticked = set_stop_reminders(&mut conn, &ids[..2], false, T0 + 2).unwrap();
    assert!(unticked.iter().all(|s| s.reminder_done_at.is_none()));
}

#[test]
fn reminders_rejected_on_finished_contracts_and_unknown_stops() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let c = add(&mut conn);
    let ids = stop_ids(&c);
    match set_stop_reminders(&mut conn, &[ids[0], 999_999], true, T0) {
        Err(FinalizeError::StopNotFound(id)) => assert_eq!(id, 999_999),
        other => panic!("expected StopNotFound, got {other:?}"),
    }
    assert!(
        load_contract(&conn, c.id).unwrap().unwrap().requirements[0].pickups[0]
            .reminder_done_at
            .is_none(),
        "all or nothing"
    );
    finalize_contract(&mut conn, c.id, FinalStatus::Abandoned, T0 + 5).unwrap();
    assert!(matches!(
        set_stop_reminders(&mut conn, &ids[..1], true, T0 + 6),
        Err(FinalizeError::StopNotActive { .. })
    ));
}

#[test]
fn ticks_never_change_active_totals_or_completion() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let c = add(&mut conn);
    let totals = |conn: &Connection| {
        list_active_contracts(conn)
            .unwrap()
            .iter()
            .map(|c| c.total_scu_milli)
            .sum::<i64>()
    };
    let before = totals(&conn);
    set_stop_reminders(&mut conn, &stop_ids(&c), true, T0 + 1).unwrap();
    assert_eq!(totals(&conn), before);
    // Completion never requires ticks, and keeps them in the stored data.
    let partial = add(&mut conn);
    set_stop_reminders(&mut conn, &stop_ids(&partial)[..1], true, T0 + 2).unwrap();
    let done = finalize_contract(&mut conn, partial.id, FinalStatus::Completed, T0 + 3).unwrap();
    assert_eq!(
        done.requirements[0].pickups[0].reminder_done_at,
        Some(T0 + 2)
    );
}

/* ---------- migration 0003 ---------- */

#[test]
fn migration_0003_adds_reminders_to_a_version_2_database_with_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE_NAME);
    {
        let mut conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 2).unwrap();
        conn.execute_batch(
            "INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes) VALUES (1, '2026-11-18', 0, 60);
             INSERT INTO contracts (run_id, title, reward_auec, accepted_at, source, created_at, updated_at)
               VALUES (1, 'Old', 5, 0, 'manual', 0, 0);
             INSERT INTO cargo_requirements (contract_id, commodity_name, quantity_scu_milli, created_at)
               VALUES (1, 'Gold', 2000, 0);
             INSERT INTO cargo_stops (requirement_id, operation, location_name, sort_order) VALUES
               (1, 'pickup', 'Port Tressler', 0), (1, 'delivery', 'Area18', 0);",
        )
        .unwrap();
    }
    let db = Database::open(&path).unwrap();
    let mut conn = db.connection().unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, LATEST_SCHEMA_VERSION);
    let old = load_contract(&conn, 1).unwrap().unwrap();
    assert_eq!(old.title, "Old");
    assert!(old.requirements[0]
        .pickups
        .iter()
        .chain(&old.requirements[0].deliveries)
        .all(|s| s.reminder_done_at.is_none()));
    let stop_id = old.requirements[0].pickups[0].id;
    set_stop_reminders(&mut conn, &[stop_id], true, 7).unwrap();
    drop(conn);
    drop(db);
    let conn = open_connection(&path).unwrap();
    let done: Option<i64> = conn
        .query_row(
            "SELECT reminder_done_at FROM cargo_stops WHERE id = ?1",
            [stop_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(done, Some(7), "persists after reopening");
}
