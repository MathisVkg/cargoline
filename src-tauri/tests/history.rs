//! Task 12 tests: History statistics and grouping, Restore to active, Clear History, run
//! numbers never reused (migration 0006), and a ~500-contract timing. Temporary databases only.

use std::path::Path;
use std::time::Instant;

use cargo_line_lib::capture::storage;
use cargo_line_lib::contracts::edit::set_stop_reminders;
use cargo_line_lib::contracts::finalize::{finalize_contracts, FinalStatus};
use cargo_line_lib::contracts::reactivate::{
    reactivate_contracts, restore_contracts, ReactivateError,
};
use cargo_line_lib::contracts::repository::{create_contract_linked, SaveLinks};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::migrations::{migrations, LATEST_SCHEMA_VERSION};
use cargo_line_lib::database::models::{ContractSource, ContractStatus};
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::history::{self, build_history};
use cargo_line_lib::settings;
use cargo_line_lib::ships;
use rusqlite::Connection;

const DAY: i64 = 86_400_000;
const T0: i64 = 1_795_176_000_000; // 2026-11-20T12:00:00Z
const M1: &str = "6a1b2c3d-1111-4aaa-8bbb-0123456789ab";

fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    (dir, db)
}

/// `day` 0 = 2026-11-20.
fn date(day: i64) -> String {
    let base = chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
    (base + chrono::Duration::days(day))
        .format("%Y-%m-%d")
        .to_string()
}

