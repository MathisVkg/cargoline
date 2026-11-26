//! Tauri IPC commands exposed to the frontend.
//!
//! Each command runs its database work on a blocking worker thread, never on
//! the UI thread, and returns a typed, serializable error.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::backup::{self, BackupError, BackupInfo, StartupNotice, StartupNoticeState};
use crate::capture::service::{self as capture_service, ServiceError};
use crate::capture::storage::CaptureInfo;
use crate::capture::CaptureError;
use crate::contracts::finalize::{self, FinalStatus, FinalizeError};
use crate::contracts::reactivate::{self, ReactivateError};
use crate::contracts::validation::validate_new_contract;
use crate::contracts::{
    repository, AcceptanceTime, ContractDetail, NewContractInput, ValidationIssue,
};
use crate::database::models::ContractSource;
use crate::database::{DatabaseError, DatabaseState};
use crate::gamelog::service::GameLogState;
use crate::ocr::{service as ocr_service, OcrError, OcrResult};
use crate::reference::{self, ReferenceCommodity, ReferenceLocation};
use crate::settings::{
    self, AppSettings, CaptureSource, CompleteFromGameMode, ContractGrouping, SettingsError,
};
use crate::ships::{self, CatalogInfo, SelectedShip, ShipEntry, ShipError};

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
    /// Text recognition failed or was cancelled. Manual entry is unaffected.
    Ocr { reason: OcrFailure, message: String },
    /// The ship id is not in the catalog (or no longer listed).
    UnknownShip { message: String, ship_id: String },
    /// An Undo of a completion was refused; nothing changed.
    CannotReactivate {
        message: String,
        reason: ReactivateFailure,
        id: Option<i64>,
    },
    /// Backup export or restore refused (Task 17); nothing changed.
    Backup {
        reason: BackupFailure,
        message: String,
    },
    /// Restore from History: the contract is active, not completed or abandoned; nothing changed.
    NotFinished {
        message: String,
        id: i64,
        status: String,
    },
}

/// Why a backup was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupFailure {
    NotSqlite,
    NotCargoLine,
    TooNew,
    Corrupted,
    SameAsLive,
    UnknownLink,
    Io,
}

impl From<BackupError> for CommandError {
    fn from(error: BackupError) -> Self {
        let message = error.to_string();
        let reason = match error {
            BackupError::NotSqlite => BackupFailure::NotSqlite,
            BackupError::NotCargoLine => BackupFailure::NotCargoLine,
            BackupError::TooNew { .. } => BackupFailure::TooNew,
            BackupError::Corrupted(_) => BackupFailure::Corrupted,
            BackupError::SameAsLive => BackupFailure::SameAsLive,
            BackupError::Io(_) => BackupFailure::Io,
            BackupError::Sqlite(message) => return Self::Database { message },
        };
        Self::Backup { reason, message }
    }
}

/// Why `reactivate_contracts` refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReactivateFailure {
    Empty,
    NotFound,
    NotCompleted,
    NotFinished,
    TooOld,
}

impl From<ShipError> for CommandError {
    fn from(error: ShipError) -> Self {
        match error {
            ShipError::Unknown(ship_id) => Self::UnknownShip {
                message: format!("Unknown ship: {ship_id}"),
                ship_id,
            },
            ShipError::Invalid(issues) => Self::Validation {
                issues: issues
                    .into_iter()
                    .map(|i| ValidationIssue {
                        path: i.field.into(),
                        message: i.message,
                    })
                    .collect(),
            },
            ShipError::Sqlite(e) => Self::Database { message: e.0 },
        }
    }
}

