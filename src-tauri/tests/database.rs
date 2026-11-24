//! Headless database tests. Every test uses its own temporary directory;
//! the real user database is never touched.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use cargo_line_lib::database::connection::{open_connection, APPLICATION_ID};
use cargo_line_lib::database::migrations::LATEST_SCHEMA_VERSION;
use cargo_line_lib::database::models::*;
use cargo_line_lib::database::{Database, DatabaseError, DATABASE_FILE_NAME};
use cargo_line_lib::settings::{
    self, AppSettings, CaptureSource, CompleteFromGameMode, ContractGrouping, SettingsError,
};
use rusqlite::{params, Connection, ErrorCode};
use tempfile::TempDir;

const T0: i64 = 1_780_000_000_000;

fn temp_db() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("data").join(DATABASE_FILE_NAME);
    (dir, path)
}

fn open(path: &Path) -> Database {
    Database::open(path).expect("open database")
}

fn insert_run(conn: &Connection, number: i64, date: &str) -> i64 {
    conn.execute(
        "INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes) VALUES (?1, ?2, ?3, 60)",
        params![number, date, T0],
    )
    .expect("insert run");
    conn.last_insert_rowid()
}

fn insert_contract(conn: &Connection, run_id: i64, title: &str) -> i64 {
    conn.execute(
        "INSERT INTO contracts (run_id, title, reward_auec, accepted_at, source, created_at, updated_at)
         VALUES (?1, ?2, 48250, ?3, ?4, ?3, ?3)",
        params![run_id, title, T0, ContractSource::Manual],
    )
    .expect("insert contract");
    conn.last_insert_rowid()
}

fn insert_requirement(conn: &Connection, contract_id: i64, commodity: &str, scu: i64) -> i64 {
    conn.execute(
        "INSERT INTO cargo_requirements (contract_id, commodity_name, quantity_scu_milli, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![contract_id, commodity, scu * MILLI_SCU_PER_SCU, T0],
    )
    .expect("insert requirement");
    conn.last_insert_rowid()
}

fn insert_stop(
    conn: &Connection,
    requirement_id: i64,
    op: CargoOperation,
    location: &str,
    scu: Option<i64>,
    order: i64,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO cargo_stops (requirement_id, operation, location_name, quantity_scu_milli, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![requirement_id, op, location, scu.map(|s| s * MILLI_SCU_PER_SCU), order],
    )
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .expect("count")
}

