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
use crate::gamelog::service::GameLogState;
use crate::ocr::{service as ocr_service, OcrError, OcrResult};
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
    /// Text recognition failed or was cancelled. Manual entry is unaffected.
    Ocr { reason: OcrFailure, message: String },
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

/// Proposes `<channel>\\Game.log` from the running `<channel>\\Bin64\\StarCitizen.exe`
/// (e.g. the LIVE folder).
#[tauri::command]
pub async fn detect_game_log_path() -> Result<String, CommandError> {
    run_blocking(|| {
        #[cfg(windows)]
        let exe = crate::capture::native::star_citizen_exe_path();
        #[cfg(not(windows))]
        let exe: Option<std::path::PathBuf> = None;
        let exe = exe.ok_or_else(|| {
            validation(
                "gameLogPath",
                "Start Star Citizen first, then press Detect.",
            )
        })?;
        let bin = exe.parent();
        let channel = bin.and_then(|b| {
            if b.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("Bin64"))
            {
                b.parent()
            } else {
                Some(b)
            }
        });
        channel
            .map(|c| c.join("Game.log").to_string_lossy().into_owned())
            .ok_or_else(|| validation("gameLogPath", "Could not work out the game folder."))
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