impl From<ReactivateError> for CommandError {
    fn from(error: ReactivateError) -> Self {
        let message = error.to_string();
        let (reason, id) = match error {
            ReactivateError::Empty => (ReactivateFailure::Empty, None),
            ReactivateError::NotFound(id) => (ReactivateFailure::NotFound, Some(id)),
            ReactivateError::NotCompleted { id, .. } => (ReactivateFailure::NotCompleted, Some(id)),
            ReactivateError::NotFinished { id, .. } => (ReactivateFailure::NotFinished, Some(id)),
            ReactivateError::TooOld(id) => (ReactivateFailure::TooOld, Some(id)),
            ReactivateError::Sqlite(message) => return Self::Database { message },
        };
        Self::CannotReactivate {
            message,
            reason,
            id,
        }
    }
}

/// Why text recognition did not return a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OcrFailure {
    EngineMissing,
    Timeout,
    NoPanel,
    Cancelled,
    Failed,
}

impl From<OcrError> for CommandError {
    fn from(error: OcrError) -> Self {
        let reason = match error {
            OcrError::EngineMissing(_) => OcrFailure::EngineMissing,
            OcrError::Timeout => OcrFailure::Timeout,
            OcrError::NoPanel => OcrFailure::NoPanel,
            OcrError::Cancelled => OcrFailure::Cancelled,
            OcrError::Failed(_) => OcrFailure::Failed,
        };
        Self::Ocr {
            reason,
            message: error.to_string(),
        }
    }
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

/// Validates and saves a new contract.
/// - `capture_id`: the form's unsaved screenshot, linked in the same transaction.
/// - `from_ocr`: the form was filled from that screenshot (source `ocr`, even if edited).
/// - `game_mission_id`: the in-game MissionId (Game.log); refused if a contract already has it.
/// - `from_game_log`: the form was opened automatically by a Game.log accept: source
///   `auto_capture`, and the capture's method `auto_capture`.
#[tauri::command]
pub async fn create_contract(
    app: AppHandle,
    input: NewContractInput,
    capture_id: Option<i64>,
    from_ocr: Option<bool>,
    game_mission_id: Option<String>,
    from_game_log: Option<bool>,
) -> Result<ContractDetail, CommandError> {
    let game_mission_id = game_mission_id
        .map(|m| m.trim().to_ascii_lowercase())
        .filter(|m| !m.is_empty());
    if let Some(mission) = &game_mission_id {
        if !crate::gamelog::patterns::is_mission_id(mission) {
            return Err(validation(
                "gameMissionId",
                "This is not an in-game mission id.",
            ));
        }
    }
    let from_game_log = from_game_log == Some(true) && game_mission_id.is_some();
    let source = if from_game_log {
        ContractSource::AutoCapture
    } else if from_ocr == Some(true) && capture_id.is_some() {
        ContractSource::Ocr
    } else {
        ContractSource::Manual
    };
    let links = repository::SaveLinks {
        capture_id,
        game_mission_id: game_mission_id.clone(),
        auto_capture: from_game_log && capture_id.is_some(),
    };
    run_blocking(move || {
        let contract =
            validate_new_contract(&input).map_err(|issues| CommandError::Validation { issues })?;
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        if let Some(mission) = &game_mission_id {
            if let Some(existing) = repository::contract_for_mission(&conn, mission)? {
                return Err(validation(
                    "gameMissionId",
                    &format!(
                        "This in-game contract was already added as C{:02}.",
                        existing
                    ),
                ));
            }
        }
        Ok(repository::create_contract_linked(
            &mut conn,
            &contract,
            &AcceptanceTime::now_local(),
            source,
            &links,
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
        let list = repository::list_active_contracts(&conn)?;
        drop(conn);
        // Deferred startup work (the capture cleanup) may start now (Task 18).
        app.state::<crate::startup::FirstScreenGate>().open();
        crate::trace::mark_once("first contracts list served");
        Ok(list)
    })
    .await
}

/// A startup timing mark from the frontend (Task 18).
#[derive(Debug, Deserialize)]
pub struct FrontendMark {
    pub name: String,
    /// Wall-clock time, Unix epoch ms.
    pub at: f64,
}

/// Records frontend timing marks when `CARGO_LINE_STARTUP_TRACE` is on. Returns whether tracing
/// is on: when it is off the frontend stops recording and never calls again.
#[tauri::command]
pub async fn trace_marks(marks: Vec<FrontendMark>) -> bool {
    if !crate::trace::enabled() {
        return false;
    }
    for m in marks.iter().take(64) {
        crate::trace::mark_at(&format!("ui: {}", m.name), m.at);
    }
    true
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

/// Undo of a recent completion: sets the given `completed` contracts back to active, all or
/// nothing. Only completions within the last 5 minutes; run, ticks and cargo are untouched.
#[tauri::command]
pub async fn reactivate_contracts(
    app: AppHandle,
    ids: Vec<i64>,
) -> Result<Vec<ContractDetail>, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(reactivate::reactivate_contracts(
            &mut conn,
            &ids,
            crate::database::now_utc_ms(),
        )?)
    })
    .await
}

/* ------------------------------------------------------------- History */

/// Restore errors use the plain kinds: `notFound` and `notFinished` (and `validation` when
/// the list is empty).
fn restore_error(error: ReactivateError) -> CommandError {
    match error {
        ReactivateError::Empty => validation("ids", "Select at least one contract."),
        ReactivateError::NotFound(id) => CommandError::NotFound {
            message: error.to_string(),
            id,
        },
        ReactivateError::NotFinished { id, ref status } => CommandError::NotFinished {
            message: error.to_string(),
            id,
            status: status.clone(),
        },
        ReactivateError::Sqlite(message) => CommandError::Database { message },
        other => CommandError::Internal {
            message: other.to_string(),
        },
    }
}

/// Finished contracts grouped by run (newest first) and the lifetime statistics.
#[tauri::command]
pub async fn get_history(app: AppHandle) -> Result<crate::history::History, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let conn = db.connection()?;
        Ok(crate::history::get_history(&conn)?)
    })
    .await
}

