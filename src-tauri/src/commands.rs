//! Tauri IPC commands exposed to the frontend.
//!
//! Each command runs its database work on a blocking worker thread, never on
//! the UI thread, and returns a typed, serializable error.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::capture::service::{self as capture_service, ServiceError};
use crate::capture::storage::CaptureInfo;
use crate::capture::CaptureError;
use crate::contracts::finalize::{self, FinalStatus, FinalizeError};
use crate::contracts::validation::validate_new_contract;
use crate::contracts::{
    repository, AcceptanceTime, ContractDetail, NewContractInput, ValidationIssue,
};
use crate::database::models::ContractSource;
use crate::database::{DatabaseError, DatabaseState};
use crate::reference::{self, ReferenceCommodity, ReferenceLocation};
use crate::settings::{self, AppSettings, CaptureSource, ContractGrouping, SettingsError};

/// How long a command waits for startup database initialization.
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Error returned to the frontend, tagged by `kind`.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CommandError {
    /// The input was rejected; nothing was saved.
    Validation { issues: Vec<ValidationIssue> },
    /// The database could not be opened or is still starting.
    Unavailable { message: String },
    /// A database operation failed; nothing was saved.
    Database { message: String },
    /// Unexpected internal failure.
    Internal { message: String },
    /// The contract does not exist.
    NotFound { message: String, id: i64 },
    /// The contract is already completed or abandoned.
    NotActive {
        message: String,
        id: i64,
        status: String,
    },
    /// Bulk action: the list shown to the user is out of date; nothing changed.
    Conflict {
        message: String,
        not_found: Vec<i64>,
        not_active: Vec<i64>,
    },
    /// A screenshot could not be taken. Manual entry is unaffected.
    Capture {
        reason: CaptureFailure,
        message: String,
    },
}

/// Why a capture failed, for the preview's message and Retry button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureFailure {
    WindowNotFound,
    Minimized,
    Busy,
    Timeout,
    Unsupported,
    Failed,
}

impl From<CaptureError> for CommandError {
    fn from(error: CaptureError) -> Self {
        let reason = match error {
            CaptureError::WindowNotFound => CaptureFailure::WindowNotFound,
            CaptureError::Minimized => CaptureFailure::Minimized,
            CaptureError::Busy => CaptureFailure::Busy,
            CaptureError::Timeout => CaptureFailure::Timeout,
            CaptureError::Unsupported => CaptureFailure::Unsupported,
            CaptureError::Failed(_) => CaptureFailure::Failed,
        };
        Self::Capture {
            reason,
            message: error.to_string(),
        }
    }
}

impl From<ServiceError> for CommandError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::Capture(e) => e.into(),
            ServiceError::Database(e) => e.into(),
            ServiceError::Settings(e) => e.into(),
            ServiceError::Storage(e) => Self::Capture {
                reason: CaptureFailure::Failed,
                message: e.to_string(),
            },
            ServiceError::NoDataDir(message) => Self::Unavailable { message },
        }
    }
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

