//! Headless tests for screenshot capture: window selection, storage,
//! linking, discarding, cleanup, the new setting and migration 0004.
//! Temporary folders and databases only; no desktop access.

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use cargo_line_lib::capture::select::{any_star_citizen, select_star_citizen, WindowCandidate};
use cargo_line_lib::capture::storage::{
    self, cleanup, discard_unlinked, encode_png, file_for_serving, file_in_dir, file_name,
    is_capture_file_name, link, store, utc_stamp, CAPTURES_DIR_NAME, ORPHAN_GRACE, RETENTION_MS,
};
use cargo_line_lib::capture::{
    CaptureBackend, CaptureError, CaptureSourceKind, CaptureState, Frame,
};
use cargo_line_lib::contracts::repository::create_contract_with_capture;
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::migrations::{migrations, LATEST_SCHEMA_VERSION};
use cargo_line_lib::database::models::ContractSource;
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::settings::{self, AppSettings, SettingsError};
use rusqlite::{params, Connection};
use tempfile::TempDir;

const T0: i64 = 1_795_600_000_000; // 2026-11-25 09:46:40 UTC

/* ------------------------------------------------------------- helpers */

fn win(handle: isize, w: i32, h: i32) -> WindowCandidate {
    WindowCandidate {
        handle,
        exe: "StarCitizen.exe".into(),
        class: "CryENGINE".into(),
        visible: true,
        minimized: false,
        cloaked: false,
        width: w,
        height: h,
    }
}

struct Env {
    _tmp: TempDir,
    db: Database,
    dir: std::path::PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(&tmp.path().join(DATABASE_FILE_NAME)).unwrap();
    let dir = tmp.path().join(CAPTURES_DIR_NAME);
    Env { _tmp: tmp, db, dir }
}

/// A 4×2 BGRA test pattern.
fn frame() -> Frame {
    let mut bgra = Vec::new();
    for i in 0..8u8 {
        bgra.extend_from_slice(&[i, 100 + i, 200 + i, 255]); // B, G, R, A
    }
    Frame {
        width: 4,
        height: 2,
        bgra,
        source: CaptureSourceKind::Window,
        backend: CaptureBackend::Wgc,
    }
}

fn contract_input() -> NewContractInput {
    NewContractInput {
        title: "Junior Rank - Direct Medium Cargo Haul".into(),
        reward_auec: "57750".into(),
        requirements: vec![NewRequirementInput {
            commodity_name: "Aluminum".into(),
            quantity_scu: "38".into(),
            max_box_size_scu: None,
            pickups: vec![NewStopInput {
                location_name: "ARC-L1 Wide Forest Station".into(),
                quantity_scu: None,
            }],
            deliveries: vec![NewStopInput {
                location_name: "Everus Harbor".into(),
                quantity_scu: None,
            }],
        }],
    }
}

fn accepted() -> AcceptanceTime {
    AcceptanceTime {
        utc_ms: T0,
        local_date: "2026-11-19".into(),
        utc_offset_minutes: 60,
    }
}

fn rows(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM contract_captures", [], |r| r.get(0))
        .unwrap()
}

fn age_file(path: &Path, by: Duration) {
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - by).unwrap();
}

/* ------------------------------------------------------ window selection */

#[test]
fn selection_none_is_window_not_found() {
    assert_eq!(select_star_citizen(&[]), Err(CaptureError::WindowNotFound));
    let mut launcher = win(1, 1200, 800);
    launcher.exe = "RSI Launcher.exe".into();
    let mut other_class = win(2, 3840, 2160);
    other_class.class = "Chrome_WidgetWin_1".into();
    assert_eq!(
        select_star_citizen(&[launcher, other_class]),
        Err(CaptureError::WindowNotFound)
    );
}

#[test]
fn selection_one_window() {
    let list = [win(7, 3840, 2160)];
    assert_eq!(select_star_citizen(&list).unwrap().handle, 7);
}

#[test]
fn selection_several_takes_the_largest_visible() {
    let mut hidden = win(1, 7680, 4320);
    hidden.visible = false;
    let mut cloaked = win(2, 7680, 4320);
    cloaked.cloaked = true;
    let list = [
        win(3, 800, 600),
        hidden,
        cloaked,
        win(4, 2560, 1440),
        win(5, 1920, 1080),
    ];
    assert_eq!(select_star_citizen(&list).unwrap().handle, 4);
    // Case-insensitive names.
    let mut lower = win(9, 100, 100);
    lower.exe = "starcitizen.EXE".into();
    lower.class = "cryengine".into();
    assert_eq!(select_star_citizen(&[lower]).unwrap().handle, 9);
}

