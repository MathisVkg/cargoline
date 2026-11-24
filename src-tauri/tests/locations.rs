//! Headless tests for the location catalog (migration 0002). Temporary databases only.

use cargo_line_lib::contracts::repository::{create_contract, list_active_contracts};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::connection::open_connection;
use cargo_line_lib::database::migrations::{migrations, LATEST_SCHEMA_VERSION};
use cargo_line_lib::database::models::ContractSource;
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::locations::{
    list_locations, normalize_display, normalize_key, search_locations,
};
use rusqlite::Connection;

fn day(utc_ms: i64) -> AcceptanceTime {
    AcceptanceTime {
        utc_ms,
        local_date: "2026-11-19".into(),
        utc_offset_minutes: 60,
    }
}

fn stop(name: &str) -> NewStopInput {
    NewStopInput {
        location_name: name.into(),
        quantity_scu: None,
    }
}

fn input(pickup: &str, delivery: &str) -> NewContractInput {
    NewContractInput {
        title: "Haul".into(),
        reward_auec: "1000".into(),
        requirements: vec![NewRequirementInput {
            commodity_name: "Aluminum".into(),
            quantity_scu: "13".into(),
            max_box_size_scu: None,
            pickups: vec![stop(pickup)],
            deliveries: vec![stop(delivery)],
        }],
    }
}

fn save(conn: &mut Connection, input: &NewContractInput) -> rusqlite::Result<ContractDetail> {
    create_contract(
        conn,
        &validate_new_contract(input).unwrap(),
        &day(1_000),
        ContractSource::Manual,
    )
}

fn names(conn: &Connection) -> Vec<String> {
    list_locations(conn)
        .unwrap()
        .into_iter()
        .map(|l| l.name)
        .collect()
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn normalization_is_case_and_whitespace_insensitive() {
    assert_eq!(normalize_display("  Port   Tressler \t"), "Port Tressler");
    assert_eq!(normalize_key("  PORT  tressler"), "port tressler");
    assert_eq!(
        normalize_key("Port Tressler"),
        normalize_key("port\u{a0}tressler")
    );
}

#[test]
fn migration_seeds_catalog_from_existing_stops_without_changing_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE_NAME);
    {
        // A database from before migration 0002, with stored stops.
        let mut conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 1).unwrap();
        conn.execute_batch(
            "INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes) VALUES (1, '2026-11-18', 0, 60);
             INSERT INTO contracts (run_id, title, reward_auec, accepted_at, source, created_at, updated_at)
               VALUES (1, 'Old', 5, 0, 'manual', 0, 0);
             INSERT INTO cargo_requirements (contract_id, commodity_name, quantity_scu_milli, created_at)
               VALUES (1, 'Gold', 2000, 0);
             INSERT INTO cargo_stops (requirement_id, operation, location_name, sort_order) VALUES
               (1, 'pickup', 'Port Tressler', 0),
               (1, 'pickup', 'port  TRESSLER', 1),
               (1, 'delivery', 'Area18', 0);",
        )
        .unwrap();
    }

    let db = Database::open(&path).unwrap();
    let conn = db.connection().unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, LATEST_SCHEMA_VERSION);
    // Oldest spelling wins; case/space variants collapse into one entry.
    assert_eq!(names(&conn), ["Area18", "Port Tressler"]);
    // Stored stops are untouched.
    let stored: Vec<String> = conn
        .prepare("SELECT location_name FROM cargo_stops ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(stored, ["Port Tressler", "port  TRESSLER", "Area18"]);
    assert_eq!(list_active_contracts(&conn).unwrap().len(), 1);
}

#[test]
fn new_locations_are_added_on_successful_save_and_reused_case_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    let mut conn = db.connection().unwrap();
    assert!(names(&conn).is_empty());

    save(&mut conn, &input("Port Tressler", "New Babbage")).unwrap();
    assert_eq!(names(&conn), ["New Babbage", "Port Tressler"]);

    // Different case and spacing reuse the catalog spelling for the stored stop.
    let second = save(
        &mut conn,
        &input("  port   tressler ", "Sakura Sun Goldenrod Workcenter"),
    )
    .unwrap();
    assert_eq!(
        second.requirements[0].pickups[0].location_name,
        "Port Tressler"
    );
    assert_eq!(count(&conn, "locations"), 3);
    assert_eq!(
        names(&conn),
        [
            "New Babbage",
            "Port Tressler",
            "Sakura Sun Goldenrod Workcenter"
        ]
    );
}

#[test]
fn failed_save_adds_no_catalog_entries() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    let mut conn = db.connection().unwrap();
    save(&mut conn, &input("Port Tressler", "Area18")).unwrap();
    conn.execute_batch(
        "CREATE TEMP TRIGGER fail BEFORE INSERT ON cargo_stops WHEN NEW.location_name = 'Doomed Delivery'
         BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;",
    )
    .unwrap();
    assert!(save(&mut conn, &input("Brand New Pickup", "Doomed Delivery")).is_err());
    assert_eq!(
        names(&conn),
        ["Area18", "Port Tressler"],
        "no unused catalog entries"
    );
    assert_eq!(count(&conn, "contracts"), 1);
}

#[test]
fn whitespace_variants_count_as_duplicates_in_one_operation() {
    let mut bad = input("Port Tressler", "Area18");
    bad.requirements[0].pickups.push(stop("port   tressler"));
    let issues = validate_new_contract(&bad).unwrap_err();
    assert_eq!(issues[0].path, "requirements.0.pickups.1.locationName");
}

#[test]
fn catalog_search_is_case_insensitive_prefix_first_and_escapes_wildcards() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE_NAME);
    let db = Database::open(&path).unwrap();
    let mut conn = db.connection().unwrap();
    save(&mut conn, &input("Port Tressler", "Everus Harbor")).unwrap();
    save(&mut conn, &input("Seraphim Station", "100%_Depot")).unwrap();

    let found = |q: &str| -> Vec<String> {
        search_locations(&conn, q, 10)
            .unwrap()
            .into_iter()
            .map(|l| l.name)
            .collect()
    };
    assert_eq!(found("TRESS"), ["Port Tressler"]);
    assert_eq!(
        found("s"),
        ["Seraphim Station", "Everus Harbor", "Port Tressler"]
    );
    assert_eq!(found("%"), ["100%_Depot"]);
    assert_eq!(found("_"), ["100%_Depot"]);
    assert!(found("zzz").is_empty());
    drop(conn);
    drop(db);
    // Persists across reopening.
    let conn = open_connection(&path).unwrap();
    assert_eq!(names(&conn).len(), 4);
}

#[test]
fn schema_has_no_reference_catalog_migration() {
    assert_eq!(
        LATEST_SCHEMA_VERSION, 6,
        "0003 = stop reminders, 0004 = capture details, 0005 = capture processing status, 0006 = run sequence"
    );
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    let conn = db.connection().unwrap();
    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('locations') ORDER BY cid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        columns,
        ["id", "display_name", "normalized_name", "created_at"]
    );
    let commodities: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'commodities'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(commodities, 0, "no commodities table");
    let metadata_columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('catalog_metadata') ORDER BY cid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        metadata_columns,
        [
            "id",
            "catalog_version",
            "published_on",
            "source_summary",
            "imported_at"
        ]
    );
}
