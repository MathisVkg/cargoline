//! Headless OCR tests (Task 08): no real engine, no desktop. Fake engines are small .cmd scripts
//! in temporary folders; databases are temporary.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use cargo_line_lib::capture::storage::{
    self, begin_processing, fail_interrupted, finish_processing, processing_status, store,
    ProcessingStatus, CAPTURES_DIR_NAME,
};
use cargo_line_lib::capture::{CaptureBackend, CaptureSourceKind, Frame};
use cargo_line_lib::database::migrations::{migrations, LATEST_SCHEMA_VERSION};
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::ocr::engine::{run_child, CancelToken, Engine, TempFile};
use cargo_line_lib::ocr::image::Image;
use cargo_line_lib::ocr::layout::{
    find_anchors, looks_like_objectives, plan_regions, screen_state,
};
use cargo_line_lib::ocr::tsv::{parse_tsv, Mapping};
use cargo_line_lib::ocr::{
    AnchorsFound, OcrBox, OcrError, OcrLine, RegionKind, RegionSource, ScreenState,
};
use rusqlite::Connection;
use tempfile::TempDir;

const HEADER: &str =
    "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

#[allow(clippy::too_many_arguments)] // mirrors the TSV columns
fn word_row(
    block: u32,
    line: u32,
    word: u32,
    left: i32,
    top: i32,
    w: i32,
    h: i32,
    conf: f32,
    text: &str,
) -> String {
    format!("5\t1\t{block}\t1\t{line}\t{word}\t{left}\t{top}\t{w}\t{h}\t{conf}\t{text}")
}

/// One-line helper: a line of words laid out left to right at `y`, 20 px high.
fn line(text: &str, x: i32, y: i32) -> OcrLine {
    let tsv: Vec<String> =
        std::iter::once(HEADER.to_string())
            .chain(text.split(' ').enumerate().map(|(i, w)| {
                word_row(1, 1, i as u32 + 1, x + i as i32 * 120, y, 100, 20, 90.0, w)
            }))
            .collect();
    parse_tsv(&tsv.join("\n"), Mapping::IDENTITY).remove(0)
}

/* ------------------------------------------------------------------ TSV */

#[test]
fn tsv_groups_words_into_lines_with_boxes_and_confidence() {
    let tsv = [
        HEADER.to_string(),
        "1\t1\t0\t0\t0\t0\t0\t0\t3840\t2160\t-1\t".into(), // page row
        "4\t1\t1\t1\t1\t0\t10\t10\t300\t20\t-1\t".into(),  // line row
        word_row(1, 1, 1, 10, 10, 100, 20, 96.5, "PRIMARY"),
        word_row(1, 1, 2, 120, 12, 150, 18, 90.5, "OBJECTIVES"),
        word_row(1, 1, 3, 300, 10, 5, 5, -1.0, "   "), // blank word
        word_row(1, 2, 1, 10, 40, 80, 20, 70.0, "Deliver"),
        word_row(2, 1, 1, 500, 500, 60, 20, 50.0, "ABANDON"),
        "5\t1\t2\t1\t1\t2\tbad\t1\t1\t1\t1\tbroken".into(), // malformed numbers
        "garbage".into(),
        String::new(),
    ]
    .join("\r\n");
    let lines = parse_tsv(&tsv, Mapping::IDENTITY);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].text, "PRIMARY OBJECTIVES");
    assert_eq!(lines[0].confidence, 93.5);
    assert_eq!(
        lines[0].bbox,
        OcrBox {
            x: 10,
            y: 10,
            width: 260,
            height: 20
        }
    );
    assert_eq!(lines[0].words[1].confidence, 90.5);
    assert_eq!(lines[1].text, "Deliver");
    assert_eq!(lines[2].text, "ABANDON");
    assert!(parse_tsv("", Mapping::IDENTITY).is_empty());
    assert!(parse_tsv(HEADER, Mapping::IDENTITY).is_empty());
}

