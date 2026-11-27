//! Game.log monitoring tests: line patterns, the tracker, the polling tail (temporary
//! files), a real game log (local fixture, skipped when absent), settings rules, and saving
//! with a MissionId. No desktop; temporary databases only.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use cargo_line_lib::contracts::repository::{
    contract_for_mission, create_contract_linked, SaveLinks,
};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::models::ContractSource;
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::gamelog::patterns::{
    is_hauling_template, parse_first_objective, parse_line, LogLine,
};
use cargo_line_lib::gamelog::service::{active_contract_for, mission_known, to_app_events};
use cargo_line_lib::gamelog::tail::{Tail, TailError};
use cargo_line_lib::gamelog::tracker::{RawObjective, TrackedEvent, Tracker};
use cargo_line_lib::gamelog::CompleteFromGame;
use cargo_line_lib::gamelog::GameLogEvent;
use cargo_line_lib::settings::models::validate_shortcut;
use cargo_line_lib::settings::AppSettings;

const M1: &str = "6a1b2c3d-1111-4aaa-8bbb-0123456789ab";
const M2: &str = "6a1b2c3d-2222-4aaa-8bbb-0123456789ab";
const M3: &str = "6a1b2c3d-3333-4aaa-8bbb-0123456789ab";

fn accepted(id: &str, title: &str) -> String {
    format!(
        "<2026-11-19T14:01:02.003Z> [Notice] <SHUDEvent_OnNotification> Added notification \"Contract Accepted:  {title}: \" [33] to queue. New queue size: 1, MissionId: [{id}], ObjectiveId: [] [Team_CoreGameplayFeatures][Missions][Comms]"
    )
}
fn echo(title: &str) -> String {
    format!("<2026-11-19T14:01:02.003Z>    \"Contract Accepted:  {title}: \" [33]")
}
fn marker(id: &str, template: &str) -> String {
    format!(
        "<2026-11-19T14:01:02.000Z> [Notice] <CLocalMissionPhaseMarker::CreateMarker> Creating objective marker: missionId [{id}], generator name [Covalex_Hauling], contract [{template}], contractDefinitionId[00000000-0000-0000-0000-000000000000], objectiveId [pickup_1], markerEntityId [1], zoneHostId [2], position [x: 1, y: 2, z: 3] [Team_MissionFeatures][Missions]"
    )
}
fn objective(id: &str, text: &str) -> String {
    format!(
        "<2026-11-19T14:01:02.004Z> [Notice] <SHUDEvent_OnNotification> Added notification \"New Objective: {text}: \" [34] to queue. New queue size: 2, MissionId: [{id}], ObjectiveId: [x] [Team_CoreGameplayFeatures][Missions][Comms]"
    )
}
fn ended(id: &str, state: &str) -> String {
    format!(
        "<2026-11-19T15:00:00.000Z> [Notice] <MissionEnded> Received MissionEnded push message for: mission_id {id} - mission_state {state} [Team_GameServices][Missions]"
    )
}

/* ------------------------------------------------------------- patterns */

