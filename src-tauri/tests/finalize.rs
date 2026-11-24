//! Headless tests for completing and abandoning contracts. Temporary databases only.

use cargo_line_lib::contracts::finalize::{
    finalize_contract, finalize_contracts, validate_bulk_request, BulkRequestError, FinalStatus,
    FinalizeError,
};
use cargo_line_lib::contracts::repository::{
    create_contract, list_active_contracts, load_contract,
};
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::models::{ContractSource, ContractStatus};
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use rusqlite::Connection;
use tempfile::TempDir;

const DAY_MS: i64 = 86_400_000;
/// 2026-11-19 23:50 local (UTC+1) as UTC ms, close to midnight on purpose.
const LATE_EVENING: i64 = 1_795_647_000_000;

fn fresh() -> (TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    (dir, db)
}

fn stop(name: &str, qty: Option<&str>) -> NewStopInput {
    NewStopInput {
        location_name: name.into(),
        quantity_scu: qty.map(Into::into),
    }
}

/// Two requirements, multiple stops and a box group, so "everything else unchanged" is meaningful.
fn add(conn: &mut Connection, title: &str, at: i64, date: &str) -> ContractDetail {
    let input = NewContractInput {
        title: title.into(),
        reward_auec: "24250".into(),
        requirements: vec![
            NewRequirementInput {
                commodity_name: "Aluminum".into(),
                quantity_scu: "9".into(),
                max_box_size_scu: Some("4".into()),
                pickups: vec![stop("Port Tressler", None)],
                deliveries: vec![
                    stop("Sakura Sun Goldenrod Workcenter", Some("5")),
                    stop("Area18", Some("4")),
                ],
            },
            NewRequirementInput {
                commodity_name: "Waste".into(),
                quantity_scu: "4.5".into(),
                max_box_size_scu: None,
                pickups: vec![stop("HUR-L1", None), stop("HUR-L2", None)],
                deliveries: vec![stop("Port Tressler", None)],
            },
        ],
    };
    let accepted = AcceptanceTime {
        utc_ms: at,
        local_date: date.into(),
        utc_offset_minutes: 60,
    };
    let saved = create_contract(
        conn,
        &validate_new_contract(&input).unwrap(),
        &accepted,
        ContractSource::Manual,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cargo_box_groups (requirement_id, box_size_scu_milli, box_count, source_quality) VALUES (?1, 4000, 2, 'exact')",
        [saved.requirements[0].id],
    )
    .unwrap();
    load_contract(conn, saved.id).unwrap().unwrap()
}