#[test]
fn tsv_boxes_map_back_to_the_source_image() {
    let tsv = [
        HEADER.to_string(),
        word_row(1, 1, 1, 100, 50, 40, 20, 80.0, "x"),
    ]
    .join("\n");
    // Read from a 2× upscaled crop at (1000, 500).
    let up = parse_tsv(
        &tsv,
        Mapping {
            offset_x: 1000.0,
            offset_y: 500.0,
            scale: 2.0,
        },
    );
    assert_eq!(
        up[0].bbox,
        OcrBox {
            x: 1050,
            y: 525,
            width: 20,
            height: 10
        }
    );
    // Read from a half-size anchor pass.
    let half = parse_tsv(
        &tsv,
        Mapping {
            offset_x: 0.0,
            offset_y: 0.0,
            scale: 0.5,
        },
    );
    assert_eq!(
        half[0].bbox,
        OcrBox {
            x: 200,
            y: 100,
            width: 80,
            height: 40
        }
    );
    // Unknown confidence on every word.
    let unknown = [HEADER.to_string(), word_row(1, 1, 1, 0, 0, 1, 1, -1.0, "?")].join("\n");
    assert_eq!(parse_tsv(&unknown, Mapping::IDENTITY)[0].confidence, -1.0);
}

/* --------------------------------------------------------------- layout */

#[test]
fn screen_state_comes_from_the_reward_card() {
    // Accepted: the tokens of a real Task 06 / 10 capture (positions at 4K).
    let accepted = vec![
        line("Junior Rank - Direct Medium Cargo Haul", 1300, 380),
        line("Reward", 2604, 400),
        line("Contract Deadline", 2600, 444),
        line("Contracted By", 2600, 492),
        line("DETAILS", 1290, 570),
        line("PRIMARY OBJECTIVES @", 2454, 634),
        line("ABANBON", 1326, 1700),
        line("TRACK", 3406, 1698),
    ];
    let anchors = find_anchors(&accepted);
    assert_eq!(anchors.header.unwrap().text, "PRIMARY OBJECTIVES @");
    assert_eq!(anchors.details.unwrap().text, "DETAILS");
    assert_eq!(anchors.reward.unwrap().text, "Reward");
    assert_eq!(
        anchors.found(),
        AnchorsFound {
            header: true,
            details: true,
            reward: true
        }
    );
    assert_eq!(screen_state(&anchors), ScreenState::Accepted);

    // Offers: "Contract Availability"; the tab bar's "ACCEPTED (0/10)" never counts.
    let offer = vec![
        line("OFFERS ACCEPTED (0/10) HISTORY BEACONS", 1146, 214),
        line("Reward", 2598, 401),
        line("Contract Availability", 2594, 446),
    ];
    assert_eq!(screen_state(&find_anchors(&offer)), ScreenState::Offer);

    // No dependency on the bottom buttons: buttons alone, or the card without its label, are unknown.
    for lines in [
        vec![line("Reward", 2598, 401), line("ACCEPT OFFER", 3246, 1694)],
        vec![
            line("Reward", 2598, 401),
            line("ABANDON", 1326, 1700),
            line("TRACK", 3406, 1698),
        ],
        vec![line("OFFERS ACCEPTED (2/10) HISTORY", 1146, 214)],
    ] {
        assert_eq!(screen_state(&find_anchors(&lines)), ScreenState::Unknown);
    }
    // A label far from the Reward card (e.g. in the Details text) is ignored…
    let far = vec![
        line("Reward", 2598, 401),
        line("Contract Deadline", 1300, 1200),
    ];
    assert_eq!(screen_state(&find_anchors(&far)), ScreenState::Unknown);
    // …but without a Reward anchor, the label anywhere counts.
    assert_eq!(
        screen_state(&find_anchors(&[line("Contract Deadline", 1300, 1200)])),
        ScreenState::Accepted
    );
    let none = vec![line("Hello there", 0, 0)];
    assert!(!find_anchors(&none).any());
}