#[test]
fn selection_minimized() {
    let mut only = win(1, 160, 28);
    only.minimized = true;
    assert_eq!(
        select_star_citizen(std::slice::from_ref(&only)),
        Err(CaptureError::Minimized)
    );
    // A normal window wins over a minimised one.
    assert_eq!(
        select_star_citizen(&[only.clone(), win(2, 1280, 720)])
            .unwrap()
            .handle,
        2
    );
    // Display mode still finds the minimised window (for its monitor).
    assert_eq!(any_star_citizen(&[only]).unwrap().handle, 1);
}

#[test]
fn only_one_capture_at_a_time() {
    let state = CaptureState::default();
    let slot = state.try_begin().unwrap();
    assert!(state.is_busy());
    assert_eq!(state.try_begin().unwrap_err(), CaptureError::Busy);
    drop(slot);
    assert!(!state.is_busy());
    let moved = state.try_begin().unwrap();
    std::thread::spawn(move || drop(moved)).join().unwrap(); // released from another thread
    assert!(state.try_begin().is_ok());
}

/* ------------------------------------------------------------- naming */

#[test]
fn file_names_are_utc_and_safe() {
    assert_eq!(utc_stamp(0), "19700101T000000000Z");
    assert_eq!(utc_stamp(951_782_400_123), "20000229T000000123Z"); // leap day
    assert_eq!(utc_stamp(T0), "20261125T094640000Z");
    let name = file_name(T0, "0a1b2c3d");
    assert_eq!(name, "20261125T094640000Z-0a1b2c3d.png");
    assert!(is_capture_file_name(&name));
    for bad in [
        "../x.png",
        "a\\b.png",
        "x.png.exe",
        ".png",
        "x y.png",
        "x.PNG",
        "x.jpg",
    ] {
        assert!(!is_capture_file_name(bad), "{bad}");
    }
    let dir = Path::new("C:\\data\\captures");
    assert_eq!(
        file_in_dir(dir, "D:\\elsewhere\\abc.png"),
        Some(dir.join("abc.png"))
    );
    assert_eq!(file_in_dir(dir, "C:\\Windows\\system.ini"), None);
}

/* ---------------------------------------------------------------- PNG */

#[test]
fn png_is_rgb_at_native_size() {
    let mut bytes = Vec::new();
    encode_png(&frame(), &mut bytes).unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(
        (info.width, info.height, info.color_type),
        (4, 2, png::ColorType::Rgb)
    );
    assert_eq!(&buf[..6], &[200, 100, 0, 201, 101, 1]); // BGRA → RGB
    let mut bad = frame();
    bad.bgra.pop();
    assert!(matches!(
        encode_png(&bad, Vec::new()),
        Err(storage::StorageError::BadFrame)
    ));
}

/* ------------------------------------------------------ store and link */