fn input(title: &str, scu: &str, reward: &str) -> NewContractInput {
    NewContractInput {
        title: title.into(),
        reward_auec: reward.into(),
        requirements: vec![NewRequirementInput {
            commodity_name: "Aluminum".into(),
            quantity_scu: scu.into(),
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

fn add(
    conn: &mut Connection,
    day: i64,
    title: &str,
    scu: &str,
    reward: &str,
    links: &SaveLinks,
) -> ContractDetail {
    let valid = validate_new_contract(&input(title, scu, reward)).unwrap();
    let at = AcceptanceTime {
        utc_ms: T0 + day * DAY,
        local_date: date(day),
        utc_offset_minutes: 60,
    };
    create_contract_linked(conn, &valid, &at, ContractSource::Manual, links).unwrap()
}

fn simple(conn: &mut Connection, day: i64, title: &str, scu: &str, reward: &str) -> i64 {
    add(conn, day, title, scu, reward, &SaveLinks::default()).id
}

fn finish(conn: &mut Connection, ids: &[i64], status: FinalStatus, at: i64) {
    finalize_contracts(conn, ids, status, at).unwrap();
}

fn status(conn: &Connection, id: i64) -> String {
    conn.query_row("SELECT status FROM contracts WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .unwrap()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/* ------------------------------------------------------------ stats/runs */

#[test]
fn empty_history() {
    let (_dir, db) = db();
    let conn = db.connection().unwrap();
    let h = history::get_history(&conn).unwrap();
    assert_eq!(h.stats, history::HistoryStats::default());
    assert!(h.runs.is_empty());
}

#[test]
fn stats_and_runs() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    // Run 1 (day 0): two completed (one fractional), one abandoned, one still active.
    let a = simple(&mut conn, 0, "A", "12.5", "10000");
    let b = simple(&mut conn, 0, "B", "100", "20000");
    let c = simple(&mut conn, 0, "C", "50", "99999");
    let active = simple(&mut conn, 0, "Active", "7", "1");
    // Run 2 (day 1): finished two days later, it stays in its own run.
    let d = simple(&mut conn, 1, "D", "0.25", "500");
    // Run 3 (day 2): only active contracts: hidden.
    simple(&mut conn, 2, "E", "1", "1");

    finish(&mut conn, &[a], FinalStatus::Completed, T0 + 1_000);
    finish(&mut conn, &[b], FinalStatus::Completed, T0 + 3_000);
    finish(&mut conn, &[c], FinalStatus::Abandoned, T0 + 2_000);
    finish(&mut conn, &[d], FinalStatus::Completed, T0 + 3 * DAY);

    let h = history::get_history(&conn).unwrap();
    assert_eq!(h.stats.completed_contracts, 3);
    assert_eq!(
        h.stats.completed_scu_milli,
        12_500 + 100_000 + 250,
        "milli-SCU, abandoned excluded"
    );
    assert_eq!(h.stats.estimated_auec, 10_000 + 20_000 + 500);

    assert_eq!(
        h.runs.iter().map(|r| r.run_number).collect::<Vec<_>>(),
        [2, 1],
        "newest first, active-only run hidden"
    );
    let r2 = &h.runs[0];
    assert_eq!(
        (r2.run_date.as_str(), r2.completed, r2.abandoned),
        (date(1).as_str(), 1, 0)
    );
    assert_eq!(
        r2.contracts[0].id, d,
        "finished on a later date, still in its original run"
    );
    let r1 = &h.runs[1];
    assert_eq!(
        (
            r1.completed,
            r1.abandoned,
            r1.completed_scu_milli,
            r1.earned_auec
        ),
        (2, 1, 112_500, 30_000)
    );
    assert_eq!(
        r1.contracts.iter().map(|c| c.id).collect::<Vec<_>>(),
        [b, c, a],
        "newest finish first"
    );
    assert!(
        !r1.contracts.iter().any(|c| c.id == active),
        "active contracts never appear"
    );
    assert_eq!(
        r1.contracts[0].requirements[0].pickups.len(),
        1,
        "cargo for the read-only view"
    );
    assert!(r1.contracts.iter().all(|c| c.finished_at.is_some()));

    // The pure grouping agrees with the query.
    assert_eq!(
        build_history(r1.contracts.clone())
            .stats
            .completed_contracts,
        2
    );
}

/* ---------------------------------------------------------------- restore */

#[test]
fn restore_keeps_everything_else() {
    let (dir, db) = db();
    let mut conn = db.connection().unwrap();
    let captures = dir.path().join("captures");
    let capture = storage::store(&conn, &captures, &frame(), T0).unwrap();
    let links = SaveLinks {
        capture_id: Some(capture.id),
        game_mission_id: Some(M1.into()),
        auto_capture: false,
    };
    let a = add(&mut conn, 0, "A", "10", "100", &links);
    let b = simple(&mut conn, 0, "B", "10", "100");
    let pickup = a.requirements[0].pickups[0].id;
    set_stop_reminders(&mut conn, &[pickup], true, T0 + 10).unwrap();
    finish(&mut conn, &[a.id], FinalStatus::Completed, T0 + 1_000);
    finish(&mut conn, &[b], FinalStatus::Abandoned, T0 + 2_000);
    let before = fingerprint(&conn, a.id);

    // Far beyond Undo's 5 minutes: Undo refuses, Restore accepts (completed and abandoned).
    let later = T0 + 30 * DAY;
    assert_eq!(
        reactivate_contracts(&mut conn, &[a.id], later),
        Err(ReactivateError::TooOld(a.id))
    );
    let restored = restore_contracts(&mut conn, &[a.id, b], later).unwrap();
    assert!(restored
        .iter()
        .all(|c| c.status == ContractStatus::Active && c.finished_at.is_none()));
    assert_eq!(
        fingerprint(&conn, a.id),
        before,
        "run, cargo, ticks, MissionId and capture unchanged"
    );
    let updated: i64 = conn
        .query_row(
            "SELECT updated_at FROM contracts WHERE id = ?1",
            [a.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(updated, later);
    // updated_at never goes backwards.
    finish(&mut conn, &[b], FinalStatus::Completed, later + 5);
    restore_contracts(&mut conn, &[b], T0 + 5_000).unwrap();
    let ub: i64 = conn
        .query_row("SELECT updated_at FROM contracts WHERE id = ?1", [b], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(ub >= later + 5);
    assert!(history::get_history(&conn).unwrap().runs.is_empty());
}

#[test]
fn restore_refuses_all_or_nothing() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let a = simple(&mut conn, 0, "A", "10", "100");
    let active = simple(&mut conn, 0, "B", "10", "100");
    finish(&mut conn, &[a], FinalStatus::Completed, T0 + 1);
    assert!(matches!(
        restore_contracts(&mut conn, &[a, active], T0 + 2),
        Err(ReactivateError::NotFinished { id, ref status }) if id == active && status == "active"
    ));
    assert_eq!(
        restore_contracts(&mut conn, &[a, 9999], T0 + 2),
        Err(ReactivateError::NotFound(9999))
    );
    assert_eq!(
        restore_contracts(&mut conn, &[], T0 + 2),
        Err(ReactivateError::Empty)
    );
    assert_eq!(status(&conn, a), "completed", "nothing changed");
}

/* ------------------------------------------------------------------ clear */

fn frame() -> cargo_line_lib::capture::Frame {
    cargo_line_lib::capture::Frame {
        width: 1,
        height: 1,
        bgra: vec![0, 0, 0, 255],
        source: cargo_line_lib::capture::CaptureSourceKind::Window,
        backend: cargo_line_lib::capture::CaptureBackend::Wgc,
    }
}

/// Run, cargo, ticks, MissionId and capture link of one contract.
fn fingerprint(conn: &Connection, id: i64) -> String {
    conn.query_row(
        "SELECT c.run_id || '/' || IFNULL(c.game_mission_id, '-') || '/' ||
                (SELECT group_concat(q.commodity_name || q.quantity_scu_milli || ':' ||
                    (SELECT group_concat(s.location_name || IFNULL(s.reminder_done_at, '-'), ',') FROM cargo_stops s WHERE s.requirement_id = q.id), ';')
                 FROM cargo_requirements q WHERE q.contract_id = c.id) || '/' ||
                IFNULL((SELECT group_concat(id) FROM contract_captures WHERE contract_id = c.id), '-')
         FROM contracts c WHERE c.id = ?1",
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

fn file_of(conn: &Connection, dir: &Path, capture_id: i64) -> std::path::PathBuf {
    let path: String = conn
        .query_row(
            "SELECT file_path FROM contract_captures WHERE id = ?1",
            [capture_id],
            |r| r.get(0),
        )
        .unwrap();
    storage::file_in_dir(dir, &path).unwrap()
}

#[test]
fn clear_history_deletes_only_history() {
    let (dir, db) = db();
    let mut conn = db.connection().unwrap();
    let captures = dir.path().join("captures");
    ships::sync_catalog(&mut conn, ships::bundled().unwrap(), T0).unwrap();
    ships::set_favorite(&conn, "CRUS_Starlifter_C2", true, T0).unwrap();
    ships::set_selected(&mut conn, Some("CRUS_Starlifter_C2")).unwrap();

    // Run 1: finished only. Run 2: one finished, one active. Run 3: active only.
    let cap_done = storage::store(&conn, &captures, &frame(), T0).unwrap();
    let cap_active = storage::store(&conn, &captures, &frame(), T0 + 1).unwrap();
    let cap_unsaved = storage::store(&conn, &captures, &frame(), T0 + 2).unwrap();
    let done_link = SaveLinks {
        capture_id: Some(cap_done.id),
        ..SaveLinks::default()
    };
    let active_link = SaveLinks {
        capture_id: Some(cap_active.id),
        ..SaveLinks::default()
    };
    let r1a = add(&mut conn, 0, "R1 A", "10", "100", &done_link).id;
    let r1b = simple(&mut conn, 0, "R1 B", "10", "100");
    let r2a = simple(&mut conn, 1, "R2 A", "10", "100");
    let r2_active = add(&mut conn, 1, "R2 active", "10", "100", &active_link).id;
    let r3_active = simple(&mut conn, 2, "R3 active", "10", "100");
    finish(&mut conn, &[r1a, r2a], FinalStatus::Completed, T0 + 3 * DAY);
    finish(&mut conn, &[r1b], FinalStatus::Abandoned, T0 + 3 * DAY);
    let done_file = file_of(&conn, &captures, cap_done.id);
    let active_file = file_of(&conn, &captures, cap_active.id);
    let unsaved_file = file_of(&conn, &captures, cap_unsaved.id);

    let settings_before = settings::load(&conn).unwrap();
    let locations_before = count(&conn, "SELECT COUNT(*) FROM locations");
    let active_before = fingerprint(&conn, r2_active);

    let report = history::clear_history(&mut conn, Some(&captures)).unwrap();
    assert_eq!(
        (
            report.completed,
            report.abandoned,
            report.runs,
            report.captures
        ),
        (2, 1, 1, 1)
    );
    assert!(report.file_errors.is_empty());

    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM contracts WHERE status <> 'active'"
        ),
        0
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM contracts"), 2);
    assert_eq!(status(&conn, r2_active), "active");
    assert_eq!(status(&conn, r3_active), "active");
    assert_eq!(fingerprint(&conn, r2_active), active_before);
    let run_numbers: Vec<i64> = conn
        .prepare("SELECT run_number FROM runs ORDER BY run_number")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        run_numbers,
        [2, 3],
        "the emptied run is deleted; runs with active contracts are kept"
    );
    // Cargo cascaded.
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM cargo_requirements q WHERE NOT EXISTS (SELECT 1 FROM contracts c WHERE c.id = q.contract_id)"),
        0
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM cargo_requirements"), 2);
    // Captures: only the finished contract's row and file.
    assert!(!done_file.exists());
    assert!(active_file.exists() && unsaved_file.exists());
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM contract_captures"), 2);
    // Untouched.
    assert_eq!(settings::load(&conn).unwrap(), settings_before);
    assert_eq!(
        ships::get_selected(&mut conn)
            .unwrap()
            .ship
            .map(|s| s.id)
            .as_deref(),
        Some("CRUS_Starlifter_C2")
    );
    assert!(ships::list_ships(&conn)
        .unwrap()
        .iter()
        .any(|s| s.id == "CRUS_Starlifter_C2" && s.is_favorite));
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM locations"),
        locations_before
    );
    assert!(history::get_history(&conn).unwrap().runs.is_empty());

    // A second clear is a harmless no-op.
    let again = history::clear_history(&mut conn, Some(&captures)).unwrap();
    assert_eq!(
        (again.completed, again.abandoned, again.runs, again.captures),
        (0, 0, 0, 0)
    );
}

#[test]
fn run_numbers_are_never_reused() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let a = simple(&mut conn, 0, "A", "1", "1"); // run 1
    let b = simple(&mut conn, 1, "B", "1", "1"); // run 2
    finish(&mut conn, &[a, b], FinalStatus::Completed, T0 + 2 * DAY);
    assert_eq!(history::clear_history(&mut conn, None).unwrap().runs, 2);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM runs"), 0);

    // A new date continues after 2.
    let c = add(&mut conn, 5, "C", "1", "1", &SaveLinks::default());
    assert_eq!(c.run.run_number, 3);
    // The same date as a deleted run gets a new number too, not its old one.
    let d = add(&mut conn, 0, "D", "1", "1", &SaveLinks::default());
    assert_eq!(
        (d.run.run_number, d.run.run_date.as_str()),
        (4, date(0).as_str())
    );

    // The database itself refuses a reused number and a sequence that goes backwards.
    assert!(conn
        .execute("INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes) VALUES (1, '2030-01-01', 0, 0)", [])
        .is_err());
    assert!(conn
        .execute("UPDATE run_sequence SET last_run_number = 1", [])
        .is_err());
    assert!(conn.execute("DELETE FROM run_sequence", []).is_err());
}

/* ------------------------------------------------------------- migration */

#[test]
fn migration_0006_on_a_version_5_database_with_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE_NAME);
    {
        let mut conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 5).unwrap();
        conn.execute_batch(
            "INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes) VALUES (1, '2026-11-01', 0, 60), (2, '2026-11-02', 0, 60), (7, '2026-11-05', 0, 60);
             INSERT INTO contracts (run_id, title, reward_auec, status, accepted_at, finished_at, source, created_at, updated_at)
             VALUES (3, 'Old', 10, 'completed', 1, 2, 'manual', 1, 2);",
        )
        .unwrap();
    }
    let db = Database::open(&path).unwrap();
    let mut conn = db.connection().unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, LATEST_SCHEMA_VERSION);
    assert_eq!(
        count(
            &conn,
            "SELECT last_run_number FROM run_sequence WHERE id = 1"
        ),
        7,
        "seeded from existing runs"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM contracts"),
        1,
        "data kept"
    );
    history::clear_history(&mut conn, None).unwrap();
    let next = add(&mut conn, 0, "New", "1", "1", &SaveLinks::default());
    assert_eq!(next.run.run_number, 8);
}

/* ----------------------------------------------------------- performance */

#[test]
fn history_with_about_500_finished_contracts() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let mut ids = Vec::new();
    for i in 0..500 {
        // 50 runs of 10 contracts, two objectives each.
        let mut inp = input(&format!("Contract {i}"), "12.5", "25000");
        inp.requirements.push(NewRequirementInput {
            commodity_name: "Titanium".into(),
            quantity_scu: "4".into(),
            max_box_size_scu: Some("2".into()),
            pickups: vec![NewStopInput {
                location_name: "Everus Harbor".into(),
                quantity_scu: None,
            }],
            deliveries: vec![NewStopInput {
                location_name: "Seraphim Station".into(),
                quantity_scu: None,
            }],
        });
        let valid = validate_new_contract(&inp).unwrap();
        let day = i / 10;
        let at = AcceptanceTime {
            utc_ms: T0 + day * DAY,
            local_date: date(day),
            utc_offset_minutes: 60,
        };
        ids.push(
            create_contract_linked(
                &mut conn,
                &valid,
                &at,
                ContractSource::Manual,
                &SaveLinks::default(),
            )
            .unwrap()
            .id,
        );
    }
    let (abandoned, completed) = ids.split_at(100);
    finish(&mut conn, completed, FinalStatus::Completed, T0 + 60 * DAY);
    finish(&mut conn, abandoned, FinalStatus::Abandoned, T0 + 60 * DAY);

    let _warm = history::get_history(&conn).unwrap();
    let started = Instant::now();
    let runs = 5;
    let mut h = None;
    for _ in 0..runs {
        h = Some(history::get_history(&conn).unwrap());
    }
    let per_call = started.elapsed() / runs;
    let h = h.unwrap();
    eprintln!("[timing] get_history with 500 finished contracts (1000 objectives, 2000 stops): {per_call:?} per call");
    assert_eq!(h.runs.len(), 50);
    assert_eq!(h.stats.completed_contracts, 400);
    assert_eq!(h.stats.completed_scu_milli, 400 * 16_500);
    assert_eq!(h.stats.estimated_auec, 400 * 25_000);
    assert!(per_call.as_millis() < 1_000, "{per_call:?}");

    let started = Instant::now();
    let report = history::clear_history(&mut conn, None).unwrap();
    eprintln!(
        "[timing] clear_history of 500 contracts: {:?}",
        started.elapsed()
    );
    assert_eq!(
        (report.completed, report.abandoned, report.runs),
        (400, 100, 50)
    );
}