fn assert_constraint(result: rusqlite::Result<usize>) {
    match result {
        Err(rusqlite::Error::SqliteFailure(e, _)) => {
            assert_eq!(
                e.code,
                ErrorCode::ConstraintViolation,
                "unexpected error: {e:?}"
            )
        }
        other => panic!("expected a constraint violation, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Initialization & migrations
// ---------------------------------------------------------------------------

#[test]
fn fresh_database_is_created_and_configured() {
    let (_dir, path) = temp_db();
    assert!(!path.exists());
    let db = open(&path);
    assert!(
        path.exists(),
        "database file and parent directory are created"
    );

    let conn = db.connection().unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, LATEST_SCHEMA_VERSION);
    let fk: bool = conn
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .unwrap();
    assert!(fk);
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    let timeout: i64 = conn
        .pragma_query_value(None, "busy_timeout", |r| r.get(0))
        .unwrap();
    assert_eq!(timeout, 5_000);

    let mut tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    tables.sort();
    assert_eq!(
        tables,
        [
            "app_settings",
            "cargo_box_groups",
            "cargo_requirements",
            "cargo_stops",
            "catalog_metadata",
            "contract_captures",
            "contracts",
            "locations",
            "run_sequence",
            "runs",
            "ship_favorites",
            "ships",
        ]
    );
    let fk_violations = conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query([])
        .unwrap()
        .next()
        .unwrap()
        .is_none();
    assert!(fk_violations);
}

#[test]
fn migrations_definition_is_valid() {
    cargo_line_lib::database::migrations::migrations()
        .validate()
        .expect("valid migrations");
}

#[test]
fn reopening_preserves_data_and_reapplying_migrations_is_a_no_op() {
    let (_dir, path) = temp_db();
    {
        let db = open(&path);
        let conn = db.connection().unwrap();
        let run = insert_run(&conn, 1, "2026-11-19");
        let contract = insert_contract(&conn, run, "Covalex delivery");
        insert_requirement(&conn, contract, "Agricium", 12);
    }
    for _ in 0..3 {
        let db = open(&path);
        let conn = db.connection().unwrap();
        assert_eq!(count(&conn, "runs"), 1);
        assert_eq!(count(&conn, "contracts"), 1);
        assert_eq!(count(&conn, "cargo_requirements"), 1);
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
        let app_id: i32 = conn
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .unwrap();
        assert_eq!(app_id, APPLICATION_ID);
    }
}

#[test]
fn database_from_a_newer_version_is_rejected_and_left_unchanged() {
    let (_dir, path) = temp_db();
    {
        let db = open(&path);
        let conn = db.connection().unwrap();
        insert_run(&conn, 1, "2026-11-19");
        conn.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION + 5)
            .unwrap();
    }
    match Database::open(&path) {
        Err(DatabaseError::SchemaTooNew { found, .. }) => {
            assert_eq!(found, LATEST_SCHEMA_VERSION + 5)
        }
        Err(other) => panic!("unexpected error: {other}"),
        Ok(_) => panic!("a newer database must not be opened"),
    }
    let conn = open_connection(&path).unwrap();
    assert_eq!(count(&conn, "runs"), 1, "existing data must be untouched");
}

#[test]
fn foreign_sqlite_file_is_rejected_and_left_byte_for_byte_unchanged() {
    let (_dir, path) = temp_db();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        // e.g. a database from an earlier prototype with the same file name.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA user_version = 1; CREATE TABLE runs (legacy TEXT); INSERT INTO runs VALUES ('keep');",
        )
        .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        Database::open(&path),
        Err(DatabaseError::NotCargoLineDatabase { .. })
    ));
    assert!(matches!(
        open_connection(&path),
        Err(DatabaseError::NotCargoLineDatabase { .. })
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before, "file not modified");
    assert!(
        !path.with_extension("sqlite3-wal").exists(),
        "WAL not enabled"
    );
}

#[test]
fn failed_migration_is_rolled_back() {
    let (_dir, path) = temp_db();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // A conflicting pre-existing object makes the migration batch fail midway.
    let mut conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE ships (legacy TEXT); INSERT INTO ships VALUES ('keep');")
        .unwrap();
    assert!(cargo_line_lib::database::migrations::migrations()
        .to_latest(&mut conn)
        .is_err());

    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 0, "schema version unchanged");
    let app_id: i32 = conn
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .unwrap();
    assert_eq!(app_id, 0, "application id rolled back with the schema");
    assert_eq!(count(&conn, "ships"), 1, "pre-existing data kept");
    let runs_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'runs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(runs_exists, 0, "partially created tables were rolled back");
}

// ---------------------------------------------------------------------------
// Integrity
// ---------------------------------------------------------------------------

#[test]
fn foreign_keys_are_enforced() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let conn = db.connection().unwrap();
    assert_constraint(conn.execute(
        "INSERT INTO contracts (run_id, title, reward_auec, accepted_at, source, created_at, updated_at)
         VALUES (999, 'Orphan', 0, ?1, 'manual', ?1, ?1)",
        [T0],
    ));
    assert_constraint(conn.execute(
        "INSERT INTO cargo_requirements (contract_id, commodity_name, quantity_scu_milli, created_at)
         VALUES (999, 'Gold', 1000, 0)",
        [],
    ));
    assert_constraint(conn.execute(
        "INSERT INTO ship_favorites (ship_id, created_at) VALUES ('none', 0)",
        [],
    ));
}