#[test]
fn reward_card_labels_tolerate_ocr_noise() {
    use cargo_line_lib::ocr::layout::{card_label_state, edit_distance};
    let state = |t: &str| card_label_state(&line(t, 0, 0));
    for accepted in [
        "Contract Deadline",
        "Contract Deadiine",
        "Contrac Deadline",
        "Coutract Deadlme",
        "ContractDeadline",
        "|Contract Deadline.",
    ] {
        assert_eq!(state(accepted), Some(ScreenState::Accepted), "{accepted}");
    }
    for offer in [
        "Contract Availability",
        "Contract Availabllity",
        "Contract Avai1ability",
        "Contract Availabiity",
        "ContractAvailability",
    ] {
        assert_eq!(state(offer), Some(ScreenState::Offer), "{offer}");
    }
    for neither in [
        "Contracted By",
        "Deadline",
        "Availability",
        "Contract",
        "Reward",
        "ACCEPTED (0/10)",
        "Contract Details",
    ] {
        assert_eq!(state(neither), None, "{neither}");
    }
    assert_eq!(edit_distance("DEADIINE", "DEADLINE"), 1);
    assert_eq!(edit_distance("DEADLME", "DEADLINE"), 2);
    assert_eq!(edit_distance("", "ABC"), 3);
}

#[test]
fn regions_follow_the_anchors_without_buttons() {
    let lines = vec![
        line("Reward", 2500, 300),             // 100 × 20
        line("DETAILS", 1300, 600),            // 100 × 20
        line("PRIMARY OBJECTIVES", 2400, 600), // 220 × 20
    ];
    let regions = plan_regions(&find_anchors(&lines), 3840, 2160);
    let get = |k| regions.iter().find(|r| r.kind == k).unwrap().clone();
    let obj = get(RegionKind::Objectives);
    assert_eq!(obj.source, RegionSource::Anchor);
    assert_eq!((obj.bbox.x, obj.bbox.y, obj.bbox.width), (2380, 594, 515)); // header - line, 2.25 widths + line
    assert_eq!(
        obj.bbox.y + obj.bbox.height,
        1593,
        "panel bottom: header + 0.46 × frame height"
    );
    let details = get(RegionKind::Details);
    assert_eq!(details.source, RegionSource::Anchor);
    assert_eq!((details.bbox.x, details.bbox.y), (1280, 594));
    assert_eq!(
        details.bbox.x + details.bbox.width,
        2380,
        "up to the objectives column"
    );
    assert_eq!(details.bbox.y + details.bbox.height, 1593);
    let reward = get(RegionKind::Reward);
    assert_eq!(
        (
            reward.bbox.x,
            reward.bbox.y,
            reward.bbox.width,
            reward.bbox.height
        ),
        (2600, 288, 950, 44)
    );
    let title = get(RegionKind::Title);
    assert!(
        title.bbox.x + title.bbox.width <= 2500,
        "the title sits left of the info card"
    );
    assert_eq!(title.bbox.y, 310);
    // Button lines change nothing.
    let mut with_buttons = lines.clone();
    with_buttons.push(line("ABANDON", 1300, 1700));
    with_buttons.push(line("ACCEPT OFFER", 3300, 1700));
    assert_eq!(
        plan_regions(&find_anchors(&with_buttons), 3840, 2160),
        regions
    );
    assert!(
        obj.bbox.y + obj.bbox.height < 1700,
        "the objectives stop above the button row"
    );
}

