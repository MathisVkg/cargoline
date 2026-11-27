//! Tests: the catalog sync skip, the deferred-startup gate, the watcher's "nothing to
//! do" plan on an unchanged file, the trace helpers and the data folder override. Temporary
//! files only.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use cargo_line_lib::database::{now_utc_ms, Database, DATABASE_FILE_NAME};
use cargo_line_lib::gamelog::tail::{plan_poll, Tail};
use cargo_line_lib::ships::{self, CustomShipInput};
use cargo_line_lib::startup::{FirstScreenGate, GateOutcome};
use cargo_line_lib::{data_dir_override, trace};

fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    (dir, db)
}

#[test]
fn catalog_sync_skips_when_the_stored_version_is_the_bundled_one() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let list = ships::bundled().unwrap();

    // First start: written.
    assert!(!ships::catalog_up_to_date(&conn, list).unwrap());
    let first = ships::sync_catalog_if_changed(&mut conn, list, now_utc_ms()).unwrap();
    let first = first.expect("an empty database is synced");
    assert_eq!(first.upserted, list.items.len());

    // Next starts: checked only, nothing written.
    let changes_before = conn.total_changes();
    assert!(ships::catalog_up_to_date(&conn, list).unwrap());
    assert_eq!(
        ships::sync_catalog_if_changed(&mut conn, list, now_utc_ms()).unwrap(),
        None
    );
    assert_eq!(conn.total_changes(), changes_before, "no row written");

    // A custom ship does not count as a catalog change.
    ships::create_custom_ship(
        &mut conn,
        &CustomShipInput {
            name: "Test Hauler".into(),
            cargo_scu: 100,
        },
    )
    .unwrap();
    assert!(ships::catalog_up_to_date(&conn, list).unwrap());

    // A different stored version (a new catalog in an update): synced again.
    conn.execute(
        "UPDATE catalog_metadata SET catalog_version = 'older' WHERE id = 1",
        [],
    )
    .unwrap();
    assert!(
        ships::sync_catalog_if_changed(&mut conn, list, now_utc_ms())
            .unwrap()
            .is_some()
    );
    assert!(ships::catalog_up_to_date(&conn, list).unwrap());

    // Same version but a listed ship missing (an odd database): synced again, ship back.
    let id = list.items[0].id.clone();
    conn.execute(
        "UPDATE ships SET availability_status = 'removed' WHERE id = ?1",
        [&id],
    )
    .unwrap();
    assert!(!ships::catalog_up_to_date(&conn, list).unwrap());
    let report = ships::sync_catalog_if_changed(&mut conn, list, now_utc_ms())
        .unwrap()
        .unwrap();
    assert_eq!(report.upserted, 1);
    assert!(ships::list_ships(&conn).unwrap().iter().any(|s| s.id == id));
}

#[test]
fn deferred_startup_work_waits_for_the_first_screen() {
    let gate = Arc::new(FirstScreenGate::default());
    assert!(!gate.is_open());
    let waiter = {
        let gate = Arc::clone(&gate);
        std::thread::spawn(move || {
            let t = Instant::now();
            (gate.wait(Duration::from_secs(10)), t.elapsed())
        })
    };
    std::thread::sleep(Duration::from_millis(50));
    gate.open(); // the first contracts list was served
    let (outcome, waited) = waiter.join().unwrap();
    assert_eq!(outcome, GateOutcome::Opened);
    assert!(
        waited < Duration::from_secs(5),
        "woken at once, not at the limit"
    );
    assert!(gate.is_open());
    // Already open: no waiting; opening again is harmless.
    gate.open();
    assert_eq!(gate.wait(Duration::from_secs(10)), GateOutcome::Opened);
}

#[test]
fn deferred_startup_work_still_runs_when_the_first_screen_never_comes() {
    // No window or a failing list: the cleanup goes ahead after the limit.
    let gate = FirstScreenGate::default();
    let t = Instant::now();
    assert_eq!(gate.wait(Duration::from_millis(80)), GateOutcome::TimedOut);
    assert!(t.elapsed() >= Duration::from_millis(80));
    assert_eq!(
        cargo_line_lib::startup::FIRST_SCREEN_WAIT,
        Duration::from_secs(5)
    );
}

#[test]
fn watcher_plans_nothing_for_an_unchanged_file() {
    let created = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000));
    // Unchanged: same length, same creation time → nothing opened or read.
    let plan = plan_poll(4_096, created, 4_096, created);
    assert!(!plan.restart);
    assert!(!plan.reads(4_096));
    // Grown: only the new bytes.
    let plan = plan_poll(4_096, created, 5_000, created);
    assert_eq!((plan.restart, plan.from), (false, 4_096));
    assert!(plan.reads(5_000));
    // Truncated, or replaced by a new file: from the start.
    assert_eq!(plan_poll(4_096, created, 100, created).from, 0);
    let newer = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(2_000));
    let plan = plan_poll(4_096, created, 5_000, newer);
    assert!(plan.restart);
    assert_eq!(plan.from, 0);
    // Creation time unknown on either side: not treated as replaced.
    assert!(!plan_poll(10, None, 10, created).reads(10));
}

#[test]
fn watcher_reads_only_new_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Game.log");
    std::fs::write(&path, "old line\n").unwrap();
    let mut tail = Tail::new(&path);
    assert!(tail.poll().unwrap().is_empty(), "starts at the end");
    for _ in 0..3 {
        assert!(tail.poll().unwrap().is_empty(), "unchanged file");
    }
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(f, "new line").unwrap();
    drop(f);
    assert_eq!(tail.poll().unwrap(), vec!["new line".to_string()]);
    assert!(tail.poll().unwrap().is_empty());
}

#[test]
fn trace_helpers() {
    for on in ["1", "true", "YES", " on "] {
        assert!(trace::is_enabled_value(Some(on)), "{on}");
    }
    for off in [None, Some(""), Some("0"), Some("false"), Some("2")] {
        assert!(!trace::is_enabled_value(off), "{off:?}");
    }
    assert_eq!(
        trace::sanitize_name("ui: page cargo shown"),
        "ui: page cargo shown"
    );
    assert_eq!(trace::sanitize_name("a\nb\"c<d>"), "a?b?c?d?");
    assert_eq!(
        trace::sanitize_name(&"x".repeat(100)).len(),
        trace::MAX_NAME_CHARS
    );
    assert_eq!(
        trace::format_line(412.64, "database opened"),
        "[trace]     412.6 ms  database opened"
    );
    // Not initialised in tests (the variable is not set): marks are ignored.
    trace::mark("ignored");
    assert!(!trace::enabled());
}

#[test]
fn data_folder_override() {
    let abs: OsString = if cfg!(windows) {
        r"C:\perf\data".into()
    } else {
        "/perf/data".into()
    };
    // CARGO_LINE_DATA_DIR: any build, absolute paths only.
    assert_eq!(
        data_dir_override(false, Some(abs.clone()), None),
        Some(PathBuf::from(&abs))
    );
    assert_eq!(
        data_dir_override(false, Some("relative".into()), None),
        None
    );
    assert_eq!(data_dir_override(false, Some("".into()), None), None);
    // CARGO_LINE_DEV_DATA_DIR: debug builds only.
    assert_eq!(data_dir_override(false, None, Some(abs.clone())), None);
    assert_eq!(
        data_dir_override(true, None, Some(abs.clone())),
        Some(PathBuf::from(&abs))
    );
    // Neither set: the normal folders.
    assert_eq!(data_dir_override(true, None, None), None);
    assert_eq!(data_dir_override(false, None, None), None);
}