/// Validates and saves a manually entered contract. `capture_id` is the form's unsaved
/// screenshot, linked to the new contract in the same transaction.
#[tauri::command]
pub async fn create_contract(
    app: AppHandle,
    input: NewContractInput,
    capture_id: Option<i64>,
) -> Result<ContractDetail, CommandError> {
    run_blocking(move || {
        let contract =
            validate_new_contract(&input).map_err(|issues| CommandError::Validation { issues })?;
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(repository::create_contract_with_capture(
            &mut conn,
            &contract,
            &AcceptanceTime::now_local(),
            ContractSource::Manual,
            capture_id,
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

impl From<FinalizeError> for CommandError {
    fn from(error: FinalizeError) -> Self {
        let message = error.to_string();
        match error {
            FinalizeError::NotFound(id) => Self::NotFound { message, id },
            FinalizeError::NotActive { id, status }
            | FinalizeError::StopNotActive { id, status } => Self::NotActive {
                message,
                id,
                status: status.to_string(),
            },
            FinalizeError::StopNotFound(id) => Self::NotFound { message, id },
            FinalizeError::Conflict {
                not_found,
                not_active,
            } => Self::Conflict {
                message,
                not_found,
                not_active,
            },
            FinalizeError::Sqlite(e) => Self::Database {
                message: e.to_string(),
            },
        }
    }
}

fn validation(path: &str, message: &str) -> CommandError {
    CommandError::Validation {
        issues: vec![ValidationIssue {
            path: path.into(),
            message: message.into(),
        }],
    }
}

async fn finalize_one(
    app: AppHandle,
    id: i64,
    status: FinalStatus,
) -> Result<ContractDetail, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(finalize::finalize_contract(
            &mut conn,
            id,
            status,
            crate::database::now_utc_ms(),
        )?)
    })
    .await
}

/// Marks one active contract completed. Its run and cargo are unchanged.
#[tauri::command]
pub async fn complete_contract(app: AppHandle, id: i64) -> Result<ContractDetail, CommandError> {
    finalize_one(app, id, FinalStatus::Completed).await
}

/// Marks one active contract abandoned. Its run and cargo are unchanged.
#[tauri::command]
pub async fn abandon_contract(app: AppHandle, id: i64) -> Result<ContractDetail, CommandError> {
    finalize_one(app, id, FinalStatus::Abandoned).await
}

/// Finalises exactly the contracts the confirmation dialog showed, all or
/// nothing. A stale list returns `conflict` and changes nothing.
#[tauri::command]
pub async fn finalize_active_contracts(
    app: AppHandle,
    ids: Vec<i64>,
    status: String,
) -> Result<Vec<ContractDetail>, CommandError> {
    let status = finalize::validate_bulk_request(&ids, &status).map_err(|e| {
        let (path, message) = e.issue();
        validation(path, message)
    })?;
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(finalize::finalize_contracts(
            &mut conn,
            &ids,
            status,
            crate::database::now_utc_ms(),
        )?)
    })
    .await
}

/// Edits an active contract with the same input and validation as
/// `create_contract`. The id, status, run and acceptance time never change.
#[tauri::command]
pub async fn update_contract(
    app: AppHandle,
    id: i64,
    input: NewContractInput,
) -> Result<ContractDetail, CommandError> {
    run_blocking(move || {
        let contract =
            validate_new_contract(&input).map_err(|issues| CommandError::Validation { issues })?;
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(crate::contracts::edit::update_contract(
            &mut conn,
            id,
            &contract,
            crate::database::now_utc_ms(),
        )?)
    })
    .await
}

/// Ticks or unticks the Pick Up / Turn In reminder of the given stops (one
/// displayed row can combine several). Only stops of active contracts.
#[tauri::command]
pub async fn set_stop_reminder(
    app: AppHandle,
    stop_ids: Vec<i64>,
    done: bool,
) -> Result<Vec<crate::contracts::StopDetail>, CommandError> {
    if stop_ids.is_empty() {
        return Err(validation("stopIds", "Select at least one stop."));
    }
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(crate::contracts::edit::set_stop_reminders(
            &mut conn,
            &stop_ids,
            done,
            crate::database::now_utc_ms(),
        )?)
    })
    .await
}

/// All settings (the Settings page and the Add Contract capture read them).
#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<AppSettings, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let conn = db.connection()?;
        Ok(settings::load(&conn)?)
    })
    .await
}

/// The OCR Capture group of the Settings page.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureSettingsInput {
    pub source: CaptureSource,
    pub delay_ms: u32,
    pub on_add_contract: bool,
}

/// Validates and saves the capture settings; nothing is written if any is invalid.
#[tauri::command]
pub async fn update_capture_settings(
    app: AppHandle,
    input: CaptureSettingsInput,
) -> Result<AppSettings, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        settings::update(&mut conn, |s| {
            s.capture_source = input.source;
            s.capture_delay_ms = input.delay_ms;
            s.capture_on_add_contract = input.on_add_contract;
        })
        .map_err(|e| match e {
            SettingsError::Invalid(invalid) => validation(invalid.field, &invalid.reason),
            other => other.into(),
        })
    })
    .await
}

/// Captures Star Citizen (window or display, per the settings) and stores the image.
/// `replaces`: the form's previous unsaved capture, deleted once the new one is stored.
#[tauri::command]
pub async fn capture_screenshot(
    app: AppHandle,
    replaces: Option<i64>,
) -> Result<CaptureInfo, CommandError> {
    run_blocking(move || Ok(capture_service::capture_and_store(&app, replaces)?)).await
}

/// Deletes an unsaved capture (row and file). Captures linked to a contract are kept.
#[tauri::command]
pub async fn discard_capture(app: AppHandle, id: i64) -> Result<bool, CommandError> {
    run_blocking(move || Ok(capture_service::discard(&app, id)?)).await
}