#[test]
fn runs_enforce_unique_dates_and_increasing_numbers() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let conn = db.connection().unwrap();
    let run = insert_run(&conn, 1, "2026-11-19");
    insert_run(&conn, 2, "2026-11-20");

    let add = |n: i64, d: &str, off: i64| {
        conn.execute(
            "INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes) VALUES (?1, ?2, 0, ?3)",
            params![n, d, off],
        )
    };
    assert_constraint(add(3, "2026-11-19", 0)); // duplicate date
    assert_constraint(add(2, "2026-11-21", 0)); // duplicate number
    assert_constraint(add(1, "2026-11-22", 0)); // not increasing
    assert_constraint(add(3, "2026-02-30", 0)); // invalid date
    assert_constraint(add(3, "19/11/2026", 0)); // wrong format
    assert_constraint(add(3, "2026-11-23", 900)); // offset out of range
    assert_constraint(conn.execute(
        "UPDATE runs SET run_date = '2026-12-01' WHERE id = ?1",
        [run],
    ));
}

#[test]
fn contracts_reject_invalid_values() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let conn = db.connection().unwrap();
    let run = insert_run(&conn, 1, "2026-11-19");
    let insert = |title: &str, reward: i64, status: &str, finished: Option<i64>, source: &str| {
        conn.execute(
            "INSERT INTO contracts (run_id, title, reward_auec, status, accepted_at, finished_at, source, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?5, ?5)",
            params![run, title, reward, status, T0, finished, source],
        )
    };
    assert_constraint(insert("  ", 10, "active", None, "manual"));
    assert_constraint(insert("Negative", -1, "active", None, "manual"));
    assert_constraint(insert("Bad status", 10, "paused", None, "manual"));
    assert_constraint(insert("Bad source", 10, "active", None, "clipboard"));
    assert_constraint(insert(
        "Active with finish",
        10,
        "active",
        Some(T0),
        "manual",
    ));
    assert_constraint(insert(
        "Completed without finish",
        10,
        "completed",
        None,
        "manual",
    ));
    assert_constraint(insert(
        "Finished before accepted",
        10,
        "completed",
        Some(T0 - 1),
        "manual",
    ));

    for status in ContractStatus::ALL {
        let finished = (*status != ContractStatus::Active).then_some(T0);
        insert("Ok", 0, status.as_str(), finished, "ocr").expect("all enum values accepted");
    }
    for source in ContractSource::ALL {
        insert("Ok", 0, "active", None, source.as_str()).expect("all enum values accepted");
    }
}

#[test]
fn status_changes_keep_the_original_run_and_record() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let conn = db.connection().unwrap();
    let day1 = insert_run(&conn, 1, "2026-11-19");
    let day2 = insert_run(&conn, 2, "2026-11-20");
    let contract = insert_contract(&conn, day1, "Accepted before midnight");
    insert_requirement(&conn, contract, "Titanium", 8);

    // Completed the next day: record stays, run unchanged.
    conn.execute(
        "UPDATE contracts SET status = 'completed', finished_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![T0 + 86_400_000, contract],
    )
    .unwrap();
    let (run_id, status): (i64, ContractStatus) = conn
        .query_row(
            "SELECT run_id, status FROM contracts WHERE id = ?1",
            [contract],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((run_id, status), (day1, ContractStatus::Completed));
    assert_eq!(count(&conn, "cargo_requirements"), 1);

    assert_constraint(conn.execute(
        "UPDATE contracts SET run_id = ?1 WHERE id = ?2",
        params![day2, contract],
    ));
}

