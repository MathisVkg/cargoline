//! SQL operations for contracts and their cargo.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::models::*;
use crate::database::models::{CargoOperation, ContractSource, ContractStatus};

/// Saves a validated contract with all of its cargo in one transaction,
/// assigning it to the run of its local acceptance date (creating that run
/// if needed). Nothing — not even a new run — is kept if any step fails.
pub fn create_contract(
    conn: &mut Connection,
    contract: &ValidContract,
    accepted: &AcceptanceTime,
    source: ContractSource,
) -> rusqlite::Result<ContractDetail> {
    create_contract_with_capture(conn, contract, accepted, source, None)
}

/// Same as [`create_contract`], and links the form's unsaved screenshot (if any) to the
/// new contract in the same transaction. A capture that is gone (expired, already
/// linked) is ignored: the contract never depends on its screenshot.
pub fn create_contract_with_capture(
    conn: &mut Connection,
    contract: &ValidContract,
    accepted: &AcceptanceTime,
    source: ContractSource,
    capture_id: Option<i64>,
) -> rusqlite::Result<ContractDetail> {
    create_contract_linked(
        conn,
        contract,
        accepted,
        source,
        &SaveLinks {
            capture_id,
            ..SaveLinks::default()
        },
    )
}

/// What a new contract is linked to besides its cargo (all optional).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveLinks {
    /// The form's unsaved screenshot, linked in the same transaction.
    pub capture_id: Option<i64>,
    /// The in-game MissionId (Game.log), stored in `contracts.game_mission_id`.
    pub game_mission_id: Option<String>,
    /// The capture was triggered by a Game.log accept: `capture_method = auto_capture`.
    pub auto_capture: bool,
}

/// The contract (any status) that already carries this in-game MissionId.
pub fn contract_for_mission(conn: &Connection, mission_id: &str) -> rusqlite::Result<Option<i64>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT id FROM contracts WHERE game_mission_id = ?1 ORDER BY id LIMIT 1",
        [mission_id],
        |r| r.get(0),
    )
    .optional()
}

/// Creates the contract and applies [`SaveLinks`] in one transaction.
pub fn create_contract_linked(
    conn: &mut Connection,
    contract: &ValidContract,
    accepted: &AcceptanceTime,
    source: ContractSource,
    links: &SaveLinks,
) -> rusqlite::Result<ContractDetail> {
    let capture_id = links.capture_id;
    // IMMEDIATE takes the write lock up front, so concurrent creations on the
    // same date cannot both decide to create the run.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run_id = run_for_date(&tx, accepted)?;

    tx.prepare_cached(
        "INSERT INTO contracts (run_id, title, reward_auec, status, accepted_at, source, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?5)",
    )?
    .execute(params![
        run_id,
        contract.title,
        contract.reward_auec,
        ContractStatus::Active,
        accepted.utc_ms,
        source
    ])?;
    let contract_id = tx.last_insert_rowid();

    insert_cargo(&tx, contract_id, &contract.requirements, accepted.utc_ms)?;
    if let Some(capture_id) = capture_id {
        let linked = crate::capture::storage::link(&tx, capture_id, contract_id)?;
        if linked && links.auto_capture {
            tx.execute(
                "UPDATE contract_captures SET capture_method = 'auto_capture' WHERE id = ?1",
                [capture_id],
            )?;
        }
    }
    if let Some(mission_id) = &links.game_mission_id {
        tx.execute(
            "UPDATE contracts SET game_mission_id = ?2 WHERE id = ?1",
            params![contract_id, mission_id],
        )?;
    }

    let detail = load_contracts(&tx, Some(contract_id))?
        .pop()
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    tx.commit()?;
    Ok(detail)
}

/// One contract (any status) with nested cargo.
pub fn load_contract(conn: &Connection, id: i64) -> rusqlite::Result<Option<ContractDetail>> {
    Ok(load_contracts(conn, Some(id))?.pop())
}

/// All active contracts, newest first, with nested cargo.
pub fn list_active_contracts(conn: &Connection) -> rusqlite::Result<Vec<ContractDetail>> {
    load_contracts(conn, None)
}

