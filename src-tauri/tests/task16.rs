//! Task 16 tests: the `EndMission` pattern (abandon), the per-MissionId dedupe with `MissionEnded`,
//! interstellar objectives naming a system, and the hauling template filter for
//! `HaulCargo_MultiToSingle_Interstellar_*`. Synthetic lines only: fake MissionIds, a fake player
//! name and id. Temporary databases only.

use cargo_line_lib::contracts::repository::{create_contract_linked, SaveLinks};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::models::ContractSource;
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::gamelog::patterns::{
    completion_state, is_hauling_template, parse_first_objective, parse_line, LogLine,
    STATE_ABANDONED, STATE_COMPLETED,
};
use cargo_line_lib::gamelog::service::{is_system_name, to_app_events};
use cargo_line_lib::gamelog::tracker::{TrackedEvent, Tracker};
use cargo_line_lib::gamelog::{CompleteFromGame, GameLogEvent};

const M1: &str = "6a1b2c3d-1111-4aaa-8bbb-0123456789ab";
const M2: &str = "6a1b2c3d-2222-4aaa-8bbb-0123456789ab";
const PLAYER: &str = "Synthetic_Pilot_42";
const PLAYER_ID: &str = "987654321";

fn end_mission(id: &str, kind: &str, reason: &str) -> String {
    format!(
        "<2026-11-26T09:30:00.000Z> [Notice] <EndMission> Ending mission for player. MissionId[{id}] Player[{PLAYER}] PlayerId[{PLAYER_ID}] CompletionType[{kind}] Reason[{reason}] [Team_MissionFeatures][Missions]"
    )
}
fn player_left(id: &str) -> String {
    format!(
        "<2026-11-26T09:30:00.000Z> [Notice] <PlayerLeft> Received PlayerLeft push message for: mission_id {id} - player_id {PLAYER_ID} [Team_GameServices][Missions]"
    )
}
fn mission_ended(id: &str, state: &str) -> String {
    format!(
        "<2026-11-26T09:29:59.000Z> [Notice] <MissionEnded> Received MissionEnded push message for: mission_id {id} - mission_state {state} [Team_GameServices][Missions]"
    )
}
fn accepted(id: &str, title: &str) -> String {
    format!(
        "<2026-11-26T09:00:00.003Z> [Notice] <SHUDEvent_OnNotification> Added notification \"Contract Accepted:  {title}: \" [33] to queue. New queue size: 1, MissionId: [{id}], ObjectiveId: [] [Team_CoreGameplayFeatures][Missions][Comms]"
    )
}
fn marker(id: &str, generator: &str, template: &str) -> String {
    format!(
        "<2026-11-26T09:00:00.000Z> [Notice] <CLocalMissionPhaseMarker::CreateMarker> Creating objective marker: missionId [{id}], generator name [{generator}], contract [{template}], contractDefinitionId[00000000-0000-0000-0000-000000000000], objectiveId [pickup_1], markerEntityId [1], zoneHostId [2], position [x: 1, y: 2, z: 3] [Team_MissionFeatures][Missions]"
    )
}
fn objective(id: &str, text: &str) -> String {
    format!(
        "<2026-11-26T09:00:00.004Z> [Notice] <SHUDEvent_OnNotification> Added notification \"New Objective: {text}: \" [34] to queue. New queue size: 2, MissionId: [{id}], ObjectiveId: [x] [Team_CoreGameplayFeatures][Missions][Comms]"
    )
}

fn feed_all(lines: &[String]) -> Vec<TrackedEvent> {
    let mut tracker = Tracker::default();
    for line in lines {
        if let Some(parsed) = parse_line(line) {
            tracker.feed(parsed);
        }
    }
    tracker.flush()
}

/* ------------------------------------------------------------ EndMission */

#[test]
fn end_mission_abandon_is_an_abandoned_end_without_personal_data() {
    let parsed = parse_line(&end_mission(&M1.to_uppercase(), "Abandon", "Player left")).unwrap();
    assert_eq!(
        parsed,
        LogLine::Ended {
            mission_id: M1.into(),
            state: STATE_ABANDONED.into(),
            at: Some("2026-11-26T09:30:00.000Z".into()),
        }
    );
    // Only the MissionId, the completion type and the timestamp are kept.
    let debug = format!("{parsed:?}");
    assert!(
        !debug.contains(PLAYER) && !debug.contains(PLAYER_ID),
        "{debug}"
    );
    // The PlayerLeft line that precedes it carries no end state and is ignored.
    assert_eq!(parse_line(&player_left(M1)), None);
}

#[test]
fn completion_types() {
    assert_eq!(completion_state("Complete"), STATE_COMPLETED);
    assert_eq!(completion_state("Abandon"), STATE_ABANDONED);
    assert_eq!(
        completion_state("Fail"),
        "Fail",
        "unknown types pass through by name"
    );
    let fail = parse_line(&end_mission(M1, "Fail", "Mission failed")).unwrap();
    assert!(matches!(fail, LogLine::Ended { ref state, .. } if state == "Fail"));
    // Malformed: no CompletionType, an invalid MissionId, an empty type.
    let no_type = end_mission(M1, "Abandon", "x").replace("CompletionType[Abandon] ", "");
    assert_eq!(parse_line(&no_type), None);
    assert_eq!(
        parse_line(&end_mission("not-a-mission-id", "Abandon", "x")),
        None
    );
    assert_eq!(parse_line(&end_mission(M1, "", "x")), None);
}

