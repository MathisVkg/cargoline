//! Completing and abandoning contracts.
//!
//! Finishing only changes `status`, `finished_at` and `updated_at`. The run,
//! acceptance time, title, reward and all cargo rows stay untouched, and
//! nothing is deleted: finished contracts remain for the History page.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::models::ContractDetail;
use crate::database::models::ContractStatus;

/// The two terminal statuses a contract can be finalised to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalStatus {
    Completed,
    Abandoned,
}

impl FinalStatus {
    /// Parses `"completed"` / `"abandoned"`; anything else is rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "completed" => Some(Self::Completed),
            "abandoned" => Some(Self::Abandoned),
            _ => None,
        }
    }

    pub const fn status(self) -> ContractStatus {
        match self {
            Self::Completed => ContractStatus::Completed,
            Self::Abandoned => ContractStatus::Abandoned,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FinalizeError {
    #[error("contract {0} does not exist")]
    NotFound(i64),
    #[error("contract {id} is already {status}")]
    NotActive { id: i64, status: &'static str },
    #[error("cargo stop {0} does not exist")]
    StopNotFound(i64),
    #[error("cargo stop {id} belongs to a contract that is already {status}")]
    StopNotActive { id: i64, status: &'static str },
    /// Bulk only: the list shown to the user no longer matches the database.
    #[error("the active contract list changed; nothing was finalised")]
    Conflict {
        not_found: Vec<i64>,
        not_active: Vec<i64>,
    },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// Finalises one active contract and returns its updated detail.
pub fn finalize_contract(
    conn: &mut Connection,
    id: i64,
    status: FinalStatus,
    now_ms: i64,
) -> Result<ContractDetail, FinalizeError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    match current_status(&tx, id)? {
        None => return Err(FinalizeError::NotFound(id)),
        Some(ContractStatus::Active) => {}
        Some(other) => {
            return Err(FinalizeError::NotActive {
                id,
                status: other.as_str(),
            })
        }
    }
    mark_finished(&tx, id, status, now_ms)?;
    let detail = super::repository::load_contract(&tx, id)?.ok_or(FinalizeError::NotFound(id))?;
    tx.commit()?;
    Ok(detail)
}

/// Finalises exactly `ids` (as shown to the user), all or nothing. If any id
/// is missing or no longer active, nothing changes and a conflict is returned.
/// `ids` must be non-empty and without duplicates (checked by the caller).
pub fn finalize_contracts(
    conn: &mut Connection,
    ids: &[i64],
    status: FinalStatus,
    now_ms: i64,
) -> Result<Vec<ContractDetail>, FinalizeError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (mut not_found, mut not_active) = (Vec::new(), Vec::new());
    for &id in ids {
        match current_status(&tx, id)? {
            None => not_found.push(id),
            Some(ContractStatus::Active) => {}
            Some(_) => not_active.push(id),
        }
    }
    if !not_found.is_empty() || !not_active.is_empty() {
        // Dropping the transaction rolls back (nothing was written anyway).
        return Err(FinalizeError::Conflict {
            not_found,
            not_active,
        });
    }
    let mut details = Vec::with_capacity(ids.len());
    for &id in ids {
        mark_finished(&tx, id, status, now_ms)?;
        details
            .push(super::repository::load_contract(&tx, id)?.ok_or(FinalizeError::NotFound(id))?);
    }
    tx.commit()?;
    Ok(details)
}

fn current_status(conn: &Connection, id: i64) -> rusqlite::Result<Option<ContractStatus>> {
    conn.prepare_cached("SELECT status FROM contracts WHERE id = ?1")?
        .query_row([id], |r| r.get(0))
        .optional()
}

/// Sets the terminal status. The finish time is never earlier than the
/// acceptance time (schema CHECK), even if the system clock went backwards.
fn mark_finished(
    conn: &Connection,
    id: i64,
    status: FinalStatus,
    now_ms: i64,
) -> rusqlite::Result<()> {
    let changed = conn
        .prepare_cached(
            "UPDATE contracts
             SET status = ?2,
                 finished_at = MAX(?3, accepted_at),
                 updated_at = MAX(?3, created_at)
             WHERE id = ?1 AND status = 'active'",
        )?
        .execute(params![id, status.status(), now_ms])?;
    debug_assert_eq!(changed, 1, "status was checked inside the same transaction");
    Ok(())
}

/// Why a bulk request was rejected before touching the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkRequestError {
    /// Only "completed" and "abandoned" are accepted.
    InvalidStatus,
    Empty,
    Duplicates,
}

impl BulkRequestError {
    /// (input path, message) for a validation issue.
    pub const fn issue(self) -> (&'static str, &'static str) {
        match self {
            Self::InvalidStatus => ("status", "Status must be \"completed\" or \"abandoned\"."),
            Self::Empty => ("ids", "Select at least one contract."),
            Self::Duplicates => ("ids", "The contract list contains duplicates."),
        }
    }
}

/// Checks a bulk request: valid terminal status, non-empty list, no duplicates.
pub fn validate_bulk_request(ids: &[i64], status: &str) -> Result<FinalStatus, BulkRequestError> {
    let status = FinalStatus::parse(status).ok_or(BulkRequestError::InvalidStatus)?;
    if ids.is_empty() {
        return Err(BulkRequestError::Empty);
    }
    let unique: std::collections::HashSet<i64> = ids.iter().copied().collect();
    if unique.len() != ids.len() {
        return Err(BulkRequestError::Duplicates);
    }
    Ok(status)
}
