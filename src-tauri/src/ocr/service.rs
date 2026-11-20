//! App integration of OCR: finds the bundled engine on first use, recognises a stored capture
//! with cancellation, and records `processing_status`. Recognised text is returned only; it is
//! never written to the database or to disk (it can contain private data).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};

use super::engine::{clear_temp_dir, CancelToken, Engine};
use super::{recognize_file, OcrError, OcrResult};
use crate::capture::service::captures_dir;
use crate::capture::storage::{self, ProcessingStatus};
use crate::database::DatabaseState;

/// Per Tesseract call (the anchor pass takes about 1 s on a 4K capture).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(15);
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Folder of the bundle inside the app's resources (see `bundle.resources` in tauri.conf.json).
pub const BUNDLE_DIR: &str = "tesseract";

#[derive(Debug, Default)]
pub struct OcrState {
    /// The verified engine, found on first use (never at startup).
    engine: Mutex<Option<Engine>>,
    /// Running recognitions by capture id, so closing the form can cancel them.
    running: Mutex<HashMap<i64, CancelToken>>,
}

impl OcrState {
    fn engine<R: Runtime>(&self, app: &AppHandle<R>) -> Result<Engine, OcrError> {
        let mut cached = self
            .engine
            .lock()
            .map_err(|_| OcrError::Failed("OCR state poisoned".into()))?;
        if let Some(engine) = cached.as_ref() {
            return Ok(engine.clone());
        }
        let resources = app
            .path()
            .resource_dir()
            .map_err(|e| OcrError::EngineMissing(format!("no resource folder: {e}")))?;
        let temp = crate::local_data_dir(app)
            .map_err(|e| OcrError::Failed(format!("no local data folder: {e}")))?
            .join("ocr-tmp");
        let engine = Engine::in_dir(&resources.join(BUNDLE_DIR), temp, CALL_TIMEOUT);
        let version = engine.version()?; // lazy check: files present and the engine runs
        eprintln!("[cargo-line] OCR engine ready: {version}");
        clear_temp_dir(&engine.temp_dir);
        *cached = Some(engine.clone());
        Ok(engine)
    }

    /// Registers a recognition for `capture_id`, cancelling any earlier one for the same capture.
    fn register(&self, capture_id: i64) -> CancelToken {
        let token = CancelToken::default();
        if let Ok(mut running) = self.running.lock() {
            if let Some(previous) = running.insert(capture_id, token.clone()) {
                previous.cancel();
            }
        }
        token
    }

    fn unregister(&self, capture_id: i64, token: &CancelToken) {
        if let Ok(mut running) = self.running.lock() {
            if running.get(&capture_id).is_some_and(|t| t.same_as(token)) {
                running.remove(&capture_id);
            }
        }
    }

    /// Cancels the running recognition of `capture_id`; returns whether one was running.
    pub fn cancel(&self, capture_id: i64) -> bool {
        self.running
            .lock()
            .ok()
            .and_then(|mut running| running.remove(&capture_id))
            .map(|token| token.cancel())
            .is_some()
    }
}

/// Recognises a stored capture. Blocking: call from a worker thread.
pub fn recognize_capture<R: Runtime>(
    app: &AppHandle<R>,
    capture_id: i64,
) -> Result<OcrResult, OcrError> {
    let state = app.state::<OcrState>();
    let db_state = app.state::<DatabaseState>();
    let db = db_state
        .wait_ready(READY_TIMEOUT)
        .map_err(|e| OcrError::Failed(e.to_string()))?;
    let dir = captures_dir(app).map_err(|e| OcrError::Failed(e.to_string()))?;
    let path = {
        let conn = db
            .connection()
            .map_err(|e| OcrError::Failed(e.to_string()))?;
        storage::file_for_serving(&conn, &dir, capture_id)
            .map_err(|e| OcrError::Failed(e.to_string()))?
            .ok_or_else(|| OcrError::Failed("the screenshot no longer exists".into()))?
    };
    let engine = state.engine(app)?;

    let set_status = |outcome: Option<ProcessingStatus>| {
        let Ok(conn) = db.connection() else { return };
        let result = match outcome {
            None => storage::begin_processing(&conn, capture_id),
            Some(status) => storage::finish_processing(&conn, capture_id, status),
        };
        if let Err(e) = result {
            eprintln!("[cargo-line] could not update the OCR status of capture {capture_id}: {e}");
        }
    };
    set_status(None);
    let token = state.register(capture_id);
    let result = recognize_file(&engine, &path, &token);
    state.unregister(capture_id, &token);
    set_status(Some(match &result {
        Ok(_) => ProcessingStatus::Done,
        Err(OcrError::Cancelled) => ProcessingStatus::Pending,
        Err(_) => ProcessingStatus::Failed,
    }));
    result
}