/// Sets completed or abandoned contracts back to active, all or nothing, with no time limit.
/// The run, cargo, ticks, MissionId and capture link are untouched.
#[tauri::command]
pub async fn restore_contracts(
    app: AppHandle,
    ids: Vec<i64>,
) -> Result<Vec<ContractDetail>, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        reactivate::restore_contracts(&mut conn, &ids, crate::database::now_utc_ms())
            .map_err(restore_error)
    })
    .await
}

/// Erases all history: every finished contract (cargo cascades), their screenshots and every
/// run left empty. Active contracts and settings are not affected. Run numbers are never reused.
#[tauri::command]
pub async fn clear_history(app: AppHandle) -> Result<crate::history::ClearReport, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        let dir = capture_service::captures_dir(&app).ok();
        let report = crate::history::clear_history(&mut conn, dir.as_deref())?;
        for e in &report.file_errors {
            eprintln!("[cargo-line] clear history: {e}");
        }
        Ok(report)
    })
    .await
}

/* --------------------------------------------------------------- Ships */

/// Listed catalog ships, alphabetical, with the favourite flag.
#[tauri::command]
pub async fn list_ships(app: AppHandle) -> Result<Vec<ShipEntry>, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let conn = db.connection()?;
        Ok(ships::list_ships(&conn)?)
    })
    .await
}

/// The selected ship; a selection no longer listed is cleared and reported `stale`.
#[tauri::command]
pub async fn get_selected_ship(app: AppHandle) -> Result<SelectedShip, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(ships::get_selected(&mut conn)?)
    })
    .await
}

/// Selects a ship (`null` = no ship). Informational only: never touches contracts or runs.
#[tauri::command]
pub async fn set_selected_ship(
    app: AppHandle,
    id: Option<String>,
) -> Result<SelectedShip, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(ships::set_selected(&mut conn, id.as_deref())?)
    })
    .await
}

/// Stars or unstars a ship. Never changes the selection.
#[tauri::command]
pub async fn set_ship_favorite(
    app: AppHandle,
    id: String,
    favorite: bool,
) -> Result<ShipEntry, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let conn = db.connection()?;
        Ok(ships::set_favorite(
            &conn,
            &id,
            favorite,
            crate::database::now_utc_ms(),
        )?)
    })
    .await
}