/// Finds the run of the local date or creates it with the next run number. Numbers continue
/// from the highest ever assigned (`run_sequence`), so a cleared history never reuses one.
fn run_for_date(conn: &Connection, accepted: &AcceptanceTime) -> rusqlite::Result<i64> {
    let existing = conn
        .prepare_cached("SELECT id FROM runs WHERE run_date = ?1")?
        .query_row([&accepted.local_date], |r| r.get(0))
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    conn.prepare_cached(
        "INSERT INTO runs (run_number, run_date, created_at, utc_offset_minutes)
         VALUES ((SELECT last_run_number + 1 FROM run_sequence WHERE id = 1), ?1, ?2, ?3)",
    )?
    .execute(params![
        accepted.local_date,
        accepted.utc_ms,
        accepted.utc_offset_minutes
    ])?;
    Ok(conn.last_insert_rowid())
}

/// Loads one contract (`Some(id)`) or all active contracts (`None`).
fn load_contracts(conn: &Connection, only: Option<i64>) -> rusqlite::Result<Vec<ContractDetail>> {
    // ?1 IS NULL selects all active contracts; otherwise exactly one contract.
    load_contracts_where(
        conn,
        "(?1 IS NULL AND c.status = 'active') OR c.id = ?1",
        only,
    )
}

/// Every completed or abandoned contract with nested cargo, in the same four queries.
pub fn list_finished_contracts(conn: &Connection) -> rusqlite::Result<Vec<ContractDetail>> {
    load_contracts_where(
        conn,
        "?1 IS NULL AND c.status IN ('completed', 'abandoned')",
        None,
    )
}

/// `filter` is a condition on `c` (contracts) with one optional parameter `?1`.
fn load_contracts_where(
    conn: &Connection,
    filter: &str,
    only: Option<i64>,
) -> rusqlite::Result<Vec<ContractDetail>> {
    let mut contracts: Vec<ContractDetail> = conn
        .prepare_cached(&format!(
            "SELECT c.id, r.id, r.run_number, r.run_date, c.title, c.reward_auec, c.status, c.source, c.accepted_at, c.finished_at
             FROM contracts c JOIN runs r ON r.id = c.run_id
             WHERE {filter}
             ORDER BY c.accepted_at DESC, c.id DESC"
        ))?
        .query_map([only], |row| {
            Ok(ContractDetail {
                id: row.get(0)?,
                run: RunSummary { id: row.get(1)?, run_number: row.get(2)?, run_date: row.get(3)? },
                title: row.get(4)?,
                reward_auec: row.get(5)?,
                status: row.get(6)?,
                source: row.get(7)?,
                accepted_at: row.get(8)?,
                finished_at: row.get(9)?,
                total_scu_milli: 0,
                requirements: Vec::new(),
            })
        })?
        .collect::<Result<_, _>>()?;

    let index: HashMap<i64, usize> = contracts
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id, i))
        .collect();

    let mut requirement_owner: HashMap<i64, (usize, usize)> = HashMap::new();
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT q.id, q.contract_id, q.commodity_name, q.quantity_scu_milli, q.max_box_size_scu_milli
         FROM cargo_requirements q JOIN contracts c ON c.id = q.contract_id
         WHERE {filter}
         ORDER BY q.contract_id, q.id"
    ))?;
    let mut rows = stmt.query([only])?;
    while let Some(row) = rows.next()? {
        let contract_id: i64 = row.get(1)?;
        let Some(&ci) = index.get(&contract_id) else {
            continue;
        };
        let requirement = RequirementDetail {
            id: row.get(0)?,
            commodity_name: row.get(2)?,
            quantity_scu_milli: row.get(3)?,
            max_box_size_scu_milli: row.get(4)?,
            box_groups: Vec::new(),
            pickups: Vec::new(),
            deliveries: Vec::new(),
        };
        let contract = &mut contracts[ci];
        // Totals come from requirements only, never from stop allocations.
        contract.total_scu_milli += requirement.quantity_scu_milli;
        requirement_owner.insert(requirement.id, (ci, contract.requirements.len()));
        contract.requirements.push(requirement);
    }

    let mut stmt = conn.prepare_cached(&format!(
        "SELECT s.id, s.requirement_id, s.operation, s.location_name, s.quantity_scu_milli, s.reminder_done_at
         FROM cargo_stops s
         JOIN cargo_requirements q ON q.id = s.requirement_id
         JOIN contracts c ON c.id = q.contract_id
         WHERE {filter}
         ORDER BY s.requirement_id, s.operation, s.sort_order"
    ))?;
    let mut rows = stmt.query([only])?;
    while let Some(row) = rows.next()? {
        let requirement_id: i64 = row.get(1)?;
        let Some(&(ci, ri)) = requirement_owner.get(&requirement_id) else {
            continue;
        };
        let operation: CargoOperation = row.get(2)?;
        let stop = StopDetail {
            id: row.get(0)?,
            location_name: row.get(3)?,
            quantity_scu_milli: row.get(4)?,
            reminder_done_at: row.get(5)?,
        };
        let requirement = &mut contracts[ci].requirements[ri];
        match operation {
            CargoOperation::Pickup => requirement.pickups.push(stop),
            CargoOperation::Delivery => requirement.deliveries.push(stop),
        }
    }

    let mut stmt = conn.prepare_cached(&format!(
        "SELECT b.requirement_id, b.box_size_scu_milli, b.box_count, b.source_quality
         FROM cargo_box_groups b
         JOIN cargo_requirements q ON q.id = b.requirement_id
         JOIN contracts c ON c.id = q.contract_id
         WHERE {filter}
         ORDER BY b.requirement_id, b.box_size_scu_milli DESC"
    ))?;
    let mut rows = stmt.query([only])?;
    while let Some(row) = rows.next()? {
        let requirement_id: i64 = row.get(0)?;
        let Some(&(ci, ri)) = requirement_owner.get(&requirement_id) else {
            continue;
        };
        contracts[ci].requirements[ri]
            .box_groups
            .push(BoxGroupDetail {
                box_size_scu_milli: row.get(1)?,
                box_count: row.get(2)?,
                source_quality: row.get(3)?,
            });
    }
    Ok(contracts)
}