#[test]
fn regions_fall_back_to_fixed_fractions_and_clamp() {
    let regions = plan_regions(&find_anchors(&[]), 3840, 2160);
    assert_eq!(regions.len(), 4);
    assert!(regions.iter().all(|r| r.source == RegionSource::Fallback));
    let obj = regions
        .iter()
        .find(|r| r.kind == RegionKind::Objectives)
        .unwrap();
    assert_eq!((obj.bbox.x, obj.bbox.y), (2419, 648));
    // Only the objectives header: Details placed from it, title and reward from the fallback.
    let only_header = vec![line("PRIMARY OBJECTIVES", 2400, 600)];
    let regions = plan_regions(&find_anchors(&only_header), 3840, 2160);
    let get = |k| regions.iter().find(|r| r.kind == k).unwrap().clone();
    assert_eq!(get(RegionKind::Objectives).source, RegionSource::Anchor);
    assert_eq!(get(RegionKind::Details).source, RegionSource::Anchor);
    assert_eq!(
        get(RegionKind::Details).bbox.x,
        1247,
        "0.295 × width left of the header, minus a line"
    );
    assert_eq!(get(RegionKind::Reward).source, RegionSource::Fallback);
    // Anchors near the edge are clamped inside the frame.
    let edge = vec![
        line("Reward", 3800, 2150),
        line("PRIMARY OBJECTIVES", 3700, 2100),
    ];
    for r in plan_regions(&find_anchors(&edge), 3840, 2160) {
        assert!(
            r.bbox.x >= 0
                && r.bbox.y >= 0
                && r.bbox.x + r.bbox.width <= 3840
                && r.bbox.y + r.bbox.height <= 2160
        );
        assert!(r.bbox.width >= 8 && r.bbox.height >= 8);
    }
    assert!(looks_like_objectives(&[line("o Deliver 0/84 SCU", 0, 0)]));
    assert!(!looks_like_objectives(&[line("Hello there", 0, 0)]));
}

/* ---------------------------------------------------------------- image */

#[test]
fn image_operations() {
    // 4×2 RGB gradient.
    let data: Vec<u8> = (0..8u8).flat_map(|i| [i * 30, 100, 200]).collect();
    let img = Image::rgb(4, 2, data);
    let crop = img.crop(OcrBox {
        x: 1,
        y: 0,
        width: 2,
        height: 2,
    });
    assert_eq!((crop.width, crop.height), (2, 2));
    assert_eq!(&crop.data[..3], &[30, 100, 200]);
    // Out-of-range crops are clipped.
    assert_eq!(
        img.crop(OcrBox {
            x: 3,
            y: 1,
            width: 10,
            height: 10
        })
        .width,
        1
    );
    let gray = img.gray();
    assert_eq!((gray.channels, gray.data[0]), (1, 82)); // 0.299*0 + 0.587*100 + 0.114*200
    let half = img.half();
    assert_eq!((half.width, half.height), (2, 1));
    assert_eq!(half.data[0], 75); // 2×2 average of red: (0 + 30 + 120 + 150) / 4
    let double = gray.double();
    assert_eq!((double.width, double.height), (8, 4));
    // A flat image stays flat (weights sum to 1).
    let flat = Image {
        width: 3,
        height: 3,
        channels: 1,
        data: vec![77; 9],
    }
    .double();
    assert!(flat.data.iter().all(|&v| v == 77));
    let mut pnm = Vec::new();
    gray.write_pnm(&mut pnm).unwrap();
    assert!(pnm.starts_with(b"P5\n4 2\n255\n"));
    assert_eq!(pnm.len(), "P5\n4 2\n255\n".len() + 8);
    // PNG round trip through the capture encoder.
    let frame = Frame {
        width: 2,
        height: 1,
        bgra: vec![1, 2, 3, 255, 4, 5, 6, 255],
        source: CaptureSourceKind::Window,
        backend: CaptureBackend::Wgc,
    };
    let mut png = Vec::new();
    storage::encode_png(&frame, &mut png).unwrap();
    let decoded = Image::decode_png(std::io::Cursor::new(png)).unwrap();
    assert_eq!(decoded.data, vec![3, 2, 1, 6, 5, 4]);
    assert!(Image::decode_png(std::io::Cursor::new(b"not a png".to_vec())).is_err());
}