/// Adds a custom ship (name 1–60 characters, unique among custom ships ignoring case; 1–100,000
/// SCU). Validation errors use the `validation` kind with paths `name` and `cargoScu`.
#[tauri::command]
pub async fn create_custom_ship(
    app: AppHandle,
    input: ships::CustomShipInput,
) -> Result<ShipEntry, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(ships::create_custom_ship(&mut conn, &input)?)
    })
    .await
}

/// Renames or resizes a custom ship. A catalog or unknown id is `unknownShip`.
#[tauri::command]
pub async fn update_custom_ship(
    app: AppHandle,
    id: String,
    input: ships::CustomShipInput,
) -> Result<ShipEntry, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(ships::update_custom_ship(&mut conn, &id, &input)?)
    })
    .await
}

/// Deletes a custom ship, its favourite, and the selection if it was selected. Contracts are never
/// affected. Returns whether it was the selected ship.
#[tauri::command]
pub async fn delete_custom_ship(app: AppHandle, id: String) -> Result<bool, CommandError> {
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        Ok(ships::delete_custom_ship(&mut conn, &id)?)
    })
    .await
}

/* ---------------------------------------------------------------- Data */

fn app_data_dir(app: &AppHandle) -> Result<std::path::PathBuf, CommandError> {
    crate::data_dir(app).map_err(|e| CommandError::Unavailable {
        message: format!("no app data folder: {e}"),
    })
}

/// True when `candidate` is the live database file (or one of its companions).
fn is_live_database(candidate: &std::path::Path, live: &std::path::Path) -> bool {
    let canon = |p: &std::path::Path| -> Option<std::path::PathBuf> {
        if let Ok(c) = std::fs::canonicalize(p) {
            return Some(c);
        }
        let parent = std::fs::canonicalize(p.parent()?).ok()?;
        Some(parent.join(p.file_name()?))
    };
    match (canon(candidate), canon(live)) {
        (Some(a), Some(b)) => {
            let (a, b) = (
                a.to_string_lossy().to_lowercase(),
                b.to_string_lossy().to_lowercase(),
            );
            a == b || a == format!("{b}-wal") || a == format!("{b}-shm")
        }
        _ => false,
    }
}

/// The Data group: the app data folder (full path) and the number of safety copies.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataInfo {
    pub folder: String,
    pub safety_copies: usize,
}

#[tauri::command]
pub fn get_data_info(app: AppHandle) -> Result<DataInfo, CommandError> {
    let dir = app_data_dir(&app)?;
    Ok(DataInfo {
        safety_copies: backup::safety_copies(&dir.join(backup::BACKUPS_DIR_NAME))
            .map(|v| v.len())
            .unwrap_or(0),
        folder: dir.to_string_lossy().into_owned(),
    })
}

/// Opens the app's own data folder in Explorer. Takes no path from the frontend.
#[tauri::command]
pub fn open_data_folder(app: AppHandle) -> Result<(), CommandError> {
    let dir = app_data_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| CommandError::Internal {
        message: e.to_string(),
    })?;
    let (program, folder) = backup::open_folder_command(&dir);
    std::process::Command::new(program)
        .arg(folder)
        .spawn()
        .map(|_| ())
        .map_err(|e| CommandError::Internal {
            message: format!("could not open Explorer: {e}"),
        })
}

/// The exported file's name and what it contains.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub file_name: String,
    pub info: BackupInfo,
}

/// Writes a consistent copy of the database to `path` (chosen in the Save dialog) and checks it.
#[tauri::command]
pub async fn export_backup(app: AppHandle, path: String) -> Result<ExportResult, CommandError> {
    run_blocking(move || {
        let dest = std::path::PathBuf::from(&path);
        let dir = app_data_dir(&app)?;
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        if is_live_database(&dest, db.path()) {
            return Err(BackupError::SameAsLive.into());
        }
        let conn = db.connection()?;
        let info = backup::export(&conn, &dest, &dir)?;
        Ok(ExportResult {
            file_name: dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(path),
            info,
        })
    })
    .await
}

