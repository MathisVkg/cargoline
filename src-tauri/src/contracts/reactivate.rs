//! Setting finished contracts back to active, all or nothing, in one transaction.
//!
//! Two rules share the same code:
//! - [`reactivate_contracts`] (Undo): only `completed` contracts finished within the
//!   last 5 minutes;
//! - [`restore_contracts`] (History): `completed` or `abandoned`, no time limit.
//!
//! Both set `status = 'active'`, clear `finished_at` and move `updated_at` forward (never back).
//! The run, cargo, ticks, MissionId and capture link are untouched.

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
    #[error("Contract {id} is {status}, not completed or abandoned.")]
    NotFinished { id: i64, status: String },
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

#[derive(Debug, Clone, Copy)]
enum Rule {
    /// Undo: completed only, within [`REACTIVATE_WINDOW_MS`].
    Undo,
    /// History restore: completed or abandoned, any age.
    Restore,
}

impl Rule {
    fn check(
        self,
        id: i64,
        status: String,
        finished: Option<i64>,
        now_ms: i64,
    ) -> Result<(), ReactivateError> {
        match self {
            Rule::Undo if status != "completed" => {
                Err(ReactivateError::NotCompleted { id, status })
            }
            Rule::Undo if finished.is_none_or(|f| now_ms - f > REACTIVATE_WINDOW_MS) => {
                Err(ReactivateError::TooOld(id))
            }
            Rule::Restore if status != "completed" && status != "abandoned" => {
                Err(ReactivateError::NotFinished { id, status })
            }
            _ => Ok(()),
        }
    }
}

/// Undo of a recent completion.
pub fn reactivate_contracts(
    conn: &mut Connection,
    ids: &[i64],
    now_ms: i64,
) -> Result<Vec<ContractDetail>, ReactivateError> {
    set_active(conn, ids, now_ms, Rule::Undo)
}

/// Restore from History: completed or abandoned contracts, no time limit.
pub fn restore_contracts(
    conn: &mut Connection,
    ids: &[i64],
    now_ms: i64,
) -> Result<Vec<ContractDetail>, ReactivateError> {
    set_active(conn, ids, now_ms, Rule::Restore)
}

fn set_active(
    conn: &mut Connection,
    ids: &[i64],
    now_ms: i64,
    rule: Rule,
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
        let Some((status, finished)) = row else {
            return Err(ReactivateError::NotFound(id));
        };
        rule.check(id, status, finished, now_ms)?;
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