#[test]
fn line_patterns() {
    assert_eq!(
        parse_line(&accepted(M1, "Junior Rank - Direct Medium Cargo Haul")),
        Some(LogLine::Accepted {
            mission_id: M1.into(),
            title: "Junior Rank - Direct Medium Cargo Haul".into(),
            at: Some("2026-11-19T14:01:02.003Z".into())
        })
    );
    // Titles with a colon, and the " : " ending.
    let colon = accepted(
        M1,
        "Claim #25215: Crusader C2 Hercules Starlifter Salvage Rights",
    )
    .replace(": \"", " : \"");
    match parse_line(&colon) {
        Some(LogLine::Accepted { title, .. }) => assert_eq!(
            title,
            "Claim #25215: Crusader C2 Hercules Starlifter Salvage Rights"
        ),
        other => panic!("{other:?}"),
    }
    // The echoes carry no MissionId and never count; an empty or malformed id doesn't either.
    assert_eq!(parse_line(&echo("Junior Rank")), None);
    assert_eq!(parse_line(&accepted("", "Junior Rank")), None);
    assert_eq!(parse_line(&accepted("not-a-uuid", "Junior Rank")), None);
    assert_eq!(
        parse_line(&marker(
            M1,
            "HaulCargo_AToB_RefinedOre_Aluminium_Stanton_SupplyGrade1"
        )),
        Some(LogLine::Marker {
            mission_id: M1.into(),
            template: "HaulCargo_AToB_RefinedOre_Aluminium_Stanton_SupplyGrade1".into()
        })
    );
    assert_eq!(
        parse_line(&objective(
            M1,
            "Deliver 0/84 SCU of Aluminum to Baijini Point"
        )),
        Some(LogLine::Objective {
            mission_id: M1.into(),
            text: "Deliver 0/84 SCU of Aluminum to Baijini Point".into()
        })
    );
    assert_eq!(
        parse_line(&ended(M1, "MISSION_STATE_COMPLETED")),
        Some(LogLine::Ended {
            mission_id: M1.into(),
            state: "MISSION_STATE_COMPLETED".into(),
            at: Some("2026-11-19T15:00:00.000Z".into())
        })
    );
    match parse_line(&ended(M1, "MISSION_STATE_SOMETHING_NEW")) {
        Some(LogLine::Ended { state, .. }) => assert_eq!(state, "MISSION_STATE_SOMETHING_NEW"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        parse_line("<2026-11-19T14:01:02.003Z> [Notice] <Something> unrelated"),
        None
    );
    assert_eq!(parse_line(""), None);
    assert!(is_hauling_template(
        "HaulCargo_SingleToMulti2_RefinedOre_Aluminium_Stanton_SupplyGrade1"
    ));
    assert!(!is_hauling_template(
        "Adaigo_Stanton1_Lawful_Salvage_Hard_1"
    ));
    assert!(!is_hauling_template("HaulCargoX_Whatever"));
    assert_eq!(
        parse_first_objective("Deliver 12/51 SCU of Agricultural Supplies to Everus Harbor"),
        Some((51, "Agricultural Supplies".into(), "Everus Harbor".into()))
    );
    assert_eq!(parse_first_objective("Locate Salvage Claim #25215"), None);
}

/* -------------------------------------------------------------- tracker */

fn feed(tracker: &mut Tracker, lines: &[String]) {
    for line in lines {
        if let Some(parsed) = parse_line(line) {
            tracker.feed(parsed);
        }
    }
}

#[test]
fn tracker_filters_hauling_dedupes_and_pairs_the_first_objective() {
    let mut t = Tracker::default();
    feed(
        &mut t,
        &[
            marker(
                M1,
                "HaulCargo_AToB_RefinedOre_Aluminium_Stanton_SupplyGrade1",
            ),
            accepted(M1, "Junior Rank - Direct Medium Cargo Haul"),
            echo("Junior Rank - Direct Medium Cargo Haul"),
            objective(M1, "Deliver 0/84 SCU of Aluminum to Baijini Point"),
            objective(M1, "Deliver 0/10 SCU of Aluminum to Somewhere Else"), // not the first
            marker(M2, "Adaigo_Stanton1_Lawful_Salvage_Hard_1"),
            accepted(
                M2,
                "Claim #25215: Crusader C2 Hercules Starlifter Salvage Rights",
            ),
            accepted(M1, "Junior Rank - Direct Medium Cargo Haul"), // duplicate
        ],
    );
    let events = t.flush();
    assert_eq!(
        events,
        [TrackedEvent::Accepted {
            mission_id: M1.into(),
            title: "Junior Rank - Direct Medium Cargo Haul".into(),
            at: Some("2026-11-19T14:01:02.003Z".into()),
            objective: Some(RawObjective {
                scu: 84,
                commodity: "Aluminum".into(),
                location: "Baijini Point".into()
            }),
        }],
        "salvage ignored, duplicate dropped, objective attached"
    );
    // An accept without a hauling marker (unknown kind) is ignored.
    feed(&mut t, &[accepted(M3, "Mystery")]);
    assert!(t.flush().is_empty());
    // The first objective arriving in a later batch is reported on its own.
    feed(
        &mut t,
        &[
            marker(M2, "x"),
            marker("6a1b2c3d-4444-4aaa-8bbb-0123456789ab", "HaulCargo_AToB_X"),
        ],
    );
    feed(
        &mut t,
        &[accepted(
            "6a1b2c3d-4444-4aaa-8bbb-0123456789ab",
            "Late objective",
        )],
    );
    assert_eq!(t.flush().len(), 1);
    feed(
        &mut t,
        &[objective(
            "6a1b2c3d-4444-4aaa-8bbb-0123456789ab",
            "Deliver 0/5 SCU of Tungsten to Port Tressler",
        )],
    );
    assert!(
        matches!(&t.flush()[..], [TrackedEvent::Objective { objective, .. }] if objective.scu == 5)
    );
    // Ends are reported once per mission, whatever the kind and state.
    feed(
        &mut t,
        &[
            ended(M2, "MISSION_STATE_COMPLETED"),
            ended(M2, "MISSION_STATE_COMPLETED"),
            ended(M1, "MISSION_STATE_FAILED"),
        ],
    );
    let ends = t.flush();
    assert_eq!(ends.len(), 2);
    // Mission ids already saved as contracts are never reported again.
    let mut known = Tracker::default();
    known.mark_known(M1);
    feed(
        &mut known,
        &[
            marker(M1, "HaulCargo_AToB_X"),
            accepted(M1, "Saved already"),
        ],
    );
    assert!(known.flush().is_empty());
}

/* ----------------------------------------------------------------- tail */

fn append(path: &Path, text: &str) {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

#[test]
fn tail_starts_at_the_end_and_follows_appends() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("Game.log");
    append(&log, "old line 1\r\nold line 2\r\n");
    let mut tail = Tail::new(&log);
    assert_eq!(
        tail.poll().unwrap(),
        Vec::<String>::new(),
        "old events are never replayed"
    );
    append(&log, "new 1\r\nnew 2\n");
    assert_eq!(
        tail.poll().unwrap(),
        ["new 1", "new 2"],
        "CRLF and LF mixed"
    );
    // A partial line waits for its newline.
    append(&log, "half");
    assert!(tail.poll().unwrap().is_empty());
    append(&log, " done\r\n");
    assert_eq!(tail.poll().unwrap(), ["half done"]);
    assert!(tail.poll().unwrap().is_empty());
    // Invalid UTF-8 is decoded lossily.
    let mut f = OpenOptions::new().append(true).open(&log).unwrap();
    f.write_all(b"bad \xff byte\n").unwrap();
    assert_eq!(tail.poll().unwrap(), ["bad \u{fffd} byte"]);
}

#[test]
fn tail_handles_rotation_missing_files_and_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("Game.log");
    append(&log, &"x".repeat(100));
    append(&log, "\n");
    let mut tail = Tail::new(&log);
    tail.poll().unwrap();
    // Truncated (rotation in place): read the new content from the start.
    fs::write(&log, "fresh 1\n").unwrap();
    assert_eq!(tail.poll().unwrap(), ["fresh 1"]);
    // Replaced: deleted, then a new file appears → read from its start.
    fs::remove_file(&log).unwrap();
    assert_eq!(tail.poll().unwrap_err(), TailError::NotFound);
    fs::write(&log, "new file 1\nnew file 2\n").unwrap();
    assert_eq!(tail.poll().unwrap(), ["new file 1", "new file 2"]);

    // Missing when monitoring starts, created later (the game started): read from the start.
    let later = dir.path().join("later").join("Game.log");
    let mut waiting = Tail::new(&later);
    assert_eq!(waiting.poll().unwrap_err(), TailError::NotFound);
    assert_eq!(waiting.poll().unwrap_err(), TailError::NotFound);
    fs::create_dir_all(later.parent().unwrap()).unwrap();
    fs::write(&later, "first\n").unwrap();
    assert_eq!(waiting.poll().unwrap(), ["first"]);

    // Stop and start (or a path change) = a new tail: it starts at the end again.
    append(&log, "before restart\n");
    let mut restarted = Tail::new(&log);
    assert!(restarted.poll().unwrap().is_empty());
    append(&log, "after restart\n");
    assert_eq!(restarted.poll().unwrap(), ["after restart"]);
    let mut other = Tail::new(&later);
    assert!(other.poll().unwrap().is_empty());
    append(&later, "other file\n");
    assert_eq!(other.poll().unwrap(), ["other file"]);
    // A folder is not a log.
    assert!(matches!(
        Tail::new(dir.path()).poll(),
        Err(TailError::Unreadable(_))
    ));
}