/* ------------------------------------------------------- engine process */

fn fake_engine(dir: &TempDir, script: &str, timeout: Duration) -> Engine {
    let exe = dir.path().join("fake-tesseract.cmd");
    std::fs::write(&exe, format!("@echo off\r\n{script}\r\n")).unwrap();
    Engine {
        exe,
        tessdata: dir.path().join("tessdata"),
        temp_dir: dir.path().join("ocr-tmp"),
        timeout,
    }
}

fn temp_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

fn tiny() -> Image {
    Image::rgb(2, 2, vec![0; 12])
}

#[test]
fn engine_returns_tsv_and_deletes_its_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let engine = fake_engine(
        &dir,
        "echo 5\t1\t1\t1\t1\t1\t0\t0\t10\t10\t95\tHello",
        Duration::from_secs(10),
    );
    let tsv = engine
        .recognize_tsv(&tiny(), 6, &CancelToken::default())
        .unwrap();
    let lines = parse_tsv(&tsv, Mapping::IDENTITY);
    assert_eq!(lines[0].text, "Hello");
    assert_eq!(
        temp_files(&engine.temp_dir),
        0,
        "temp file deleted after success"
    );
}

#[test]
fn engine_errors_delete_the_temp_file_too() {
    let dir = tempfile::tempdir().unwrap();
    let engine = fake_engine(&dir, "echo boom 1>&2\r\nexit /b 3", Duration::from_secs(10));
    match engine.recognize_tsv(&tiny(), 6, &CancelToken::default()) {
        Err(OcrError::Failed(message)) => assert!(message.contains("boom"), "{message}"),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(temp_files(&engine.temp_dir), 0);
}

#[test]
fn a_slow_engine_times_out_and_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("finished.txt");
    // ~3 s of work, then a marker that must never appear once the process is killed.
    let engine = fake_engine(
        &dir,
        &format!(
            "ping -n 4 127.0.0.1 >nul\r\necho done> \"{}\"",
            marker.display()
        ),
        Duration::from_millis(400),
    );
    let started = Instant::now();
    assert_eq!(
        engine
            .recognize_tsv(&tiny(), 6, &CancelToken::default())
            .unwrap_err(),
        OcrError::Timeout
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "returned at the timeout, not when the child ended"
    );
    assert_eq!(
        temp_files(&engine.temp_dir),
        0,
        "temp file deleted after a timeout"
    );
    std::thread::sleep(Duration::from_secs(4));
    assert!(!marker.exists(), "the timed-out process was killed");
}

#[test]
fn cancelling_stops_a_running_engine() {
    let dir = tempfile::tempdir().unwrap();
    let engine = fake_engine(&dir, "ping -n 6 127.0.0.1 >nul", Duration::from_secs(30));
    let token = CancelToken::default();
    let canceller = {
        let token = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            token.cancel();
        })
    };
    let started = Instant::now();
    assert_eq!(
        engine.recognize_tsv(&tiny(), 6, &token).unwrap_err(),
        OcrError::Cancelled
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    canceller.join().unwrap();
    assert_eq!(temp_files(&engine.temp_dir), 0);
    // An already-cancelled token never starts the engine.
    assert_eq!(
        engine.recognize_tsv(&tiny(), 6, &token).unwrap_err(),
        OcrError::Cancelled
    );
    assert!(token.same_as(&token.clone()) && !token.same_as(&CancelToken::default()));
}