#[test]
fn deletion_policy_protects_active_records_and_cascades_archived_ones() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let conn = db.connection().unwrap();
    let run = insert_run(&conn, 1, "2026-11-19");
    let contract = insert_contract(&conn, run, "Protected");
    let req = insert_requirement(&conn, contract, "Laranite", 4);
    insert_stop(&conn, req, CargoOperation::Pickup, "Port Olisar", None, 0).unwrap();
    conn.execute(
        "INSERT INTO cargo_box_groups (requirement_id, box_size_scu_milli, box_count, source_quality) VALUES (?1, 1000, 4, 'exact')",
        [req],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO contract_captures (contract_id, file_path, captured_at, expires_at, capture_method)
         VALUES (?1, 'captures/a.png', ?2, ?3, 'manual')",
        params![contract, T0, T0 + CAPTURE_RETENTION_MS],
    )
    .unwrap();

    // Active contracts cannot be deleted; runs with contracts cannot be deleted.
    assert_constraint(conn.execute("DELETE FROM contracts WHERE id = ?1", [contract]));
    assert_constraint(conn.execute("DELETE FROM runs WHERE id = ?1", [run]));

    conn.execute(
        "UPDATE contracts SET status = 'abandoned', finished_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![T0 + 1, contract],
    )
    .unwrap();
    conn.execute("DELETE FROM contracts WHERE id = ?1", [contract])
        .unwrap();

    assert_eq!(count(&conn, "cargo_requirements"), 0);
    assert_eq!(count(&conn, "cargo_stops"), 0);
    assert_eq!(count(&conn, "cargo_box_groups"), 0);
    // Capture metadata survives (unlinked) so its file can still be cleaned up.
    let linked: Option<i64> = conn
        .query_row("SELECT contract_id FROM contract_captures", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(linked, None);
    conn.execute("DELETE FROM runs WHERE id = ?1", [run])
        .unwrap();
}

#[test]
fn multi_pickup_and_multi_destination_cargo_is_represented_without_double_counting() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let conn = db.connection().unwrap();
    let run = insert_run(&conn, 1, "2026-11-19");
    let contract = insert_contract(&conn, run, "Multi-stop haul");
    let gold = insert_requirement(&conn, contract, "Gold", 30);
    let quartz = insert_requirement(&conn, contract, "Quartz", 10);

    // Gold: two pickups (one quantity unknown), three deliveries.
    insert_stop(&conn, gold, CargoOperation::Pickup, "HUR-L1", Some(20), 0).unwrap();
    insert_stop(&conn, gold, CargoOperation::Pickup, "HUR-L2", None, 1).unwrap();
    insert_stop(
        &conn,
        gold,
        CargoOperation::Delivery,
        "Lorville",
        Some(10),
        0,
    )
    .unwrap();
    insert_stop(&conn, gold, CargoOperation::Delivery, "Area18", Some(10), 1).unwrap();
    insert_stop(
        &conn,
        gold,
        CargoOperation::Delivery,
        "New Babbage",
        Some(10),
        2,
    )
    .unwrap();
    // Quartz: one pickup, one delivery, quantities unknown.
    insert_stop(&conn, quartz, CargoOperation::Pickup, "HUR-L1", None, 0).unwrap();
    insert_stop(&conn, quartz, CargoOperation::Delivery, "Lorville", None, 0).unwrap();

    // Total SCU comes from requirements only.
    let total: i64 = conn
        .query_row(
            "SELECT SUM(quantity_scu_milli) FROM cargo_requirements WHERE contract_id = ?1",
            [contract],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(total, 40 * MILLI_SCU_PER_SCU);

    let pickups: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT location_name) FROM cargo_stops WHERE operation = 'pickup'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let deliveries: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT location_name) FROM cargo_stops WHERE operation = 'delivery'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((pickups, deliveries), (2, 3));

    // Allocations may never exceed the requirement (no double counting).
    assert_constraint(insert_stop(
        &conn,
        gold,
        CargoOperation::Pickup,
        "HUR-L3",
        Some(11),
        2,
    ));
    assert_constraint(insert_stop(
        &conn,
        gold,
        CargoOperation::Delivery,
        "Orison",
        Some(1),
        3,
    ));
    assert_constraint(conn.execute(
        "UPDATE cargo_stops SET quantity_scu_milli = 31000 WHERE requirement_id = ?1 AND location_name = 'HUR-L1'",
        [gold],
    ));
    assert_constraint(conn.execute(
        "UPDATE cargo_requirements SET quantity_scu_milli = 20000 WHERE id = ?1",
        [gold],
    ));
    // Duplicate sort position and invalid values are rejected.
    assert_constraint(insert_stop(
        &conn,
        quartz,
        CargoOperation::Pickup,
        "Elsewhere",
        None,
        0,
    ));
    assert_constraint(conn.execute(
        "INSERT INTO cargo_stops (requirement_id, operation, location_name, sort_order) VALUES (?1, 'transfer', 'X', 5)",
        [quartz],
    ));
    assert_constraint(insert_stop(
        &conn,
        quartz,
        CargoOperation::Pickup,
        "Zero",
        Some(0),
        5,
    ));
}

