//! Tests: backup export, validation of a chosen backup, safety copies, the startup swap
//! of a staged restore, and the allow-lists of the open-folder / open-link commands.
//! Temporary folders and synthetic data only.

use std::path::{Path, PathBuf};

use cargo_line_lib::backup::{
    self, apply_pending, export, inspect, link_url, open_folder_command, safety_copies,
    safety_copy, stage_restore, BackupError, StartupRestore, PENDING_FILE_NAME, SAFETY_COPIES_KEPT,
};
use cargo_line_lib::contracts::finalize::{finalize_contracts, FinalStatus};
use cargo_line_lib::contracts::repository::{create_contract_linked, SaveLinks};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::connection::APPLICATION_ID;
use cargo_line_lib::database::migrations::{migrations, LATEST_SCHEMA_VERSION};
use cargo_line_lib::database::models::ContractSource;
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::settings;
use cargo_line_lib::ships::{self, CustomShipInput};
use rusqlite::Connection;

const T0: i64 = 1_795_176_000_000;

fn add_contract(conn: &mut Connection, title: &str, day: &str) -> i64 {
    let valid = validate_new_contract(&NewContractInput {
        title: title.into(),
        reward_auec: "1000".into(),
        requirements: vec![NewRequirementInput {
            commodity_name: "Aluminum".into(),
            quantity_scu: "10".into(),
            max_box_size_scu: None,
            pickups: vec![NewStopInput {
                location_name: "Synthetic Pickup".into(),
                quantity_scu: None,
            }],
            deliveries: vec![NewStopInput {
                location_name: "Synthetic Dropoff".into(),
                quantity_scu: None,
            }],
        }],
    })
    .unwrap();
    let at = AcceptanceTime {
        utc_ms: T0,
        local_date: day.into(),
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

/// A database with active and finished contracts, a custom ship (favourite, selected) and a
/// changed setting.
fn populated(dir: &Path) -> Database {
    let db = Database::open(&dir.join(DATABASE_FILE_NAME)).unwrap();
    {
        let mut conn = db.connection().unwrap();
        add_contract(&mut conn, "Active Haul", "2026-11-20");
        let done = add_contract(&mut conn, "Finished Haul", "2026-11-21");
        finalize_contracts(&mut conn, &[done], FinalStatus::Completed, T0 + 1000).unwrap();
        let ship = ships::create_custom_ship(
            &mut conn,
            &CustomShipInput {
                name: "Backup Hauler".into(),
                cargo_scu: 42,
            },
        )
        .unwrap();
        ships::set_favorite(&conn, &ship.id, true, T0).unwrap();
        ships::set_selected(&mut conn, Some(&ship.id)).unwrap();
        settings::update(&mut conn, |s| s.capture_delay_ms = 2_300).unwrap();
    }
    db
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/* ------------------------------------------------------------------ export */

#[test]
fn export_is_a_valid_complete_copy() {
    let live = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let db = populated(live.path());
    let dest = out.path().join("cargo-line-backup-2026-11-26.sqlite3");
    let info = {
        let conn = db.connection().unwrap();
        export(&conn, &dest, live.path()).unwrap()
    };
    assert_eq!(
        (
            info.schema_version,
            info.active_contracts,
            info.finished_contracts,
            info.runs
        ),
        (LATEST_SCHEMA_VERSION, 1, 1, 2)
    );
    assert_eq!(info.bytes, std::fs::metadata(&dest).unwrap().len());
    assert!(
        !out.path()
            .join("cargo-line-backup-2026-11-26.sqlite3.partial")
            .exists(),
        "no partial file left"
    );

    // The copy opens as a normal database with everything in it.
    let copy = Connection::open(&dest).unwrap();
    let app_id: i32 = copy
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .unwrap();
    assert_eq!(app_id, APPLICATION_ID);
    let integrity: String = copy
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    assert_eq!(count(&copy, "SELECT COUNT(*) FROM contracts"), 2);
    assert_eq!(
        count(
            &copy,
            "SELECT COUNT(*) FROM ships WHERE availability_status = 'custom'"
        ),
        1
    );
    assert_eq!(count(&copy, "SELECT COUNT(*) FROM ship_favorites"), 1);
    assert_eq!(
        count(
            &copy,
            "SELECT last_run_number FROM run_sequence WHERE id = 1"
        ),
        2
    );
    assert!(
        count(&copy, "SELECT COUNT(*) FROM locations") >= 2,
        "the location catalog"
    );
    let settings = settings::load(&copy).unwrap();
    assert_eq!(settings.capture_delay_ms, 2_300);
    assert!(settings.selected_ship_id.unwrap().starts_with("custom:"));
    // Exporting again over the same (closed) file replaces it.
    drop(copy);
    let conn = db.connection().unwrap();
    export(&conn, &dest, live.path()).unwrap();
}

/* -------------------------------------------------------------- validation */

fn backup_file(dir: &Path) -> PathBuf {
    let live = tempfile::tempdir().unwrap();
    let db = populated(live.path());
    let dest = dir.join("backup.sqlite3");
    export(&db.connection().unwrap(), &dest, live.path()).unwrap();
    dest
}

#[test]
fn validation_refuses_bad_files() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");

    let text = dir.path().join("notes.sqlite3");
    std::fs::write(
        &text,
        "this is not a database, just some text that is long enough to have a header".repeat(20),
    )
    .unwrap();
    assert_eq!(inspect(&text, &work), Err(BackupError::NotSqlite));

    let foreign = dir.path().join("foreign.sqlite3");
    Connection::open(&foreign)
        .unwrap()
        .execute_batch("CREATE TABLE notes (body TEXT); INSERT INTO notes VALUES ('x');")
        .unwrap();
    assert_eq!(inspect(&foreign, &work), Err(BackupError::NotCargoLine));

    let newer = backup_file(dir.path());
    Connection::open(&newer)
        .unwrap()
        .pragma_update(None, "user_version", LATEST_SCHEMA_VERSION + 1)
        .unwrap();
    assert_eq!(
        inspect(&newer, &work),
        Err(BackupError::TooNew {
            found: LATEST_SCHEMA_VERSION + 1,
            supported: LATEST_SCHEMA_VERSION
        })
    );
    assert_eq!(
        BackupError::TooNew {
            found: 7,
            supported: 6
        }
        .to_string(),
        "This backup was made by a newer version of Cargo Line."
    );

    // Corrupted: valid header and application id, garbage in the pages after the first ones.
    let corrupt = dir.path().join("corrupt.sqlite3");
    std::fs::copy(backup_file(dir.path()), &corrupt).unwrap();
    let mut bytes = std::fs::read(&corrupt).unwrap();
    let page = 4096;
    assert!(bytes.len() > 6 * page, "enough pages to damage");
    let end = bytes.len() - page;
    bytes[2 * page..end].fill(0xA5);
    std::fs::write(&corrupt, &bytes).unwrap();
    assert!(
        matches!(inspect(&corrupt, &work), Err(BackupError::Corrupted(_))),
        "{:?}",
        inspect(&corrupt, &work)
    );

    // Validation never modifies the chosen file and leaves no scratch files.
    let good = backup_file(dir.path());
    let before = std::fs::read(&good).unwrap();
    inspect(&good, &work).unwrap();
    assert_eq!(std::fs::read(&good).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(&work).unwrap().count(),
        0,
        "scratch copy removed"
    );
}

#[test]
fn validation_accepts_the_same_and_an_older_version() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    assert_eq!(
        inspect(&backup_file(dir.path()), &work)
            .unwrap()
            .schema_version,
        LATEST_SCHEMA_VERSION
    );

    // A version-4 database with data: accepted, then migrated when the app opens it.
    let old = dir.path().join("old.sqlite3");
    {
        let mut conn = Connection::open(&old).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 4).unwrap();
        conn.execute_batch(
            "INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes) VALUES (1, '2026-11-01', 0, 0);
             INSERT INTO contracts (run_id, title, reward_auec, status, accepted_at, finished_at, source, created_at, updated_at)
             VALUES (1, 'Old Haul', 10, 'active', 1, NULL, 'manual', 1, 1);",
        )
        .unwrap();
    }
    let info = inspect(&old, &work).unwrap();
    assert_eq!((info.schema_version, info.active_contracts), (4, 1));

    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    stage_restore(&old, &data).unwrap();
    let db_file = data.join(DATABASE_FILE_NAME);
    assert!(
        matches!(apply_pending(&data, &db_file), StartupRestore::Restored(i) if i.schema_version == 4)
    );
    let db = Database::open(&db_file).unwrap();
    let conn = db.connection().unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, LATEST_SCHEMA_VERSION, "migrated on open");
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM contracts WHERE title = 'Old Haul'"
        ),
        1
    );
}