fn row_counts(conn: &Connection) -> Vec<i64> {
    [
        "runs",
        "contracts",
        "cargo_requirements",
        "cargo_stops",
        "cargo_box_groups",
    ]
    .iter()
    .map(|t| {
        conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect()
}

/// Everything except status / finished time.
fn unchanged_parts(
    c: &ContractDetail,
) -> (
    i64,
    RunSummary,
    String,
    i64,
    i64,
    i64,
    Vec<RequirementDetail>,
) {
    (
        c.id,
        c.run.clone(),
        c.title.clone(),
        c.reward_auec,
        c.accepted_at,
        c.total_scu_milli,
        c.requirements.clone(),
    )
}

#[test]
fn completing_sets_status_and_finish_time_and_keeps_everything_else() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let before = add(&mut conn, "Haul", LATE_EVENING, "2026-11-19");
    let counts = row_counts(&conn);

    let done = finalize_contract(
        &mut conn,
        before.id,
        FinalStatus::Completed,
        LATE_EVENING + 60_000,
    )
    .unwrap();
    assert_eq!(done.status, ContractStatus::Completed);
    assert_eq!(done.finished_at, Some(LATE_EVENING + 60_000));
    assert_eq!(unchanged_parts(&done), unchanged_parts(&before));
    assert_eq!(row_counts(&conn), counts, "nothing deleted or added");
    let updated_at: i64 = conn
        .query_row(
            "SELECT updated_at FROM contracts WHERE id = ?1",
            [before.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(updated_at, LATE_EVENING + 60_000);
}

#[test]
fn abandoning_sets_status_and_finish_time_and_keeps_everything_else() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let before = add(&mut conn, "Haul", LATE_EVENING, "2026-11-19");
    let counts = row_counts(&conn);
    let gone = finalize_contract(
        &mut conn,
        before.id,
        FinalStatus::Abandoned,
        LATE_EVENING + 1,
    )
    .unwrap();
    assert_eq!(gone.status, ContractStatus::Abandoned);
    assert_eq!(gone.finished_at, Some(LATE_EVENING + 1));
    assert_eq!(unchanged_parts(&gone), unchanged_parts(&before));
    assert_eq!(row_counts(&conn), counts);
}

#[test]
fn finishing_after_midnight_keeps_the_original_run() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let before = add(&mut conn, "Late haul", LATE_EVENING, "2026-11-19");
    // Finished "the next day" (injected timestamp), and a new contract on that day.
    let next_day = LATE_EVENING + DAY_MS / 2;
    let done = finalize_contract(&mut conn, before.id, FinalStatus::Completed, next_day).unwrap();
    let tomorrow = add(&mut conn, "Next day", next_day, "2026-11-20");
    assert_eq!(done.run, before.run, "run unchanged");
    assert_eq!(done.run.run_date, "2026-11-19");
    assert_ne!(tomorrow.run.id, before.run.id);
}

#[test]
fn finish_time_is_never_before_acceptance() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let c = add(&mut conn, "Clock skew", LATE_EVENING, "2026-11-19");
    let done = finalize_contract(
        &mut conn,
        c.id,
        FinalStatus::Completed,
        LATE_EVENING - 5_000,
    )
    .unwrap();
    assert_eq!(
        done.finished_at,
        Some(LATE_EVENING),
        "clamped to accepted_at"
    );
}

#[test]
fn finalising_twice_is_not_active_and_unknown_id_is_not_found() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let c = add(&mut conn, "Once", LATE_EVENING, "2026-11-19");
    finalize_contract(&mut conn, c.id, FinalStatus::Completed, LATE_EVENING + 1).unwrap();
    for status in [FinalStatus::Completed, FinalStatus::Abandoned] {
        match finalize_contract(&mut conn, c.id, status, LATE_EVENING + 2) {
            Err(FinalizeError::NotActive { id, status }) => {
                assert_eq!((id, status), (c.id, "completed"))
            }
            other => panic!("expected NotActive, got {other:?}"),
        }
    }
    // The second attempt changed nothing.
    let finished_at: i64 = conn
        .query_row(
            "SELECT finished_at FROM contracts WHERE id = ?1",
            [c.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(finished_at, LATE_EVENING + 1);
    assert!(matches!(
        finalize_contract(&mut conn, 9_999, FinalStatus::Completed, 0),
        Err(FinalizeError::NotFound(9_999))
    ));
}

#[test]
fn bulk_finalises_exactly_the_given_ids() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let a = add(&mut conn, "A", LATE_EVENING, "2026-11-19");
    let b = add(&mut conn, "B", LATE_EVENING + 1, "2026-11-19");
    let later = add(
        &mut conn,
        "Added after the dialog opened",
        LATE_EVENING + 2,
        "2026-11-19",
    );
    let done = finalize_contracts(
        &mut conn,
        &[a.id, b.id],
        FinalStatus::Completed,
        LATE_EVENING + 10,
    )
    .unwrap();
    assert_eq!(
        done.iter().map(|c| (c.id, c.status)).collect::<Vec<_>>(),
        [
            (a.id, ContractStatus::Completed),
            (b.id, ContractStatus::Completed)
        ]
    );
    let active: Vec<i64> = list_active_contracts(&conn)
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(
        active,
        [later.id],
        "a contract not in the list is never finalised"
    );
}

