//! Times the backend startup steps on a seeded data folder, without a window (Task 18).
//!
//! ```text
//! cargo run --release --example perf_seed -- <folder>
//! cargo run --release --example perf_bench -- <folder> [runs]
//! ```
//!
//! Each run opens the database the way the app does, then times:
//! - the ship catalog sync;
//! - the startup capture cleanup;
//! - the first contracts list, the settings, the ship list and the history.
//!
//! The capture cleanup only touches rows and files that the app would clean anyway.

use std::path::PathBuf;
use std::time::Instant;

use cargo_line_lib::capture::storage::{self, CAPTURES_DIR_NAME};
use cargo_line_lib::contracts::repository;
use cargo_line_lib::database::{now_utc_ms, Database, DATABASE_FILE_NAME};
use cargo_line_lib::{history, settings, ships};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: perf_bench <data folder> [runs]"));
    let runs: usize = args.next().map(|r| r.parse().expect("runs")).unwrap_or(5);
    let names = [
        "open + migrations",
        "catalog sync (always)",
        "catalog check (Task 18)",
        "capture cleanup",
        "first contracts list",
        "settings",
        "ship list",
        "history",
    ];
    let mut totals = vec![0.0; names.len()];
    for run in 0..runs {
        let mut times = Vec::new();
        let t = Instant::now();
        let db = Database::open(&dir.join(DATABASE_FILE_NAME)).expect("open");
        times.push(ms(t));
        let mut conn = db.connection().expect("connection");

        let t = Instant::now();
        let report =
            ships::sync_catalog(&mut conn, ships::bundled().unwrap(), now_utc_ms()).expect("sync");
        times.push(ms(t));

        let t = Instant::now();
        let skipped =
            ships::sync_catalog_if_changed(&mut conn, ships::bundled().unwrap(), now_utc_ms())
                .expect("check")
                .is_none();
        times.push(ms(t));

        let t = Instant::now();
        storage::cleanup(&conn, &dir.join(CAPTURES_DIR_NAME), now_utc_ms(), true);
        times.push(ms(t));

        let t = Instant::now();
        let active = repository::list_active_contracts(&conn).expect("list");
        times.push(ms(t));

        let t = Instant::now();
        settings::load(&conn).expect("settings");
        times.push(ms(t));

        let t = Instant::now();
        let ship_list = ships::list_ships(&conn).expect("ships");
        times.push(ms(t));

        let t = Instant::now();
        history::get_history(&conn).expect("history");
        times.push(ms(t));

        println!(
            "run {}: {}  (catalog: {} rows written, check skipped: {}; {} active, {} ships)",
            run + 1,
            times
                .iter()
                .map(|v| format!("{v:.1}"))
                .collect::<Vec<_>>()
                .join(" / "),
            report.upserted,
            skipped,
            active.len(),
            ship_list.len()
        );
        for (total, v) in totals.iter_mut().zip(&times) {
            *total += v;
        }
    }
    // The one CPU-heavy path: encoding a 4K screenshot (the release profile must keep it fast).
    let frame = cargo_line_lib::capture::Frame {
        width: 3840,
        height: 2160,
        bgra: (0..3840u32 * 2160 * 4).map(|i| (i % 251) as u8).collect(),
        source: cargo_line_lib::capture::CaptureSourceKind::Window,
        backend: cargo_line_lib::capture::CaptureBackend::Wgc,
    };
    let mut png_ms = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let mut out = Vec::new();
        storage::encode_png(&frame, &mut out).expect("png");
        png_ms.push(ms(t));
    }
    println!("4K PNG encode (ms): {png_ms:.0?}");
    println!("average over {runs} runs (ms):");
    for (name, total) in names.iter().zip(&totals) {
        println!("  {name:<22} {:>7.1}", total / runs as f64);
    }
}