/// Validates a backup chosen in the Open dialog and returns its contents (counts) for the
/// confirmation. Never modifies the file.
#[tauri::command]
pub async fn inspect_backup(app: AppHandle, path: String) -> Result<BackupInfo, CommandError> {
    run_blocking(move || {
        let file = std::path::PathBuf::from(&path);
        let dir = app_data_dir(&app)?;
        if let Ok(db) = app.state::<DatabaseState>().wait_ready(READY_TIMEOUT) {
            if is_live_database(&file, db.path()) {
                return Err(BackupError::SameAsLive.into());
            }
        }
        Ok(backup::inspect(&file, &dir)?)
    })
    .await
}

/// Restores a backup: validates it again, writes a safety copy of the current data, stages the
/// backup and restarts the app; the swap happens at the next start, before the database opens.
#[tauri::command]
pub async fn restore_backup(app: AppHandle, path: String) -> Result<(), CommandError> {
    let handle = app.clone();
    run_blocking(move || {
        let file = std::path::PathBuf::from(&path);
        let dir = app_data_dir(&app)?;
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        if is_live_database(&file, db.path()) {
            return Err(BackupError::SameAsLive.into());
        }
        backup::inspect(&file, &dir)?;
        {
            let conn = db.connection()?;
            backup::safety_copy(
                &conn,
                &dir.join(backup::BACKUPS_DIR_NAME),
                crate::database::now_utc_ms(),
            )?;
        }
        backup::stage_restore(&file, &dir)?;
        Ok(())
    })
    .await?;
    eprintln!("[cargo-line] restore staged; restarting");
    handle.restart()
}

/// The notice from a restore at startup, once (then cleared).
#[tauri::command]
pub fn take_startup_notice(app: AppHandle) -> Option<StartupNotice> {
    app.state::<StartupNoticeState>()
        .0
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

/// Opens one of the About links in the default browser. The frontend sends a key; the URL comes
/// from the fixed allow-list.
#[tauri::command]
pub fn open_link(key: String) -> Result<(), CommandError> {
    let Some(url) = backup::link_url(&key) else {
        return Err(CommandError::Backup {
            reason: BackupFailure::UnknownLink,
            message: format!("Unknown link: {key}"),
        });
    };
    std::process::Command::new("explorer.exe")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| CommandError::Internal {
            message: format!("could not open the browser: {e}"),
        })
}

/// One bundled catalog's header, for the About data credits.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCredit {
    pub name: &'static str,
    pub game_version: String,
    pub generated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    pub version: String,
    /// `installed`, `portable` or `dev` (a debug build). Task 20: only `installed` self-updates.
    pub mode: crate::layout::AppMode,
    pub catalogs: Vec<CatalogCredit>,
    /// THIRD_PARTY_NOTICES.md, embedded at build time.
    pub third_party_notices: &'static str,
}

pub const THIRD_PARTY_NOTICES: &str = include_str!("../../THIRD_PARTY_NOTICES.md");

#[tauri::command]
pub fn get_about_info(app: AppHandle) -> Result<AboutInfo, CommandError> {
    let mut catalogs = Vec::new();
    if let Ok(list) = ships::bundled() {
        catalogs.push(CatalogCredit {
            name: "Ships",
            game_version: list.game_version.clone(),
            generated_at: list.generated_at.clone(),
        });
    }
    if let Ok(list) = reference::locations() {
        catalogs.push(CatalogCredit {
            name: "Locations",
            game_version: list.game_version.clone(),
            generated_at: list.generated_at.clone(),
        });
    }
    if let Ok(list) = reference::commodities() {
        catalogs.push(CatalogCredit {
            name: "Commodities",
            game_version: list.game_version.clone(),
            generated_at: list.generated_at.clone(),
        });
    }
    Ok(AboutInfo {
        version: app.package_info().version.to_string(),
        mode: crate::layout::current().mode,
        catalogs,
        third_party_notices: THIRD_PARTY_NOTICES,
    })
}