/* ------------------------------------------------------------ startup swap */

#[test]
fn a_pending_restore_replaces_the_database_and_stale_wal_files() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let db_file = data.join(DATABASE_FILE_NAME);
    let backup = backup_file(dir.path()); // 1 active + 1 finished, custom ship
    {
        // The current database: a different contract, which must be gone after the restore.
        let db = Database::open(&db_file).unwrap();
        add_contract(
            &mut db.connection().unwrap(),
            "Added After Backup",
            "2026-11-26",
        );
    }
    // Stale companions from a crash.
    std::fs::write(data.join(format!("{DATABASE_FILE_NAME}-wal")), b"stale wal").unwrap();
    std::fs::write(data.join(format!("{DATABASE_FILE_NAME}-shm")), b"stale shm").unwrap();

    stage_restore(&backup, &data).unwrap();
    let outcome = apply_pending(&data, &db_file);
    let StartupRestore::Restored(info) = &outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!((info.active_contracts, info.finished_contracts), (1, 1));
    assert_eq!(
        backup::restore_notice(&outcome).unwrap(),
        (true, "Backup restored (1 active contract, 1 in history). A safety copy of your previous data is in the backups folder.".into())
    );
    for gone in [
        PENDING_FILE_NAME.to_string(),
        format!("{DATABASE_FILE_NAME}-wal"),
        format!("{DATABASE_FILE_NAME}-shm"),
        format!("{DATABASE_FILE_NAME}.replaced"),
    ] {
        assert!(!data.join(&gone).exists(), "{gone} left behind");
    }
    let db = Database::open(&db_file).unwrap();
    let conn = db.connection().unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM contracts WHERE title = 'Added After Backup'"
        ),
        0
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM contracts"), 2);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM ships WHERE availability_status = 'custom'"
        ),
        1
    );
}

