//! History (Task 12): finished contracts grouped by their run, lifetime statistics, and
//! Clear History.
//!
//! - Stats count **completed** contracts only; abandoned ones are listed but never counted.
//! - A run is listed when it holds at least one finished contract, with only those contracts.
//! - Restoring lives in `contracts::reactivate::restore_contracts` (shared with Undo).
//! - Clear History deletes every finished contract (cargo cascades), their captures and every
//!   run left empty. Run numbers are protected by `run_sequence` (migration 0006).

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, TransactionBehavior};
use serde::Serialize;

use crate::capture::storage;
use crate::contracts::repository::list_finished_contracts;
use crate::contracts::ContractDetail;
use crate::database::models::ContractStatus;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStats {
    pub completed_scu_milli: i64,
    pub completed_contracts: i64,
    /// Sum of completed rewards: the contract values, not verified payouts.
    pub estimated_auec: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRun {
    pub run_id: i64,
    pub run_number: i64,
    pub run_date: String,
    pub completed: i64,
    pub abandoned: i64,
    pub completed_scu_milli: i64,
    pub earned_auec: i64,
    /// Finished contracts of this run, newest finish first.
    pub contracts: Vec<ContractDetail>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub stats: HistoryStats,
    /// Newest run first.
    pub runs: Vec<HistoryRun>,
}

/// Everything the History page shows, from one batch of four queries.
pub fn get_history(conn: &Connection) -> rusqlite::Result<History> {
    Ok(build_history(list_finished_contracts(conn)?))
}

/// Groups finished contracts (pure, so it can be tested on its own).
pub fn build_history(mut contracts: Vec<ContractDetail>) -> History {
    contracts.sort_by(|a, b| b.finished_at.cmp(&a.finished_at).then(b.id.cmp(&a.id)));
    let mut stats = HistoryStats::default();
    let mut runs: BTreeMap<i64, HistoryRun> = BTreeMap::new();
    for contract in contracts {
        let run = runs
            .entry(contract.run.run_number)
            .or_insert_with(|| HistoryRun {
                run_id: contract.run.id,
                run_number: contract.run.run_number,
                run_date: contract.run.run_date.clone(),
                completed: 0,
                abandoned: 0,
                completed_scu_milli: 0,
                earned_auec: 0,
                contracts: Vec::new(),
            });
        match contract.status {
            ContractStatus::Completed => {
                run.completed += 1;
                run.completed_scu_milli += contract.total_scu_milli;
                run.earned_auec += contract.reward_auec;
                stats.completed_contracts += 1;
                stats.completed_scu_milli += contract.total_scu_milli;
                stats.estimated_auec += contract.reward_auec;
            }
            ContractStatus::Abandoned => run.abandoned += 1,
            ContractStatus::Active => continue, // never listed (the query excludes them)
        }
        run.contracts.push(contract);
    }
    History {
        stats,
        runs: runs.into_values().rev().collect(),
    }
}

/// What Clear History removed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearReport {
    pub completed: i64,
    pub abandoned: i64,
    pub runs: i64,
    pub captures: i64,
    /// Screenshot files that could not be deleted after the commit (their rows are gone; the
    /// startup orphan cleanup removes them later).
    pub file_errors: Vec<String>,
}

/// Counts shown in the Clear History dialog.
pub fn finished_counts(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    conn.query_row(
        "SELECT COALESCE(SUM(status = 'completed'), 0), COALESCE(SUM(status = 'abandoned'), 0)
         FROM contracts WHERE status IN ('completed', 'abandoned')",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

/// One IMMEDIATE transaction: deletes finished contracts (cargo cascades), their capture rows
/// and every run left without contracts. PNG files are deleted after the commit, only inside
/// `captures_dir` and only for validated names. Active contracts, their runs, settings, ships,
/// favourites and the location catalog are never touched.
pub fn clear_history(
    conn: &mut Connection,
    captures_dir: Option<&Path>,
) -> rusqlite::Result<ClearReport> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (completed, abandoned) = finished_counts(&tx)?;
    let files: Vec<String> = tx
        .prepare(
            "DELETE FROM contract_captures
             WHERE contract_id IN (SELECT id FROM contracts WHERE status IN ('completed', 'abandoned'))
             RETURNING file_path",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    tx.execute(
        "DELETE FROM contracts WHERE status IN ('completed', 'abandoned')",
        [],
    )?;
    let runs = tx.execute(
        "DELETE FROM runs WHERE NOT EXISTS (SELECT 1 FROM contracts c WHERE c.run_id = runs.id)",
        [],
    )? as i64;
    tx.commit()?;

    let mut file_errors = Vec::new();
    if let Some(dir) = captures_dir {
        file_errors = storage::remove_files(dir, &files);
    }
    Ok(ClearReport {
        completed,
        abandoned,
        runs,
        captures: files.len() as i64,
        file_errors,
    })
}