#[test]
fn engine_missing_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::in_dir(dir.path(), dir.path().join("tmp"), Duration::from_secs(5));
    assert!(matches!(
        engine.check_files(),
        Err(OcrError::EngineMissing(_))
    ));
    assert!(matches!(engine.version(), Err(OcrError::EngineMissing(_))));
    // The executable exists but the English data does not.
    std::fs::write(dir.path().join("tesseract.exe"), b"").unwrap();
    match engine.check_files() {
        Err(OcrError::EngineMissing(m)) => assert!(m.contains("eng.traineddata"), "{m}"),
        other => panic!("{other:?}"),
    }
    // Spawning a program that does not exist.
    let missing = Command::new(dir.path().join("nope.exe"));
    assert!(matches!(
        run_child(missing, Duration::from_secs(1), &CancelToken::default()),
        Err(OcrError::EngineMissing(_))
    ));
    assert_eq!(
        OcrError::EngineMissing("x".into()).to_string(),
        "The OCR engine is not installed with this copy of Cargo Line (x)"
    );
}

#[test]
fn temp_files_are_removed_when_dropped_and_leftovers_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf;
    {
        let file = TempFile::create(dir.path(), &tiny()).unwrap();
        path = file.path().to_path_buf();
        assert!(path.is_file());
        assert!(path.extension().unwrap() == "ppm");
    }
    assert!(!path.exists());
    std::fs::write(dir.path().join("ocr-1-2.pgm"), b"x").unwrap();
    std::fs::write(dir.path().join("keep.txt"), b"x").unwrap();
    cargo_line_lib::ocr::engine::clear_temp_dir(dir.path());
    assert!(!dir.path().join("ocr-1-2.pgm").exists());
    assert!(dir.path().join("keep.txt").exists());
}

/* ---------------------------------------------------- processing status */

fn db_with_capture() -> (TempDir, Database, i64) {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(&tmp.path().join(DATABASE_FILE_NAME)).unwrap();
    let frame = Frame {
        width: 1,
        height: 1,
        bgra: vec![0, 0, 0, 255],
        source: CaptureSourceKind::Window,
        backend: CaptureBackend::Wgc,
    };
    let id = {
        let conn = db.connection().unwrap();
        store(
            &conn,
            &tmp.path().join(CAPTURES_DIR_NAME),
            &frame,
            1_795_600_000_000,
        )
        .unwrap()
        .id
    };
    (tmp, db, id)
}

#[test]
fn processing_status_transitions() {
    let (_tmp, db, id) = db_with_capture();
    let conn = db.connection().unwrap();
    assert_eq!(
        processing_status(&conn, id).unwrap(),
        Some(ProcessingStatus::Pending)
    );
    // Finishing without starting changes nothing.
    assert!(!finish_processing(&conn, id, ProcessingStatus::Done).unwrap());
    assert!(begin_processing(&conn, id).unwrap());
    assert_eq!(
        processing_status(&conn, id).unwrap(),
        Some(ProcessingStatus::Processing)
    );
    assert!(finish_processing(&conn, id, ProcessingStatus::Done).unwrap());
    assert_eq!(
        processing_status(&conn, id).unwrap(),
        Some(ProcessingStatus::Done)
    );
    // Retry after done or failed.
    assert!(begin_processing(&conn, id).unwrap());
    assert!(finish_processing(&conn, id, ProcessingStatus::Failed).unwrap());
    assert_eq!(
        processing_status(&conn, id).unwrap(),
        Some(ProcessingStatus::Failed)
    );
    // Cancelled → back to pending.
    assert!(begin_processing(&conn, id).unwrap());
    assert!(finish_processing(&conn, id, ProcessingStatus::Pending).unwrap());
    assert_eq!(
        processing_status(&conn, id).unwrap(),
        Some(ProcessingStatus::Pending)
    );
    // "Processing" is not an outcome; unknown captures change nothing.
    assert!(begin_processing(&conn, id).unwrap());
    assert!(!finish_processing(&conn, id, ProcessingStatus::Processing).unwrap());
    assert!(!begin_processing(&conn, 9999).unwrap());
    // A crash leaves "processing": startup marks it failed.
    assert_eq!(fail_interrupted(&conn).unwrap(), 1);
    assert_eq!(
        processing_status(&conn, id).unwrap(),
        Some(ProcessingStatus::Failed)
    );
    // The CHECK refuses anything else.
    assert!(conn
        .execute(
            "UPDATE contract_captures SET processing_status = 'recognized' WHERE id = ?1",
            [id]
        )
        .is_err());
}

