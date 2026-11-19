//! Editing active contracts and the optional Pick Up / Turn In reminders.
//!
//! Update strategy: *replace*. The contract's requirements are deleted
//! (cascading to their stops and box groups) and re-inserted through the
//! same code path as Add Contract, all inside one IMMEDIATE transaction.
//! Data the form does not edit is restored by key:
//! - a stop's reminder tick, when a new stop has the same
//!   (commodity, location, operation) as an old one;
//! - a requirement's box groups, when a new requirement has the same
//!   (commodity, total SCU, max box size) as an old one.
//!
//! The contract row itself (id, status, run, acceptance and creation times,
//! source, game mission id) is updated in place and keeps its identity.

use std::collections::{HashMap, VecDeque};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::finalize::FinalizeError;
use super::models::{ContractDetail, StopDetail, ValidContract};
use super::repository::{insert_cargo, load_contract};
use crate::database::models::{CargoOperation, ContractStatus};
use crate::locations::normalize_key;

type TickKey = (String, CargoOperation, String);
type BoxKey = (String, i64, Option<i64>);
type BoxRow = (i64, i64, String);

/// Replaces the editable parts of an active contract and returns it.
pub fn update_contract(
    conn: &mut Connection,
    id: i64,
    contract: &ValidContract,
    now_ms: i64,
) -> Result<ContractDetail, FinalizeError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let status: Option<ContractStatus> = tx
        .prepare_cached("SELECT status FROM contracts WHERE id = ?1")?
        .query_row([id], |r| r.get(0))
        .optional()?;
    match status {
        None => return Err(FinalizeError::NotFound(id)),
        Some(ContractStatus::Active) => {}
        Some(other) => {
            return Err(FinalizeError::NotActive {
                id,
                status: other.as_str(),
            })
        }
    }

    // 1. Remember ticks and box groups before the old rows go.
    let mut ticks: HashMap<TickKey, VecDeque<i64>> = HashMap::new();
    {
        let mut stmt = tx.prepare(
            "SELECT q.commodity_name, s.operation, s.location_name, s.reminder_done_at
             FROM cargo_stops s JOIN cargo_requirements q ON q.id = s.requirement_id
             WHERE q.contract_id = ?1 AND s.reminder_done_at IS NOT NULL
             ORDER BY q.id, s.operation, s.sort_order",
        )?;
        let mut rows = stmt.query([id])?;
        while let Some(row) = rows.next()? {
            let key = (
                normalize_key(&row.get::<_, String>(0)?),
                row.get(1)?,
                normalize_key(&row.get::<_, String>(2)?),
            );
            ticks.entry(key).or_default().push_back(row.get(3)?);
        }
    }
    let mut boxes: HashMap<BoxKey, VecDeque<Vec<BoxRow>>> = HashMap::new();
    {
        let mut stmt = tx.prepare(
            "SELECT q.id, q.commodity_name, q.quantity_scu_milli, q.max_box_size_scu_milli,
                    b.box_size_scu_milli, b.box_count, b.source_quality
             FROM cargo_requirements q JOIN cargo_box_groups b ON b.requirement_id = q.id
             WHERE q.contract_id = ?1
             ORDER BY q.id, b.box_size_scu_milli DESC",
        )?;
        let mut rows = stmt.query([id])?;
        let mut current: Option<(i64, BoxKey, Vec<BoxRow>)> = None;
        while let Some(row) = rows.next()? {
            let req_id: i64 = row.get(0)?;
            let entry: BoxRow = (row.get(4)?, row.get(5)?, row.get(6)?);
            match &mut current {
                Some((rid, _, list)) if *rid == req_id => list.push(entry),
                _ => {
                    if let Some((_, key, list)) = current.take() {
                        boxes.entry(key).or_default().push_back(list);
                    }
                    let key = (
                        normalize_key(&row.get::<_, String>(1)?),
                        row.get(2)?,
                        row.get(3)?,
                    );
                    current = Some((req_id, key, vec![entry]));
                }
            }
        }
        if let Some((_, key, list)) = current {
            boxes.entry(key).or_default().push_back(list);
        }
    }

    // 2. Update the contract row in place; updated_at never goes backwards.
    tx.prepare_cached(
        "UPDATE contracts SET title = ?2, reward_auec = ?3, updated_at = MAX(?4, updated_at) WHERE id = ?1",
    )?
    .execute(params![id, contract.title, contract.reward_auec, now_ms])?;

    // 3. Replace the cargo (cascades to stops and box groups) and re-insert.
    tx.prepare_cached("DELETE FROM cargo_requirements WHERE contract_id = ?1")?
        .execute([id])?;
    let inserted = insert_cargo(&tx, id, &contract.requirements, now_ms)?;

    // 4. Restore kept data on unchanged rows.
    let mut insert_box = tx.prepare_cached(
        "INSERT INTO cargo_box_groups (requirement_id, box_size_scu_milli, box_count, source_quality)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    let mut restore_tick =
        tx.prepare_cached("UPDATE cargo_stops SET reminder_done_at = ?2 WHERE id = ?1")?;
    for req in &inserted {
        let box_key = (
            req.commodity_key.clone(),
            req.quantity_scu_milli,
            req.max_box_size_scu_milli,
        );
        if let Some(groups) = boxes.get_mut(&box_key).and_then(VecDeque::pop_front) {
            for (size, count, quality) in groups {
                insert_box.execute(params![req.id, size, count, quality])?;
            }
        }
        for (stop_id, operation, location_key) in &req.stops {
            let key = (req.commodity_key.clone(), *operation, location_key.clone());
            if let Some(done_at) = ticks.get_mut(&key).and_then(VecDeque::pop_front) {
                restore_tick.execute(params![stop_id, done_at])?;
            }
        }
    }
    drop(insert_box);
    drop(restore_tick);

    let detail = load_contract(&tx, id)?.ok_or(FinalizeError::NotFound(id))?;
    tx.commit()?;
    Ok(detail)
}