#[test]
fn one_end_per_mission_when_both_lines_arrive() {
    // Completion: MissionEnded first, EndMission (Complete) on the next line (seen in the Task 06
    // log for all 4 completions). One event, from the first line.
    let events = feed_all(&[
        mission_ended(M1, STATE_COMPLETED),
        end_mission(M1, "Complete", "Mission Ended"),
    ]);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], TrackedEvent::Ended { state, .. } if state == STATE_COMPLETED));
    // If the order were reversed, the completion is still the completed state.
    let reversed = feed_all(&[
        end_mission(M1, "Complete", "Mission Ended"),
        mission_ended(M1, STATE_COMPLETED),
    ]);
    assert_eq!(reversed.len(), 1);
    assert!(matches!(&reversed[0], TrackedEvent::Ended { state, .. } if state == STATE_COMPLETED));
    // Abandon: only PlayerLeft + EndMission.
    let abandon = feed_all(&[player_left(M2), end_mission(M2, "Abandon", "Player left")]);
    assert_eq!(abandon.len(), 1);
    assert!(
        matches!(&abandon[0], TrackedEvent::Ended { mission_id, state, .. } if mission_id == M2 && state == STATE_ABANDONED)
    );
}

fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    (dir, db)
}

#[test]
fn an_abandon_reaches_the_ended_flow_for_the_active_contract() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    let valid = validate_new_contract(&NewContractInput {
        title: "Synthetic Haul".into(),
        reward_auec: "1000".into(),
        requirements: vec![NewRequirementInput {
            commodity_name: "Aluminum".into(),
            quantity_scu: "10".into(),
            max_box_size_scu: None,
            pickups: vec![NewStopInput {
                location_name: "A".into(),
                quantity_scu: None,
            }],
            deliveries: vec![NewStopInput {
                location_name: "B".into(),
                quantity_scu: None,
            }],
        }],
    })
    .unwrap();
    // Accepted shortly before the synthetic end lines (09:30Z on 2026-11-26).
    let at = AcceptanceTime {
        utc_ms: 1_795_683_600_000,
        local_date: "2026-11-26".into(),
        utc_offset_minutes: 60,
    };
    let links = SaveLinks {
        capture_id: None,
        game_mission_id: Some(M2.into()),
        auto_capture: false,
    };
    let saved =
        create_contract_linked(&mut conn, &valid, &at, ContractSource::Manual, &links).unwrap();

    // Even with automatic completion on, the event carries the abandoned state: the frontend only
    // ever completes on MISSION_STATE_COMPLETED and shows the Abandon notice for this one.
    let events = to_app_events(
        &conn,
        feed_all(&[player_left(M2), end_mission(M2, "Abandon", "Player left")]),
        false,
        CompleteFromGame::Automatic,
    );
    assert_eq!(events.len(), 1);
    match &events[0] {
        GameLogEvent::Ended {
            mission_id,
            state,
            contract_id,
            ..
        } => {
            assert_eq!(
                (mission_id.as_str(), state.as_str(), *contract_id),
                (M2, STATE_ABANDONED, saved.id)
            );
        }
        other => panic!("{other:?}"),
    }
}

/* ------------------------------------------------------------ interstellar */

#[test]
fn interstellar_objective_names_a_system() {
    assert_eq!(
        parse_first_objective("Deliver 0/2 SCU of Souvenirs to Pyro System"),
        Some((2, "Souvenirs".into(), "Pyro System".into()))
    );
    for system in [
        "Pyro System",
        "Stanton System",
        "Nyx system",
        "Pyro",
        "Stanton",
        "Nyx",
    ] {
        assert!(is_system_name(system), "{system}");
    }
    for place in [
        "Shepherd's Rest",
        "Pyro Gateway",
        "Everus Harbor",
        "Ruin Station",
        "System",
    ] {
        assert!(!is_system_name(place), "{place}");
    }
}

#[test]
fn interstellar_template_is_hauling_and_the_first_objective_is_paired() {
    let template = "HaulCargo_MultiToSingle_Interstellar_Souvenirs_Pyro";
    assert!(is_hauling_template(template));
    let (_dir, db) = db();
    let conn = db.connection().unwrap();
    // Four New Objective lines (one per Deliver), all naming the system: the first one is paired.
    let events = feed_all(&[
        marker(M1, "LingFamilyHauling_Hauling", template),
        accepted(M1, "Ling Family Rookie Cargo Haul - Small Scale"),
        objective(M1, "Deliver 0/2 SCU of Souvenirs to Pyro System"),
        objective(M1, "Deliver 0/1 SCU of Distilled Spirits to Pyro System"),
        objective(M1, "Deliver 0/4 SCU of Processed Food to Pyro System"),
        objective(M1, "Deliver 0/2 SCU of Stims to Pyro System"),
    ]);
    let app = to_app_events(&conn, events, true, CompleteFromGame::Off);
    assert_eq!(app.len(), 1);
    match &app[0] {
        GameLogEvent::Accepted {
            mission_id,
            objective: Some(o),
            ..
        } => {
            assert_eq!(mission_id, M1);
            assert_eq!(
                (
                    o.scu,
                    o.commodity.as_str(),
                    o.location.as_str(),
                    o.location_is_system
                ),
                (2, "Souvenirs", "Pyro System", true)
            );
        }
        other => panic!("{other:?}"),
    }
    // A normal destination is still resolved and compared.
    let events = feed_all(&[
        marker(M2, "Covalex_Hauling", "HaulCargo_AToB_Aluminum"),
        accepted(M2, "Junior Rank - Direct Medium Cargo Haul"),
        objective(M2, "Deliver 0/84 SCU of Aluminum to Baijini Point"),
    ]);
    match &to_app_events(&conn, events, true, CompleteFromGame::Off)[0] {
        GameLogEvent::Accepted {
            objective: Some(o), ..
        } => assert!(!o.location_is_system && o.location == "Baijini Point"),
        other => panic!("{other:?}"),
    }
}
