//! Undo of a recent completion (Task 11 Part B; the History task may reuse it).
//!
//! Sets `completed` contracts back to `active` and clears `finished_at`, all or nothing, in one
//! transaction. Only contracts completed within the last 5 minutes are accepted; the run, ticks,
//! cargo and capture are untouched.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::repository::load_contract;
use super::ContractDetail;

/// How recent a completion must be to be undone.
pub const REACTIVATE_WINDOW_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReactivateError {
    #[error("Select at least one contract.")]
    Empty,
    #[error("Contract {0} does not exist.")]
    NotFound(i64),
    #[error("Contract {id} is {status}, not completed.")]
    NotCompleted { id: i64, status: String },
    #[error(
        "Contract {0} was completed more than 5 minutes ago and can no longer be undone here."
    )]
    TooOld(i64),
    #[error("Database error: {0}")]
    Sqlite(String),
}

impl From<rusqlite::Error> for ReactivateError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e.to_string())
    }
}

pub fn reactivate_contracts(
    conn: &mut Connection,
    ids: &[i64],
    now_ms: i64,
) -> Result<Vec<ContractDetail>, ReactivateError> {
    if ids.is_empty() {
        return Err(ReactivateError::Empty);
    }
    let mut unique = ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for &id in &unique {
        let row: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT status, finished_at FROM contracts WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            None => return Err(ReactivateError::NotFound(id)),
            Some((status, _)) if status != "completed" => {
                return Err(ReactivateError::NotCompleted { id, status })
            }
            Some((_, finished)) if finished.is_none_or(|f| now_ms - f > REACTIVATE_WINDOW_MS) => {
                return Err(ReactivateError::TooOld(id));
            }
            Some(_) => {}
        }
        tx.execute(
            "UPDATE contracts SET status = 'active', finished_at = NULL, updated_at = MAX(?2, updated_at) WHERE id = ?1",
            params![id, now_ms],
        )?;
    }
    let details = unique
        .iter()
        .map(|&id| load_contract(&tx, id).map(|c| c.expect("just updated")))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    tx.commit()?;
    Ok(details)
}