#[test]
fn cargo_ships_and_captures_reject_invalid_values() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let conn = db.connection().unwrap();
    let run = insert_run(&conn, 1, "2026-11-19");
    let contract = insert_contract(&conn, run, "Values");
    let req = insert_requirement(&conn, contract, "Iron", 2);

    assert_constraint(conn.execute(
        "INSERT INTO cargo_requirements (contract_id, commodity_name, quantity_scu_milli, created_at) VALUES (?1, 'Iron', 0, 0)",
        [contract],
    ));
    assert_constraint(conn.execute(
        "INSERT INTO cargo_requirements (contract_id, commodity_name, quantity_scu_milli, created_at) VALUES (?1, '', 1000, 0)",
        [contract],
    ));
    let box_group = |size: i64, n: i64, q: &str| {
        conn.execute(
            "INSERT INTO cargo_box_groups (requirement_id, box_size_scu_milli, box_count, source_quality) VALUES (?1, ?2, ?3, ?4)",
            params![req, size, n, q],
        )
    };
    assert_constraint(box_group(0, 1, "exact"));
    assert_constraint(box_group(1000, 0, "exact"));
    assert_constraint(box_group(1000, 1, "guessed"));
    box_group(2000, 1, BoxSourceQuality::Estimated.as_str()).unwrap();

    let ship = |id: &str, scu: i64| {
        conn.execute(
            "INSERT INTO ships (id, manufacturer, name, cargo_capacity_scu_milli) VALUES (?1, 'RSI', 'Test', ?2)",
            params![id, scu],
        )
    };
    assert_constraint(ship("zero", 0));
    assert_constraint(ship("", 1000));
    ship("rsi-test", 1000).unwrap();
    assert_constraint(ship("rsi-test", 1000));
    conn.execute(
        "INSERT INTO ship_favorites (ship_id, created_at) VALUES ('rsi-test', 0)",
        [],
    )
    .unwrap();
    assert_constraint(conn.execute(
        "INSERT INTO ship_favorites (ship_id, created_at) VALUES ('rsi-test', 1)",
        [],
    ));

    let capture = |path: &str, expires: i64, method: &str| {
        conn.execute(
            "INSERT INTO contract_captures (file_path, captured_at, expires_at, capture_method) VALUES (?1, ?2, ?3, ?4)",
            params![path, T0, expires, method],
        )
    };
    assert_constraint(capture("a.png", T0 + CAPTURE_RETENTION_MS + 1, "manual")); // > 7 days
    assert_constraint(capture("b.png", T0, "manual")); // expires immediately
    assert_constraint(capture("c.png", T0 + 1, "screen_recorder"));
    for method in CaptureMethod::ALL {
        capture(&format!("{}.png", method.as_str()), T0 + 1, method.as_str()).unwrap();
    }
    assert_constraint(capture("manual.png", T0 + 1, "manual")); // duplicate path

    assert_constraint(conn.execute(
        "INSERT INTO catalog_metadata (id, catalog_version, imported_at) VALUES (2, '1.0', 0)",
        [],
    ));
    conn.execute(
        "INSERT INTO catalog_metadata (id, catalog_version, imported_at) VALUES (1, '1.0', 0)",
        [],
    )
    .unwrap();
}