#[test]
fn store_writes_file_and_metadata() {
    let e = env();
    let conn = e.db.connection().unwrap();
    let info = store(&conn, &e.dir, &frame(), T0).unwrap();
    assert_eq!(info.contract_id, None);
    assert_eq!((info.captured_at, info.expires_at), (T0, T0 + RETENTION_MS));
    assert_eq!((info.width, info.height), (Some(4), Some(2)));
    assert_eq!(info.source, Some(CaptureSourceKind::Window));
    assert_eq!(info.backend, Some(CaptureBackend::Wgc));
    let path = Path::new(&info.file_path);
    assert_eq!(path.parent().unwrap(), e.dir);
    assert!(path.is_file());
    assert!(path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("20261125T094640000Z-"));
    let method: String = conn
        .query_row(
            "SELECT capture_method FROM contract_captures WHERE id = ?1",
            [info.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(method, "manual");
    // Two captures in the same millisecond still get distinct files.
    let second = store(&conn, &e.dir, &frame(), T0).unwrap();
    assert_ne!(second.file_path, info.file_path);
    assert_eq!(
        file_for_serving(&conn, &e.dir, info.id).unwrap(),
        Some(path.to_path_buf())
    );
    assert_eq!(file_for_serving(&conn, &e.dir, 9999).unwrap(), None);
}

#[test]
fn saving_a_contract_links_its_capture_in_the_same_transaction() {
    let e = env();
    let mut conn = e.db.connection().unwrap();
    let capture = store(&conn, &e.dir, &frame(), T0).unwrap();
    let valid = validate_new_contract(&contract_input()).unwrap();
    let saved = create_contract_with_capture(
        &mut conn,
        &valid,
        &accepted(),
        ContractSource::Manual,
        Some(capture.id),
    )
    .unwrap();
    let linked = storage::get(&conn, capture.id).unwrap().unwrap();
    assert_eq!(linked.contract_id, Some(saved.id));
    // A linked capture is never discarded by the form.
    assert!(!discard_unlinked(&conn, &e.dir, capture.id).unwrap());
    assert!(Path::new(&linked.file_path).is_file());
    // Already linked: linking again changes nothing.
    assert!(!link(&conn, capture.id, saved.id).unwrap());
}

#[test]
fn saving_without_or_with_a_missing_capture_still_saves() {
    let e = env();
    let mut conn = e.db.connection().unwrap();
    let valid = validate_new_contract(&contract_input()).unwrap();
    create_contract_with_capture(&mut conn, &valid, &accepted(), ContractSource::Manual, None)
        .unwrap();
    let saved = create_contract_with_capture(
        &mut conn,
        &valid,
        &accepted(),
        ContractSource::Manual,
        Some(424242),
    )
    .unwrap();
    assert_eq!(saved.title, "Junior Rank - Direct Medium Cargo Haul");
    let contracts: i64 = conn
        .query_row("SELECT COUNT(*) FROM contracts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(contracts, 2);
}

#[test]
fn a_failed_save_does_not_link() {
    let e = env();
    let mut conn = e.db.connection().unwrap();
    let capture = store(&conn, &e.dir, &frame(), T0).unwrap();
    let valid = validate_new_contract(&contract_input()).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_contract BEFORE INSERT ON contracts BEGIN SELECT RAISE(ABORT, 'boom'); END;",
    )
    .unwrap();
    assert!(create_contract_with_capture(
        &mut conn,
        &valid,
        &accepted(),
        ContractSource::Manual,
        Some(capture.id)
    )
    .is_err());
    assert_eq!(
        storage::get(&conn, capture.id)
            .unwrap()
            .unwrap()
            .contract_id,
        None
    );
}

#[test]
fn cancel_and_recapture_delete_the_unsaved_capture() {
    let e = env();
    let conn = e.db.connection().unwrap();
    // Recapture: the new one is stored, then the previous unsaved one is deleted.
    let first = store(&conn, &e.dir, &frame(), T0).unwrap();
    let second = store(&conn, &e.dir, &frame(), T0 + 1000).unwrap();
    assert!(discard_unlinked(&conn, &e.dir, first.id).unwrap());
    assert!(!Path::new(&first.file_path).exists());
    assert!(storage::get(&conn, first.id).unwrap().is_none());
    // Cancel: row and file go.
    assert!(discard_unlinked(&conn, &e.dir, second.id).unwrap());
    assert!(!Path::new(&second.file_path).exists());
    assert_eq!(rows(&conn), 0);
    // Discarding twice, or a capture whose file is already gone, is fine.
    assert!(!discard_unlinked(&conn, &e.dir, second.id).unwrap());
    let third = store(&conn, &e.dir, &frame(), T0).unwrap();
    fs::remove_file(&third.file_path).unwrap();
    assert!(discard_unlinked(&conn, &e.dir, third.id).unwrap());
}

/* ------------------------------------------------------------- cleanup */

#[test]
fn cleanup_removes_expired_captures_linked_or_not() {
    let e = env();
    let mut conn = e.db.connection().unwrap();
    let old = store(&conn, &e.dir, &frame(), T0).unwrap();
    let valid = validate_new_contract(&contract_input()).unwrap();
    create_contract_with_capture(
        &mut conn,
        &valid,
        &accepted(),
        ContractSource::Manual,
        Some(old.id),
    )
    .unwrap();
    let fresh = store(&conn, &e.dir, &frame(), T0 + RETENTION_MS - 1).unwrap();
    let report = cleanup(&conn, &e.dir, T0 + RETENTION_MS, false);
    assert_eq!(report.expired, 1);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!Path::new(&old.file_path).exists());
    assert!(storage::get(&conn, old.id).unwrap().is_none());
    assert!(Path::new(&fresh.file_path).is_file());
    // The contract itself is untouched.
    let contracts: i64 = conn
        .query_row("SELECT COUNT(*) FROM contracts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(contracts, 1);
}

#[test]
fn startup_cleanup_removes_unsaved_captures_but_hourly_does_not() {
    let e = env();
    let conn = e.db.connection().unwrap();
    let unsaved = store(&conn, &e.dir, &frame(), T0).unwrap();
    assert_eq!(cleanup(&conn, &e.dir, T0 + 1, false).unlinked, 0);
    assert!(Path::new(&unsaved.file_path).is_file());
    assert_eq!(cleanup(&conn, &e.dir, T0 + 1, true).unlinked, 1);
    assert!(!Path::new(&unsaved.file_path).exists());
    assert_eq!(rows(&conn), 0);
}

#[test]
fn cleanup_removes_rows_whose_file_is_gone() {
    let e = env();
    let conn = e.db.connection().unwrap();
    let gone = store(&conn, &e.dir, &frame(), T0).unwrap();
    let kept = store(&conn, &e.dir, &frame(), T0).unwrap();
    fs::remove_file(&gone.file_path).unwrap();
    let report = cleanup(&conn, &e.dir, T0 + 1, false);
    assert_eq!(report.missing_files, 1);
    assert!(storage::get(&conn, gone.id).unwrap().is_none());
    assert!(storage::get(&conn, kept.id).unwrap().is_some());
}

#[test]
fn cleanup_removes_old_orphan_files_only() {
    // File ages use the real clock here.
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let e = env();
    let conn = e.db.connection().unwrap();
    let kept = store(&conn, &e.dir, &frame(), now).unwrap();
    age_file(Path::new(&kept.file_path), ORPHAN_GRACE * 3); // old, but it has a row
    let old_orphan = e.dir.join("20260101T000000000Z-aaaaaaaa.png");
    let new_orphan = e.dir.join("20260101T000000000Z-bbbbbbbb.png");
    let not_ours = e.dir.join("notes.txt");
    for p in [&old_orphan, &new_orphan, &not_ours] {
        fs::write(p, b"x").unwrap();
    }
    age_file(&old_orphan, ORPHAN_GRACE + Duration::from_secs(60));
    age_file(&not_ours, ORPHAN_GRACE * 10);
    let report = cleanup(&conn, &e.dir, now, false);
    assert_eq!(report.orphan_files, 1);
    assert!(!old_orphan.exists(), "old orphan deleted");
    assert!(
        new_orphan.exists(),
        "a file that may be being written is kept"
    );
    assert!(
        not_ours.exists(),
        "files that are not captures are never touched"
    );
    assert!(
        Path::new(&kept.file_path).is_file(),
        "files with a row are kept"
    );
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

#[test]
fn cleanup_never_deletes_outside_the_captures_folder() {
    let e = env();
    let conn = e.db.connection().unwrap();
    let outside = e._tmp.path().join("precious.txt");
    fs::write(&outside, b"keep me").unwrap();
    conn.execute(
        "INSERT INTO contract_captures (file_path, captured_at, expires_at, capture_method)
         VALUES (?1, ?2, ?3, 'manual')",
        params![outside.to_string_lossy(), T0, T0 + 1],
    )
    .unwrap();
    let report = cleanup(&conn, &e.dir, T0 + 10, true);
    assert_eq!(report.expired, 1); // the row goes …
    assert!(outside.exists()); // … the file outside `captures\` stays
}

#[test]
fn cleanup_failures_are_reported_not_raised() {
    let e = env();
    let conn = e.db.connection().unwrap();
    // `captures` is a file, not a folder: listing it fails.
    fs::write(&e.dir, b"not a folder").unwrap();
    let report = cleanup(&conn, &e.dir, T0, true);
    assert!(!report.errors.is_empty());
    // A broken table is reported too.
    conn.execute_batch("DROP TABLE contract_captures;").unwrap();
    let report = cleanup(&conn, &e.dir, T0, true);
    assert!(report.errors.iter().any(|e| e.contains("listing captures")));
}

#[test]
fn cleanup_without_a_captures_folder_is_a_no_op() {
    let e = env();
    let conn = e.db.connection().unwrap();
    assert_eq!(
        cleanup(&conn, &e.dir, T0, true),
        storage::CleanupReport::default()
    );
}

/* ------------------------------------------------------------ settings */

#[test]
fn capture_on_add_contract_defaults_on_and_is_validated() {
    let e = env();
    let mut conn = e.db.connection().unwrap();
    let defaults = AppSettings::default();
    assert!(defaults.capture_on_add_contract);
    assert_eq!(defaults.capture_delay_ms, 1000);
    assert!(settings::load(&conn).unwrap().capture_on_add_contract);

    let updated = settings::update(&mut conn, |s| {
        s.capture_on_add_contract = false;
        s.capture_delay_ms = 2_300;
    })
    .unwrap();
    assert!(!updated.capture_on_add_contract);
    assert!(!settings::load(&conn).unwrap().capture_on_add_contract);

    // Out-of-range delays are refused and nothing is written.
    for bad in [5_100, 250] {
        assert!(matches!(
            settings::update(&mut conn, |s| {
                s.capture_on_add_contract = true;
                s.capture_delay_ms = bad;
            }),
            Err(SettingsError::Invalid(_))
        ));
    }
    let stored = settings::load(&conn).unwrap();
    assert_eq!(
        (stored.capture_on_add_contract, stored.capture_delay_ms),
        (false, 2_300)
    );

    // A stored non-boolean is reported, never silently replaced.
    conn.execute(
        "UPDATE app_settings SET value = '\"yes\"' WHERE key = 'capture_on_add_contract'",
        [],
    )
    .unwrap();
    assert!(matches!(
        settings::load(&conn),
        Err(SettingsError::InvalidStored(_))
    ));
}

/* ------------------------------------------------------- migration 0004 */

#[test]
fn migration_0004_adds_capture_details_to_a_version_3_database_with_data() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(DATABASE_FILE_NAME);
    {
        let mut conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 3).unwrap();
        conn.execute_batch(
            "INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes) VALUES (1, '2026-11-18', 0, 60);
             INSERT INTO contracts (run_id, title, reward_auec, accepted_at, source, created_at, updated_at)
               VALUES (1, 'Old', 5, 0, 'manual', 0, 0);
             INSERT INTO contract_captures (contract_id, file_path, captured_at, expires_at, capture_method)
               VALUES (1, 'C:\\old\\capture.png', 0, 1000, 'manual');",
        )
        .unwrap();
    }
    let db = Database::open(&path).unwrap();
    let conn = db.connection().unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert!(version >= 4);
    assert_eq!(version, LATEST_SCHEMA_VERSION);
    let old = storage::get(&conn, 1).unwrap().unwrap();
    assert_eq!(old.contract_id, Some(1));
    assert_eq!(
        (old.source, old.backend, old.width, old.height),
        (None, None, None, None)
    );
    // The new columns are checked.
    for (column, value) in [("capture_source", "screen"), ("capture_backend", "dxgi")] {
        assert!(conn
            .execute(
                &format!("UPDATE contract_captures SET {column} = ?1 WHERE id = 1"),
                [value]
            )
            .is_err());
    }
    assert!(conn
        .execute("UPDATE contract_captures SET width = 0 WHERE id = 1", [])
        .is_err());
}

#[test]
fn a_contract_filled_from_a_screenshot_is_saved_with_source_ocr() {
    let e = env();
    let mut conn = e.db.connection().unwrap();
    let capture = store(&conn, &e.dir, &frame(), T0).unwrap();
    let valid = validate_new_contract(&contract_input()).unwrap();
    let saved = create_contract_with_capture(
        &mut conn,
        &valid,
        &accepted(),
        ContractSource::Ocr,
        Some(capture.id),
    )
    .unwrap();
    let source: String = conn
        .query_row(
            "SELECT source FROM contracts WHERE id = ?1",
            [saved.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, "ocr");
    assert_eq!(
        storage::get(&conn, capture.id)
            .unwrap()
            .unwrap()
            .contract_id,
        Some(saved.id)
    );
}