#[test]
fn bulk_rolls_back_completely_when_one_id_is_stale() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let a = add(&mut conn, "A", LATE_EVENING, "2026-11-19");
    let b = add(&mut conn, "B", LATE_EVENING + 1, "2026-11-19");
    finalize_contract(&mut conn, b.id, FinalStatus::Abandoned, LATE_EVENING + 5).unwrap(); // finished elsewhere
    match finalize_contracts(
        &mut conn,
        &[a.id, b.id, 777],
        FinalStatus::Completed,
        LATE_EVENING + 10,
    ) {
        Err(FinalizeError::Conflict {
            not_found,
            not_active,
        }) => {
            assert_eq!(not_found, [777]);
            assert_eq!(not_active, [b.id]);
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
    let a_now = load_contract(&conn, a.id).unwrap().unwrap();
    assert_eq!(
        (a_now.status, a_now.finished_at),
        (ContractStatus::Active, None),
        "nothing changed"
    );
    let b_now = load_contract(&conn, b.id).unwrap().unwrap();
    assert_eq!(
        b_now.status,
        ContractStatus::Abandoned,
        "b keeps its earlier status"
    );
}

#[test]
fn bulk_request_validation_rejects_empty_duplicates_and_invalid_status() {
    assert_eq!(
        validate_bulk_request(&[], "completed"),
        Err(BulkRequestError::Empty)
    );
    assert_eq!(
        validate_bulk_request(&[1, 2, 1], "abandoned"),
        Err(BulkRequestError::Duplicates)
    );
    for bad in ["active", "Completed", "deleted", ""] {
        assert_eq!(
            validate_bulk_request(&[1], bad),
            Err(BulkRequestError::InvalidStatus),
            "{bad:?}"
        );
    }
    assert_eq!(
        validate_bulk_request(&[1, 2], "completed"),
        Ok(FinalStatus::Completed)
    );
    assert_eq!(
        validate_bulk_request(&[3], "abandoned"),
        Ok(FinalStatus::Abandoned)
    );
    assert_eq!(FinalStatus::parse("active"), None);
    assert_eq!(BulkRequestError::Empty.issue().0, "ids");
    assert_eq!(BulkRequestError::InvalidStatus.issue().0, "status");
}

#[test]
fn active_list_excludes_finished_contracts_and_nothing_is_deleted() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let a = add(&mut conn, "A", LATE_EVENING, "2026-11-19");
    let b = add(&mut conn, "B", LATE_EVENING + 1, "2026-11-19");
    let c = add(&mut conn, "C", LATE_EVENING + 2, "2026-11-19");
    let counts = row_counts(&conn);
    finalize_contract(&mut conn, a.id, FinalStatus::Completed, LATE_EVENING + 10).unwrap();
    finalize_contract(&mut conn, b.id, FinalStatus::Abandoned, LATE_EVENING + 11).unwrap();
    let active: Vec<i64> = list_active_contracts(&conn)
        .unwrap()
        .into_iter()
        .map(|x| x.id)
        .collect();
    assert_eq!(active, [c.id]);
    assert_eq!(
        row_counts(&conn),
        counts,
        "finished contracts and their cargo stay in the database"
    );
    let statuses: Vec<String> = conn
        .prepare("SELECT status FROM contracts ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(statuses, ["completed", "abandoned", "active"]);
}

#[test]
fn finished_contract_serializes_finished_at() {
    let (_dir, db) = fresh();
    let mut conn = db.connection().unwrap();
    let c = add(&mut conn, "A", LATE_EVENING, "2026-11-19");
    assert!(serde_json::to_value(&c).unwrap()["finishedAt"].is_null());
    let done =
        finalize_contract(&mut conn, c.id, FinalStatus::Completed, LATE_EVENING + 3).unwrap();
    let json = serde_json::to_value(&done).unwrap();
    assert_eq!(json["status"], "completed");
    assert_eq!(json["finishedAt"], LATE_EVENING + 3);
}