/** Newly inserted cargo rows, with the keys used to restore kept data on edit. */
pub(crate) struct InsertedRequirement {
    pub id: i64,
    pub commodity_key: String,
    pub quantity_scu_milli: i64,
    pub max_box_size_scu_milli: Option<i64>,
    /// (stop id, operation, normalized location key)
    pub stops: Vec<(i64, CargoOperation, String)>,
}

/// Inserts the requirements and stops of a validated contract. Locations go
/// through the catalog (reused or added) inside the caller's transaction.
pub(crate) fn insert_cargo(
    tx: &Connection,
    contract_id: i64,
    requirements: &[ValidRequirement],
    now_ms: i64,
) -> rusqlite::Result<Vec<InsertedRequirement>> {
    let mut insert_requirement = tx.prepare_cached(
        "INSERT INTO cargo_requirements (contract_id, commodity_name, quantity_scu_milli, max_box_size_scu_milli, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut insert_stop = tx.prepare_cached(
        "INSERT INTO cargo_stops (requirement_id, operation, location_name, quantity_scu_milli, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut inserted = Vec::with_capacity(requirements.len());
    for req in requirements {
        insert_requirement.execute(params![
            contract_id,
            req.commodity_name,
            req.quantity_scu_milli,
            req.max_box_size_scu_milli,
            now_ms
        ])?;
        let requirement_id = tx.last_insert_rowid();
        let mut stops = Vec::new();
        for (operation, list) in [
            (CargoOperation::Pickup, &req.pickups),
            (CargoOperation::Delivery, &req.deliveries),
        ] {
            for (order, stop) in list.iter().enumerate() {
                // Reuse the catalog spelling, or add the new name, in this
                // same transaction (rolled back if the save fails).
                let location_name =
                    crate::locations::resolve_or_insert(tx, &stop.location_name, now_ms)?;
                insert_stop.execute(params![
                    requirement_id,
                    operation,
                    location_name,
                    stop.quantity_scu_milli,
                    order as i64
                ])?;
                stops.push((
                    tx.last_insert_rowid(),
                    operation,
                    crate::locations::normalize_key(&location_name),
                ));
            }
        }
        inserted.push(InsertedRequirement {
            id: requirement_id,
            commodity_key: crate::locations::normalize_key(&req.commodity_name),
            quantity_scu_milli: req.quantity_scu_milli,
            max_box_size_scu_milli: req.max_box_size_scu_milli,
            stops,
        });
    }
    Ok(inserted)
}