/* ------------------------------------------------- database and saving */

fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    (dir, db)
}

fn input() -> NewContractInput {
    NewContractInput {
        title: "Junior Rank - Direct Medium Cargo Haul".into(),
        reward_auec: "59500".into(),
        requirements: vec![NewRequirementInput {
            commodity_name: "Aluminum".into(),
            quantity_scu: "84".into(),
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

fn now() -> AcceptanceTime {
    AcceptanceTime {
        utc_ms: 1_795_600_000_000,
        local_date: "2026-11-25".into(),
        utc_offset_minutes: 60,
    }
}

#[test]
fn saving_from_a_game_log_event() {
    let (dir, db) = db();
    let mut conn = db.connection().unwrap();
    let frame = cargo_line_lib::capture::Frame {
        width: 1,
        height: 1,
        bgra: vec![0, 0, 0, 255],
        source: cargo_line_lib::capture::CaptureSourceKind::Window,
        backend: cargo_line_lib::capture::CaptureBackend::Wgc,
    };
    let capture = cargo_line_lib::capture::storage::store(
        &conn,
        &dir.path().join("captures"),
        &frame,
        1_795_600_000_000,
    )
    .unwrap();
    let valid = validate_new_contract(&input()).unwrap();
    let links = SaveLinks {
        capture_id: Some(capture.id),
        game_mission_id: Some(M1.into()),
        auto_capture: true,
    };
    let saved = create_contract_linked(
        &mut conn,
        &valid,
        &now(),
        ContractSource::AutoCapture,
        &links,
    )
    .unwrap();
    let (source, mission): (String, String) = conn
        .query_row(
            "SELECT source, game_mission_id FROM contracts WHERE id = ?1",
            [saved.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((source.as_str(), mission.as_str()), ("auto_capture", M1));
    let method: String = conn
        .query_row(
            "SELECT capture_method FROM contract_captures WHERE id = ?1",
            [capture.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(method, "auto_capture");
    assert_eq!(contract_for_mission(&conn, M1).unwrap(), Some(saved.id));
    assert!(mission_known(&conn, M1).unwrap());
    assert_eq!(active_contract_for(&conn, M1).unwrap(), Some(saved.id));
    assert_eq!(active_contract_for(&conn, M2).unwrap(), None);

    // App events: a saved mission is not reported again; ends only for active contracts.
    let events = to_app_events(
        &conn,
        vec![
            TrackedEvent::Accepted {
                mission_id: M1.into(),
                title: "again".into(),
                at: None,
                objective: None,
            },
            TrackedEvent::Accepted {
                mission_id: M2.into(),
                title: "new".into(),
                at: None,
                objective: Some(RawObjective {
                    scu: 5,
                    commodity: "aluminum".into(),
                    location: "Long Forest Station at microTech's L2 Lagrange point".into(),
                }),
            },
            TrackedEvent::Ended {
                mission_id: M1.into(),
                state: "MISSION_STATE_COMPLETED".into(),
                at: None,
            },
            TrackedEvent::Ended {
                mission_id: M3.into(),
                state: "MISSION_STATE_COMPLETED".into(),
                at: None,
            },
        ],
        true,
        CompleteFromGame::Off,
    );
    assert_eq!(events.len(), 2);
    match &events[0] {
        GameLogEvent::Accepted {
            mission_id,
            objective: Some(o),
            auto_capture,
            ..
        } => {
            assert_eq!(mission_id, M2);
            assert!(*auto_capture);
            assert_eq!(
                (o.scu, o.commodity.as_str(), o.location.as_str()),
                (5, "Aluminum", "MIC-L2 Long Forest Station")
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(&events[1], GameLogEvent::Ended { contract_id, .. } if *contract_id == saved.id)
    );

    // A manually added contract never gets ended notices (no MissionId).
    let manual = create_contract_linked(
        &mut conn,
        &valid,
        &now(),
        ContractSource::Manual,
        &SaveLinks::default(),
    )
    .unwrap();
    let mission: Option<String> = conn
        .query_row(
            "SELECT game_mission_id FROM contracts WHERE id = ?1",
            [manual.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mission, None);
}

/* --------------------------------------------------------------- settings */

#[test]
fn shortcut_and_game_log_settings_rules() {
    for ok in [
        "Ctrl+Shift+C",
        "Alt+K",
        "Ctrl+Alt+F5",
        "Ctrl+1",
        "Ctrl+Alt+Shift+Z",
    ] {
        assert_eq!(validate_shortcut(ok), Ok(()), "{ok}");
    }
    for bad in [
        "C",
        "F5",
        "Shift+C",
        "Ctrl+C",
        "Ctrl+V",
        "Ctrl+R",
        "Alt+F4",
        "Shift+Ctrl+C",
        "Ctrl+Ctrl+C",
        "Ctrl+Space",
        "Win+C",
        "Ctrl+",
        "",
    ] {
        assert!(validate_shortcut(bad).is_err(), "{bad}");
    }
    let mut s = AppSettings::default();
    assert_eq!(s.capture_shortcut, None, "no shortcut by default");
    assert!(
        !s.game_log_monitoring_enabled && !s.auto_capture_on_accept,
        "monitoring off by default"
    );
    s.game_log_path = Some(r"C:\Games\StarCitizen\LIVE\Game.log".into());
    assert!(s.validate().is_ok());
    s.game_log_path = Some(r"C:\Games\StarCitizen\LIVE\game.LOG".into());
    assert!(s.validate().is_ok(), "case-insensitive");
    s.game_log_path = Some(r"C:\Games\StarCitizen\LIVE\Game.log.bak".into());
    assert_eq!(s.validate().unwrap_err().field, "game_log_path");
    s.game_log_path = None;
    s.auto_capture_on_accept = true;
    assert_eq!(
        s.validate().unwrap_err().field,
        "auto_capture_on_accept",
        "needs monitoring"
    );
    s.game_log_monitoring_enabled = true;
    assert!(s.validate().is_ok());
    s.capture_shortcut = Some("Ctrl+C".into());
    assert_eq!(s.validate().unwrap_err().field, "capture_shortcut");
}

/* ------------------------------------------------ Detect */

#[cfg(windows)]
#[test]
fn image_path_without_a_process_handle() {
    let own = cargo_line_lib::capture::native::image_path_without_handle(std::process::id())
        .expect("own image path");
    let expected = std::env::current_exe().unwrap();
    assert!(
        own.eq_ignore_ascii_case(&expected.to_string_lossy()),
        "{own} vs {}",
        expected.display()
    );
    assert!(cargo_line_lib::capture::native::logical_drives().contains(&'C'));
    assert_eq!(
        cargo_line_lib::capture::native::image_path_without_handle(u32::MAX - 3),
        None
    );
}

#[test]
fn detect_rules() {
    use cargo_line_lib::gamelog::detect::*;
    use std::path::PathBuf;
    assert_eq!(
        game_log_for_exe(Path::new(
            r"C:\Program Files\Roberts Space Industries\StarCitizen\LIVE\Bin64\StarCitizen.exe"
        )),
        Some(PathBuf::from(
            r"C:\Program Files\Roberts Space Industries\StarCitizen\LIVE\Game.log"
        ))
    );
    assert_eq!(
        game_log_for_exe(Path::new(r"D:\SC\PTU\StarCitizen.exe")),
        Some(PathBuf::from(r"D:\SC\PTU\Game.log"))
    );
    assert_eq!(
        channel_of(Path::new(r"D:\RSI\StarCitizen\PTU\Game.log")),
        "PTU"
    );

    // Launcher log: JSON-escaped and plain paths, deduped, never a fragment without a drive.
    let log = r#"[info] "dir":"C:\Program Files\Roberts Space Industries\StarCitizen\LIVE" x
        [info] validate C:\Program Files\Roberts Space Industries\StarCitizen (type: install)
        [info] other E:\Games\Roberts Space Industries\StarCitizen\PTU
        [info] relative Roberts Space Industries\StarCitizen without drive"#;
    assert_eq!(
        roots_from_launcher_log(log),
        [
            PathBuf::from(r"C:\Program Files\Roberts Space Industries\StarCitizen"),
            PathBuf::from(r"E:\Games\Roberts Space Industries\StarCitizen")
        ]
    );
    assert!(roots_from_launcher_log("nothing here").is_empty());

    let roots = common_roots(Some(Path::new(r"C:\Program Files")), &['C', 'D']);
    assert_eq!(
        roots[0],
        PathBuf::from(r"C:\Program Files\Roberts Space Industries\StarCitizen")
    );
    assert!(roots.contains(&PathBuf::from(r"D:\Roberts Space Industries\StarCitizen")));
    assert!(roots.contains(&PathBuf::from(
        r"D:\Program Files\Roberts Space Industries\StarCitizen"
    )));

    // Found on disk: none, one, several (LIVE first, then the most recent).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("StarCitizen");
    assert!(logs_in_roots(std::slice::from_ref(&root), DetectMethod::Folders).is_empty());
    assert_eq!(
        detect_message(&[]),
        "Couldn't find Game.log — use Browse to select it"
    );
    for channel in ["PTU", "LIVE", "EPTU"] {
        fs::create_dir_all(root.join(channel)).unwrap();
        fs::write(root.join(channel).join("Game.log"), channel).unwrap();
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(root.join("EPTU").join("Game.log"), "newer").unwrap();
    let found = rank(logs_in_roots(
        std::slice::from_ref(&root),
        DetectMethod::Folders,
    ));
    assert_eq!(
        found.iter().map(|f| f.channel.as_str()).collect::<Vec<_>>(),
        ["LIVE", "EPTU", "PTU"]
    );
    assert_eq!(
        detect_message(&found),
        "Found 3 Game.log files — choose one"
    );
    assert_eq!(detect_message(&found[..1]), "Found Game.log (LIVE)");
    // The same file from two sources is listed once (the first source wins).
    let mut twice = logs_in_roots(std::slice::from_ref(&root), DetectMethod::Launcher);
    twice.extend(logs_in_roots(
        std::slice::from_ref(&root),
        DetectMethod::Folders,
    ));
    let twice = rank(twice);
    assert_eq!(twice.len(), 3);
    assert!(twice.iter().all(|f| f.method == DetectMethod::Launcher));
    let exe = root.join("LIVE").join("Bin64").join("StarCitizen.exe");
    assert_eq!(
        found_from_exe(&exe).map(|f| (f.channel, f.method)),
        Some(("LIVE".into(), DetectMethod::Process))
    );

    // Browse starts in the current path's folder, else the first existing channel folder.
    let current = root.join("PTU").join("Game.log");
    assert_eq!(
        browse_start(Some(&current.to_string_lossy()), &[]),
        Some(root.join("PTU"))
    );
    assert_eq!(
        browse_start(Some(r"Z:\gone\Game.log"), std::slice::from_ref(&root)),
        Some(root.join("LIVE"))
    );
    assert_eq!(browse_start(None, &[dir.path().join("missing")]), None);
}