/// Ticks (`done`) or unticks the given stops, all or nothing. Only stops of
/// active contracts can change. A tick keeps its first timestamp.
pub fn set_stop_reminders(
    conn: &mut Connection,
    stop_ids: &[i64],
    done: bool,
    now_ms: i64,
) -> Result<Vec<StopDetail>, FinalizeError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for &stop_id in stop_ids {
        let status: Option<ContractStatus> = tx
            .prepare_cached(
                "SELECT c.status FROM cargo_stops s
                 JOIN cargo_requirements q ON q.id = s.requirement_id
                 JOIN contracts c ON c.id = q.contract_id
                 WHERE s.id = ?1",
            )?
            .query_row([stop_id], |r| r.get(0))
            .optional()?;
        match status {
            None => return Err(FinalizeError::StopNotFound(stop_id)),
            Some(ContractStatus::Active) => {}
            Some(other) => {
                return Err(FinalizeError::StopNotActive {
                    id: stop_id,
                    status: other.as_str(),
                })
            }
        }
    }
    let mut update = tx.prepare_cached(
        "UPDATE cargo_stops
         SET reminder_done_at = CASE WHEN ?2 THEN COALESCE(reminder_done_at, ?3) ELSE NULL END
         WHERE id = ?1",
    )?;
    let mut select = tx.prepare_cached(
        "SELECT id, location_name, quantity_scu_milli, reminder_done_at FROM cargo_stops WHERE id = ?1",
    )?;
    let mut stops = Vec::with_capacity(stop_ids.len());
    for &stop_id in stop_ids {
        update.execute(params![stop_id, done, now_ms])?;
        stops.push(select.query_row([stop_id], |r| {
            Ok(StopDetail {
                id: r.get(0)?,
                location_name: r.get(1)?,
                quantity_scu_milli: r.get(2)?,
                reminder_done_at: r.get(3)?,
            })
        })?);
    }
    drop(update);
    drop(select);
    tx.commit()?;
    Ok(stops)
}