/// `N ships · game <version> · Star Citizen Wiki` for Settings.
#[tauri::command]
pub fn get_ship_catalog_info() -> Result<CatalogInfo, CommandError> {
    ships::catalog_info().map_err(|message| CommandError::Internal { message })
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

/// Reads the text of a stored capture (title, reward and objectives regions). The text is
/// returned only, never stored. `processing_status` goes pending → processing → done / failed.
#[tauri::command]
pub async fn recognize_capture(app: AppHandle, capture_id: i64) -> Result<OcrResult, CommandError> {
    run_blocking(move || Ok(ocr_service::recognize_capture(&app, capture_id)?)).await
}

/// Cancels a running recognition (the form was closed or a recapture started).
#[tauri::command]
pub fn cancel_recognition(app: AppHandle, capture_id: i64) -> bool {
    app.state::<ocr_service::OcrState>().cancel(capture_id)
}

/// What `read_contract_from_capture` returns: the recognised text and the contract read from it.
#[derive(Debug, Serialize)]
pub struct ContractReading {
    pub ocr: OcrResult,
    pub parsed: crate::parser::ParsedContract,
}

/// Recognises a stored capture, then parses it into a contract for the form (never saved here).
/// Names are resolved against the bundled lists plus the user's locations and used commodities.
#[tauri::command]
pub async fn read_contract_from_capture(
    app: AppHandle,
    capture_id: i64,
) -> Result<ContractReading, CommandError> {
    run_blocking(move || {
        let ocr = ocr_service::recognize_capture(&app, capture_id)?;
        let (user_locations, used_commodities) = {
            let state = app.state::<DatabaseState>();
            let db = state.wait_ready(READY_TIMEOUT)?;
            let conn = db.connection()?;
            let locations: Vec<String> = crate::locations::list_locations(&conn)?
                .into_iter()
                .map(|l| l.name)
                .collect();
            (locations, crate::locations::list_used_commodities(&conn)?)
        };
        let references = crate::parser::References::build(&user_locations, &used_commodities);
        let parsed = crate::parser::parse_contract(&ocr, &references);
        Ok(ContractReading { ocr, parsed })
    })
    .await
}

/* ------------------------------------------------------------ Game.log */

/// The Game group of the Settings page.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameLogSettingsInput {
    pub monitoring: bool,
    pub path: Option<String>,
    pub auto_capture: bool,
    /// Complete contracts the game reports as completed (requires monitoring).
    #[serde(default)]
    pub complete_from_game: bool,
    #[serde(default)]
    pub complete_from_game_mode: CompleteFromGameMode,
}

/// Saves the Game.log settings and applies them to the watcher right away (no restart).
/// The path must name a readable `Game.log` file. Turning monitoring off also turns automatic
/// capture off (it requires monitoring).
#[tauri::command]
pub async fn update_game_log_settings(
    app: AppHandle,
    input: GameLogSettingsInput,
) -> Result<AppSettings, CommandError> {
    let path = input
        .path
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    if let Some(p) = &path {
        let named = std::path::Path::new(p)
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("Game.log"));
        if !named {
            return Err(validation("gameLogPath", "Choose the file named Game.log."));
        }
        let readable =
            std::fs::metadata(p).is_ok_and(|m| m.is_file()) && std::fs::File::open(p).is_ok();
        if !readable {
            return Err(validation("gameLogPath", "File not found or not readable."));
        }
    }
    if input.monitoring && path.is_none() {
        return Err(validation("gameLogPath", "Choose the Game.log file first."));
    }
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        let saved = settings::update(&mut conn, |s| {
            s.game_log_monitoring_enabled = input.monitoring;
            s.game_log_path = path;
            s.auto_capture_on_accept = input.monitoring && input.auto_capture;
            s.complete_from_game = input.monitoring && input.complete_from_game;
            s.complete_from_game_mode = input.complete_from_game_mode;
        })
        .map_err(|e| match e {
            SettingsError::Invalid(invalid) => validation(invalid.field, &invalid.reason),
            other => other.into(),
        })?;
        crate::gamelog::service::configure_from_settings(&app.state::<GameLogState>(), &saved);
        Ok(saved)
    })
    .await
}