#[test]
fn a_bad_pending_file_keeps_the_current_database() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let db_file = data.join(DATABASE_FILE_NAME);
    {
        let db = Database::open(&db_file).unwrap();
        add_contract(&mut db.connection().unwrap(), "Keep Me", "2026-11-26");
    }
    std::fs::write(data.join(PENDING_FILE_NAME), "not a database".repeat(100)).unwrap();
    let outcome = apply_pending(&data, &db_file);
    let StartupRestore::Failed(message) = &outcome else {
        panic!("{outcome:?}")
    };
    assert!(message.contains("Your data was not changed"), "{message}");
    assert_eq!(backup::restore_notice(&outcome).map(|n| n.0), Some(false));
    assert!(
        !data.join(PENDING_FILE_NAME).exists(),
        "the pending file is removed"
    );
    let db = Database::open(&db_file).unwrap();
    assert_eq!(
        count(
            &db.connection().unwrap(),
            "SELECT COUNT(*) FROM contracts WHERE title = 'Keep Me'"
        ),
        1
    );
}

#[test]
fn no_pending_file_means_nothing_happens() {
    let dir = tempfile::tempdir().unwrap();
    let db_file = dir.path().join(DATABASE_FILE_NAME);
    assert_eq!(apply_pending(dir.path(), &db_file), StartupRestore::None);
    assert_eq!(backup::restore_notice(&StartupRestore::None), None);
    assert!(!db_file.exists());
}

/* ------------------------------------------------------------- safety copies */

#[test]
fn safety_copies_are_created_and_limited_to_five() {
    let live = tempfile::tempdir().unwrap();
    let db = populated(live.path());
    let backups = live.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    std::fs::write(backups.join("my-own-file.sqlite3"), b"not ours").unwrap();
    let conn = db.connection().unwrap();
    let mut made = Vec::new();
    for i in 0..7 {
        made.push(safety_copy(&conn, &backups, T0 + i * 60_000).unwrap());
    }
    let kept = safety_copies(&backups).unwrap();
    assert_eq!(kept.len(), SAFETY_COPIES_KEPT);
    assert_eq!(kept.first(), made.last(), "the newest is kept");
    assert!(
        !made[0].exists() && !made[1].exists(),
        "the two oldest are deleted"
    );
    assert!(made[0]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("before-restore-20261120T120000000Z"));
    assert!(
        backups.join("my-own-file.sqlite3").exists(),
        "other files are never touched"
    );
    assert_eq!(
        inspect(&kept[0], &live.path().join("work"))
            .unwrap()
            .active_contracts,
        1,
        "a safety copy is a valid backup"
    );
}

/* -------------------------------------------------------------- allow-lists */

#[test]
fn folder_and_link_allow_lists() {
    let data = Path::new(r"C:\Synthetic\AppData\io.github.mathisvkg.sccargoapp");
    let (program, target) = open_folder_command(data);
    assert_eq!(
        (program, target.as_path()),
        ("explorer.exe", data),
        "only ever the app data folder"
    );
    assert_eq!(
        link_url("repository"),
        Some("https://github.com/MathisVkg/cargoline")
    );
    assert_eq!(
        link_url("license"),
        Some("https://github.com/MathisVkg/cargoline/blob/main/LICENSE")
    );
    assert_eq!(link_url("wikiApi"), Some("https://api.star-citizen.wiki"));
    for unknown in [
        "",
        "https://example.com",
        "repo",
        "LICENSE",
        "file:///C:/Windows",
    ] {
        assert_eq!(link_url(unknown), None, "{unknown}");
    }
}