#[test]
fn failed_multi_record_transaction_rolls_back_completely() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let mut conn = db.connection().unwrap();
    let run = insert_run(&conn, 1, "2026-11-19");

    let result: rusqlite::Result<()> = (|| {
        let tx = conn.transaction()?;
        let contract = insert_contract(&tx, run, "Half-written");
        let req = insert_requirement(&tx, contract, "Gold", 5);
        insert_stop(&tx, req, CargoOperation::Pickup, "Valid stop", Some(5), 0)?;
        insert_stop(
            &tx,
            req,
            CargoOperation::Pickup,
            "Over-allocated",
            Some(1),
            1,
        )?; // fails
        tx.commit()
    })();
    assert!(result.is_err());
    assert_eq!(count(&conn, "contracts"), 0);
    assert_eq!(count(&conn, "cargo_requirements"), 0);
    assert_eq!(count(&conn, "cargo_stops"), 0);
}

// ---------------------------------------------------------------------------
// Concurrency
// ---------------------------------------------------------------------------

#[test]
fn concurrent_connections_wait_for_locks_and_readers_see_committed_data() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    insert_run(&db.connection().unwrap(), 1, "2026-11-19");

    let barrier = Arc::new(Barrier::new(2));
    let writer = {
        let path = path.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            let mut conn = open_connection(&path).unwrap();
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap();
            insert_run(&tx, 2, "2026-11-20");
            barrier.wait(); // hold the write lock while the other side acts
            thread::sleep(Duration::from_millis(300));
            tx.commit().unwrap();
        })
    };

    barrier.wait();
    let reader = open_connection(&path).unwrap();
    // WAL: the reader is not blocked and sees only committed data.
    assert_eq!(count(&reader, "runs"), 1);

    // A second writer waits on the busy timeout instead of failing immediately.
    let conn = db.connection().unwrap();
    insert_run(&conn, 3, "2026-11-21");
    writer.join().unwrap();
    assert_eq!(count(&reader, "runs"), 3);
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[test]
fn settings_defaults_are_safe() {
    let s = AppSettings::default();
    s.validate().unwrap();
    assert!(!s.game_log_monitoring_enabled);
    assert!(!s.auto_capture_on_accept);
    assert!(!s.complete_from_game);
    assert_eq!(s.complete_from_game_mode, CompleteFromGameMode::Ask);
    assert!(!s.automatic_update_checks);
    assert_eq!(s.game_log_path, None);
    assert_eq!(s.selected_ship_id, None);
    assert_eq!(s.capture_source, CaptureSource::StarCitizenWindow);
    assert_eq!(s.contract_grouping, ContractGrouping::ByLocation);
    assert!(s.capture_delay_ms <= 5_000 && s.capture_delay_ms % 100 == 0);
}

