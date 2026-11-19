//! Tauri IPC commands exposed to the frontend.
//!
//! Each command runs its database work on a blocking worker thread, never on
//! the UI thread, and returns a typed, serializable error.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::contracts::validation::validate_new_contract;
use crate::contracts::{
    repository, AcceptanceTime, ContractDetail, NewContractInput, ValidationIssue,
};
use crate::database::models::ContractSource;
use crate::database::{DatabaseError, DatabaseState};
use crate::reference::{self, ReferenceCommodity, ReferenceLocation};
use crate::settings::{self, ContractGrouping, SettingsError};

/// How long a command waits for startup database initialization.
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Error returned to the frontend, tagged by `kind`.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CommandError {
    /// The input was rejected; nothing was saved.
    Validation { issues: Vec<ValidationIssue> },
    /// The database could not be opened or is still starting.
    Unavailable { message: String },
    /// A database operation failed; nothing was saved.
    Database { message: String },
    /// Unexpected internal failure.
    Internal { message: String },
}

impl From<DatabaseError> for CommandError {
    fn from(error: DatabaseError) -> Self {
        match error {
            DatabaseError::Unavailable(_) => Self::Unavailable {
                message: error.to_string(),
            },
            other => Self::Database {
                message: other.to_string(),
            },
        }
    }
}

impl From<rusqlite::Error> for CommandError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database {
            message: error.to_string(),
        }
    }
}

async fn run_blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| CommandError::Internal {
            message: e.to_string(),
        })?
}

/// Validates and saves a manually entered contract.
#[tauri::command]
pub async fn create_contract(
    app: AppHandle,
    input: NewContractInput,
) -> Result<ContractDetail, CommandError> {
    run_blocking(move || {
        let contract =
            validate_new_contract(&input).map_err(|issues| CommandError::Validation { issues })?;
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(repository::create_contract(
            &mut conn,
            &contract,
            &AcceptanceTime::now_local(),
            ContractSource::Manual,
        )?)
    })
    .await
}

/// Lists all active contracts with their cargo.
#[tauri::command]
pub async fn list_active_contracts(app: AppHandle) -> Result<Vec<ContractDetail>, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let conn = db.connection()?;
        Ok(repository::list_active_contracts(&conn)?)
    })
    .await
}

impl From<SettingsError> for CommandError {
    fn from(error: SettingsError) -> Self {
        Self::Database {
            message: error.to_string(),
        }
    }
}

/// The persisted Contracts page display mode.
#[tauri::command]
pub async fn get_contract_grouping(app: AppHandle) -> Result<ContractGrouping, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let conn = db.connection()?;
        Ok(settings::load(&conn)?.contract_grouping)
    })
    .await
}

/// Persists the Contracts page display mode. Only the settings table is
/// written; contract records are never touched.
#[tauri::command]
pub async fn set_contract_grouping(
    app: AppHandle,
    grouping: ContractGrouping,
) -> Result<ContractGrouping, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(settings::update(&mut conn, |s| s.contract_grouping = grouping)?.contract_grouping)
    })
    .await
}

/// The user's location catalog (names typed in saved contracts), alphabetical.
#[tauri::command]
pub async fn list_locations(
    app: AppHandle,
) -> Result<Vec<crate::locations::LocationEntry>, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let conn = db.connection()?;
        Ok(crate::locations::list_locations(&conn)?)
    })
    .await
}

/// Distinct commodity names already used in saved contracts.
#[tauri::command]
pub async fn list_used_commodities(app: AppHandle) -> Result<Vec<String>, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let conn = db.connection()?;
        Ok(crate::locations::list_used_commodities(&conn)?)
    })
    .await
}

/// Bundled reference locations (static data, no database access).
#[tauri::command]
pub fn list_reference_locations() -> Result<Vec<ReferenceLocation>, CommandError> {
    reference::locations()
        .map(|list| list.items.clone())
        .map_err(|message| CommandError::Internal { message })
}

/// Bundled reference commodities (static data, no database access).
#[tauri::command]
pub fn list_reference_commodities() -> Result<Vec<ReferenceCommodity>, CommandError> {
    reference::commodities()
        .map(|list| list.items.clone())
        .map_err(|message| CommandError::Internal { message })
}