/// "Watching", "File not found", "Off" (or unreadable) for the status line.
#[tauri::command]
pub fn get_game_log_status(app: AppHandle) -> crate::gamelog::WatchStatus {
    app.state::<GameLogState>().status()
}

/// What Detect found: every `Game.log` (ranked: LIVE first, then most recent) and the message
/// to show next to the button. Never an error: "none found" is a normal outcome.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectResult {
    pub found: Vec<crate::gamelog::detect::FoundLog>,
    pub message: String,
}

fn common_game_roots() -> Vec<std::path::PathBuf> {
    #[cfg(windows)]
    let drives = crate::capture::native::logical_drives();
    #[cfg(not(windows))]
    let drives: Vec<char> = Vec::new();
    let program_files = std::env::var_os("ProgramFiles").map(std::path::PathBuf::from);
    crate::gamelog::detect::common_roots(program_files.as_deref(), &drives)
}

/// Finds `Game.log` without opening the game's process (read-only):
/// 1. the running `StarCitizen.exe` path (no process handle);
/// 2. the RSI Launcher's log (`%APPDATA%\rsilauncher\logs\log.log`);
/// 3. common install folders on every drive, for each channel (LIVE, PTU, EPTU, TECH-PREVIEW, HOTFIX).
#[tauri::command]
pub async fn detect_game_log_path() -> Result<DetectResult, CommandError> {
    use crate::gamelog::detect::{self, DetectMethod};
    run_blocking(|| {
        let mut found = Vec::new();
        #[cfg(windows)]
        if let Some(exe) = crate::capture::native::star_citizen_exe_path() {
            found.extend(detect::found_from_exe(&exe));
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let log = std::path::Path::new(&appdata)
                .join("rsilauncher")
                .join("logs")
                .join("log.log");
            if let Ok(bytes) = std::fs::read(&log) {
                let roots = detect::roots_from_launcher_log(&String::from_utf8_lossy(&bytes));
                found.extend(detect::logs_in_roots(&roots, DetectMethod::Launcher));
            }
        }
        found.extend(detect::logs_in_roots(
            &common_game_roots(),
            DetectMethod::Folders,
        ));
        let found = detect::rank(found);
        let message = detect::detect_message(&found);
        Ok(DetectResult { found, message })
    })
    .await
}

/// The folder Browse… opens in: the current path's folder, else the first existing game channel
/// folder, else none (the system default).
#[tauri::command]
pub async fn game_log_browse_start(
    current: Option<String>,
) -> Result<Option<String>, CommandError> {
    run_blocking(move || {
        Ok(
            crate::gamelog::detect::browse_start(current.as_deref(), &common_game_roots())
                .map(|p| p.to_string_lossy().into_owned()),
        )
    })
    .await
}

/// Saves the capture shortcut (`None` clears it). Rules: `settings::models::validate_shortcut`.
#[tauri::command]
pub async fn update_capture_shortcut(
    app: AppHandle,
    shortcut: Option<String>,
) -> Result<AppSettings, CommandError> {
    let shortcut = shortcut
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    run_blocking(move || {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT)?;
        let mut conn = db.connection()?;
        settings::update(&mut conn, |s| s.capture_shortcut = shortcut).map_err(|e| match e {
            SettingsError::Invalid(invalid) => validation(invalid.field, &invalid.reason),
            other => other.into(),
        })
    })
    .await
}