#[test]
fn migration_0005_rebuilds_captures_with_the_new_states() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(DATABASE_FILE_NAME);
    {
        let mut conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 4).unwrap();
        conn.execute_batch(
            "INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes) VALUES (1, '2026-11-18', 0, 60);
             INSERT INTO contracts (run_id, title, reward_auec, accepted_at, source, created_at, updated_at)
               VALUES (1, 'Old', 5, 0, 'manual', 0, 0);
             INSERT INTO contract_captures (id, contract_id, file_path, captured_at, expires_at, capture_method,
                                            processing_status, capture_source, capture_backend, width, height) VALUES
               (1, 1, 'C:\\c\\a.png', 0, 1000, 'manual', 'recognized', 'window', 'wgc', 3840, 2160),
               (2, NULL, 'C:\\c\\b.png', 0, 1000, 'auto_capture', 'partial', NULL, NULL, NULL, NULL),
               (3, NULL, 'C:\\c\\c.png', 0, 1000, 'manual', 'failed', 'display', 'wgc', 10, 10),
               (4, NULL, 'C:\\c\\d.png', 0, 1000, 'manual', 'pending', NULL, NULL, NULL, NULL);",
        )
        .unwrap();
    }
    let db = Database::open(&path).unwrap();
    let conn = db.connection().unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(
        version, LATEST_SCHEMA_VERSION,
        "0005 and every later migration applied"
    );
    let states: Vec<(i64, String)> = conn
        .prepare("SELECT id, processing_status FROM contract_captures ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        states,
        [
            (1, "done".into()),
            (2, "done".into()),
            (3, "failed".into()),
            (4, "pending".into())
        ]
    );
    let first = storage::get(&conn, 1).unwrap().unwrap();
    assert_eq!(
        (first.contract_id, first.width, first.source),
        (Some(1), Some(3840), Some(CaptureSourceKind::Window))
    );
    // Indexes and the foreign key survive the rebuild.
    let indexes: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND tbl_name = 'contract_captures' AND name LIKE 'contract_captures_%_idx'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(indexes, 2);
    let fk: (String, String) = conn
        .query_row(
            "SELECT \"table\", on_delete FROM pragma_foreign_key_list('contract_captures')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        fk,
        ("contracts".into(), "SET NULL".into()),
        "foreign key kept"
    );
    assert!(conn
        .execute("INSERT INTO contract_captures (file_path, captured_at, expires_at, capture_method, processing_status) VALUES ('x', 0, 1, 'manual', 'partial')", [])
        .is_err());
}

/* ------------------------------------------------ live-session findings */

#[test]
fn verbatim_paths_are_made_plain_for_tesseract() {
    use cargo_line_lib::ocr::engine::plain_path;
    assert_eq!(
        plain_path(Path::new(r"\\?\C:\app\tesseract")),
        PathBuf::from(r"C:\app\tesseract")
    );
    assert_eq!(
        plain_path(Path::new(r"C:\app\tesseract")),
        PathBuf::from(r"C:\app\tesseract")
    );
    assert_eq!(
        plain_path(Path::new(r"\\?\UNC\server\share\x")),
        PathBuf::from(r"\\?\UNC\server\share\x")
    );
    let engine = Engine::in_dir(
        Path::new(r"\\?\C:\res\tesseract"),
        PathBuf::from(r"\\?\C:\tmp"),
        Duration::from_secs(1),
    );
    assert_eq!(engine.tessdata, PathBuf::from(r"C:\res\tesseract\tessdata"));
    assert_eq!(engine.temp_dir, PathBuf::from(r"C:\tmp"));
}
