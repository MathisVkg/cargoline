//! Seeds a data folder with realistic, synthetic data for the startup measurements (Task 18).
//!
//! ```text
//! cargo run --release --example perf_seed -- <empty data folder>
//! ```
//!
//! It writes:
//! - 30 active contracts with 2–4 objectives each;
//! - 500 finished contracts across 40 runs;
//! - 20 linked screenshots with files;
//! - a custom ship, favourites and a selected ship;
//! - Game.log monitoring on, pointing at `<folder>/test-log/Game.log` (a small synthetic log).
//!
//! Then start Cargo Line with `CARGO_LINE_DATA_DIR=<folder>`. There is no personal data:
//! titles are numbered and names come from the bundled catalogs.

use std::path::PathBuf;
use std::time::Instant;

use cargo_line_lib::capture::storage::{self, CAPTURES_DIR_NAME};
use cargo_line_lib::capture::{CaptureBackend, CaptureSourceKind, Frame};
use cargo_line_lib::contracts::finalize::{finalize_contracts, FinalStatus};
use cargo_line_lib::contracts::repository::{create_contract_linked, SaveLinks};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::models::ContractSource;
use cargo_line_lib::database::{now_utc_ms, Database, DATABASE_FILE_NAME};
use cargo_line_lib::{reference, settings, ships};

const DAY: i64 = 86_400_000;
/// The test log lives in its own folder (the setting requires the file name `Game.log`).
const LOG_PATH: [&str; 2] = ["test-log", "Game.log"];

fn main() {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: perf_seed <data folder>"),
    );
    if dir.join(DATABASE_FILE_NAME).exists() {
        panic!(
            "{} already has a database; use an empty folder",
            dir.display()
        );
    }
    let started = Instant::now();
    let db = Database::open(&dir.join(DATABASE_FILE_NAME)).expect("open");
    let mut conn = db.connection().expect("connection");
    ships::sync_catalog(&mut conn, ships::bundled().unwrap(), now_utc_ms()).expect("catalog");

    let locations: Vec<String> = reference::locations()
        .unwrap()
        .items
        .iter()
        .map(|l| l.name.clone())
        .collect();
    let commodities: Vec<String> = reference::commodities()
        .unwrap()
        .items
        .iter()
        .map(|c| c.name.clone())
        .collect();
    // A small deterministic generator (no new dependency).
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };

    let now = now_utc_ms();
    let today = chrono::Local::now().date_naive();
    let mut make =
        |conn: &mut rusqlite::Connection, n: usize, day_offset: i64, objectives: usize| {
            let requirements = (0..objectives)
                .map(|_| NewRequirementInput {
                    commodity_name: commodities[next(commodities.len())].clone(),
                    quantity_scu: (8 * (1 + next(24))).to_string(),
                    max_box_size_scu: None,
                    pickups: vec![NewStopInput {
                        location_name: locations[next(locations.len())].clone(),
                        quantity_scu: None,
                    }],
                    deliveries: vec![NewStopInput {
                        location_name: locations[next(locations.len())].clone(),
                        quantity_scu: None,
                    }],
                })
                .collect();
            let input = NewContractInput {
                title: format!("Perf contract {n:03}"),
                reward_auec: (20_000 + 1_000 * next(200)).to_string(),
                requirements,
            };
            let valid = validate_new_contract(&input).expect("valid");
            let date = today - chrono::Duration::days(day_offset);
            let at = AcceptanceTime {
                utc_ms: now - day_offset * DAY - (next(3_600) as i64) * 1000,
                local_date: date.format("%Y-%m-%d").to_string(),
                utc_offset_minutes: 60,
            };
            create_contract_linked(
                conn,
                &valid,
                &at,
                ContractSource::Manual,
                &SaveLinks::default(),
            )
            .expect("create")
            .id
        };

    // History: 500 contracts over 40 days (one run per local date), most completed.
    for n in 0..500 {
        let day = 1 + (n % 40) as i64;
        let id = make(&mut conn, n, day, 1 + n % 3);
        let status = if n % 9 == 0 {
            FinalStatus::Abandoned
        } else {
            FinalStatus::Completed
        };
        finalize_contracts(&mut conn, &[id], status, now - (day - 1) * DAY - 600_000)
            .expect("finalize");
    }
    // Active: 30 contracts today with 2–4 objectives.
    let active: Vec<i64> = (0..30)
        .map(|n| make(&mut conn, 500 + n, 0, 2 + n % 3))
        .collect();

    // 20 screenshots with files, linked to active contracts.
    let captures = dir.join(CAPTURES_DIR_NAME);
    let frame = Frame {
        width: 640,
        height: 360,
        bgra: vec![40; 640 * 360 * 4],
        source: CaptureSourceKind::Window,
        backend: CaptureBackend::Wgc,
    };
    for (k, contract) in active.iter().take(20).enumerate() {
        let info = storage::store(&conn, &captures, &frame, now - k as i64 * 1000).expect("store");
        storage::link(&conn, info.id, *contract).expect("link");
    }

    // A custom ship (selected) and two favourites.
    let custom = ships::create_custom_ship(
        &mut conn,
        &ships::CustomShipInput {
            name: "Perf Hauler".into(),
            cargo_scu: 696,
        },
    )
    .expect("custom ship");
    for ship in ships::bundled().unwrap().items.iter().take(2) {
        ships::set_favorite(&conn, &ship.id, true, now).expect("favourite");
    }
    ships::set_favorite(&conn, &custom.id, true, now).expect("favourite");
    ships::set_selected(&mut conn, Some(&custom.id)).expect("select");

    // Game.log monitoring on, with a synthetic log that only has unrelated lines.
    let log = dir.join(LOG_PATH[0]).join(LOG_PATH[1]);
    std::fs::create_dir_all(log.parent().unwrap()).expect("log folder");
    let mut text = String::new();
    for k in 0..2_000 {
        text.push_str(&format!(
            "<2026-11-26T10:{:02}:{:02}.000Z> [Notice] <Perf> synthetic line {k} [Team_Perf][Test]\n",
            (k / 60) % 60,
            k % 60
        ));
    }
    std::fs::write(&log, text).expect("log");
    settings::update(&mut conn, |s| {
        s.game_log_monitoring_enabled = true;
        s.game_log_path = Some(log.to_string_lossy().into_owned());
    })
    .expect("settings");

    drop(conn);
    println!(
        "seeded {}: 30 active, 500 finished in 40 runs, 20 captures, custom ship; {} ms",
        dir.display(),
        started.elapsed().as_millis()
    );
}