#[test]
fn settings_persist_across_restarts_and_restore_defaults() {
    let (_dir, path) = temp_db();
    let expected = AppSettings {
        game_log_monitoring_enabled: true,
        game_log_path: Some(r"C:\Games\StarCitizen\LIVE\Game.log".into()),
        capture_source: CaptureSource::FullDisplay,
        capture_delay_ms: 2_300,
        capture_on_add_contract: false,
        capture_shortcut: Some("Ctrl+Shift+C".into()),
        auto_capture_on_accept: true,
        complete_from_game: true,
        complete_from_game_mode: CompleteFromGameMode::Automatic,
        selected_ship_id: Some("drak-caterpillar".into()),
        automatic_update_checks: true,
        contract_grouping: ContractGrouping::ByContract,
    };
    {
        let db = open(&path);
        let mut conn = db.connection().unwrap();
        assert_eq!(settings::load(&conn).unwrap(), AppSettings::default());
        settings::save(&mut conn, &expected).unwrap();
    }
    {
        let db = open(&path);
        let mut conn = db.connection().unwrap();
        assert_eq!(settings::load(&conn).unwrap(), expected);

        let updated = settings::update(&mut conn, |s| s.selected_ship_id = None).unwrap();
        assert_eq!(updated.selected_ship_id, None);
        assert_eq!(settings::load(&conn).unwrap(), updated);

        assert_eq!(
            settings::restore_defaults(&mut conn).unwrap(),
            AppSettings::default()
        );
    }
    let db = open(&path);
    assert_eq!(
        settings::load(&db.connection().unwrap()).unwrap(),
        AppSettings::default()
    );
}

#[test]
fn invalid_settings_are_rejected_and_not_written() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let mut conn = db.connection().unwrap();

    let cases: Vec<(&str, AppSettings)> = vec![
        (
            "capture_delay_ms",
            AppSettings {
                capture_delay_ms: 5_100,
                ..Default::default()
            },
        ),
        (
            "capture_delay_ms",
            AppSettings {
                capture_delay_ms: 150,
                ..Default::default()
            },
        ),
        (
            "game_log_path",
            AppSettings {
                game_log_path: Some("   ".into()),
                ..Default::default()
            },
        ),
        (
            "capture_shortcut",
            AppSettings {
                capture_shortcut: Some("Ctrl+\u{0}".into()),
                ..Default::default()
            },
        ),
        (
            "capture_shortcut",
            AppSettings {
                capture_shortcut: Some("K".repeat(65)),
                ..Default::default()
            },
        ),
        (
            "selected_ship_id",
            AppSettings {
                selected_ship_id: Some(" padded ".into()),
                ..Default::default()
            },
        ),
        (
            "auto_capture_on_accept",
            AppSettings {
                auto_capture_on_accept: true,
                ..Default::default()
            },
        ),
    ];
    for (field, candidate) in cases {
        match settings::save(&mut conn, &candidate) {
            Err(SettingsError::Invalid(e)) => assert_eq!(e.field, field),
            other => panic!("expected invalid `{field}`, got {other:?}"),
        }
    }
    assert!(matches!(
        settings::update(&mut conn, |s| s.capture_delay_ms = 9_999),
        Err(SettingsError::Invalid(_))
    ));
    assert_eq!(count(&conn, "app_settings"), 0, "nothing written");
}

#[test]
fn corrupted_stored_settings_are_reported_not_reset() {
    let (_dir, path) = temp_db();
    let db = open(&path);
    let conn = db.connection().unwrap();
    // Unknown keys (e.g. from another version) are ignored.
    conn.execute(
        "INSERT INTO app_settings VALUES ('future_option', 'true', 0)",
        [],
    )
    .unwrap();
    settings::load(&conn).unwrap();

    conn.execute(
        "INSERT INTO app_settings VALUES ('capture_delay_ms', '\"soon\"', 0)",
        [],
    )
    .unwrap();
    assert!(matches!(
        settings::load(&conn),
        Err(SettingsError::InvalidStored(_))
    ));
    conn.execute(
        "UPDATE app_settings SET value = '7000' WHERE key = 'capture_delay_ms'",
        [],
    )
    .unwrap();
    assert!(matches!(
        settings::load(&conn),
        Err(SettingsError::InvalidStored(_))
    ));
    assert_eq!(
        count(&conn, "app_settings"),
        2,
        "stored rows left untouched"
    );

    assert_constraint(conn.execute("INSERT INTO app_settings VALUES ('x', 'not json', 0)", []));
}
