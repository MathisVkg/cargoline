//! Runs one capture for the app: reads the source setting, captures on a dedicated
//! thread with an overall timeout, stores the PNG and its row, and replaces the form's
//! previous unsaved capture. Also the startup / hourly cleanup and the `capture:` image
//! protocol used by the preview.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use tauri::http::{header, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime};

use super::storage::{self, CaptureInfo, CAPTURES_DIR_NAME};
use super::{CaptureError, CaptureSlot, CaptureState, Frame};
use crate::database::{now_utc_ms, DatabaseState};
use crate::settings::{self, CaptureSource};

/// Upper bound for the native part (window lookup, WGC frame, PrintWindow fallback).
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);
/// How often expired and orphan captures are removed while the app runs.
pub const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// The cleanup thread starts with the app: give a slow first database open more time.
const CLEANUP_READY_TIMEOUT: Duration = Duration::from_secs(120);

/// Errors of a whole capture request.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Capture(#[from] CaptureError),
    #[error(transparent)]
    Database(#[from] crate::database::DatabaseError),
    #[error(transparent)]
    Settings(#[from] settings::SettingsError),
    #[error(transparent)]
    Storage(#[from] storage::StorageError),
    #[error("no app data directory: {0}")]
    NoDataDir(String),
}

pub fn captures_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, ServiceError> {
    crate::data_dir(app)
        .map(|dir| dir.join(CAPTURES_DIR_NAME))
        .map_err(|e| ServiceError::NoDataDir(e.to_string()))
}

/// Captures, stores and returns the new capture. Blocking: call from a worker thread.
/// `replaces` is the form's previous unsaved capture, deleted once the new one is stored.
pub fn capture_and_store<R: Runtime>(
    app: &AppHandle<R>,
    replaces: Option<i64>,
) -> Result<CaptureInfo, ServiceError> {
    let slot = app.state::<CaptureState>().try_begin()?;
    let state = app.state::<DatabaseState>();
    let db = state.wait_ready(READY_TIMEOUT)?;
    let source = settings::load(&*db.connection()?)?.capture_source; // lock released here
    let (frame, _slot) = capture_frame(app, source, slot)?;

    let dir = captures_dir(app)?;
    let conn = db.connection()?;
    let info = storage::store(&conn, &dir, &frame, now_utc_ms())?;
    if let Some(old) = replaces.filter(|old| *old != info.id) {
        if let Err(e) = storage::discard_unlinked(&conn, &dir, old) {
            eprintln!("[cargo-line] could not delete the replaced capture {old}: {e}");
        }
    }
    Ok(info)
}

/// Deletes an unsaved capture (form cancelled, or a late result after closing).
pub fn discard<R: Runtime>(app: &AppHandle<R>, id: i64) -> Result<bool, ServiceError> {
    let state = app.state::<DatabaseState>();
    let db = state.wait_ready(READY_TIMEOUT)?;
    let dir = captures_dir(app)?;
    let conn = db.connection()?;
    Ok(storage::discard_unlinked(&conn, &dir, id)?)
}

/// Captures on a dedicated thread (WinRT initialised there) and waits at most
/// [`CAPTURE_TIMEOUT`]. On timeout the thread keeps the slot until it ends, so no
/// second capture can overlap a stuck one.
#[cfg(windows)]
fn capture_frame<R: Runtime>(
    app: &AppHandle<R>,
    source: CaptureSource,
    slot: CaptureSlot,
) -> Result<(Frame, CaptureSlot), CaptureError> {
    use super::native;

    // Full display: hide Cargo Line from capture only for this capture.
    let excluded = match source {
        CaptureSource::FullDisplay => Some(exclude_main_window(app)?),
        CaptureSource::StarCitizenWindow => None,
    };
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("screen-capture".into())
        .spawn(move || {
            let _winrt = native::WinRtThread::init();
            let result = match source {
                CaptureSource::StarCitizenWindow => native::capture_star_citizen_window(),
                CaptureSource::FullDisplay => {
                    // Let DWM compose one frame without Cargo Line first.
                    std::thread::sleep(Duration::from_millis(100));
                    native::capture_display()
                }
            };
            let _ = tx.send((result, slot));
        });
    let outcome = match spawned {
        Ok(_) => rx
            .recv_timeout(CAPTURE_TIMEOUT)
            .map_err(|_| CaptureError::Timeout),
        Err(e) => Err(CaptureError::Failed(format!(
            "could not start the capture thread: {e}"
        ))),
    };
    if let Some((window, previous)) = excluded {
        run_on_main(app, move || native::restore_affinity(window, previous));
    }
    let (result, slot) = outcome?;
    result.map(|frame| (frame, slot))
}

#[cfg(not(windows))]
fn capture_frame<R: Runtime>(
    _app: &AppHandle<R>,
    _source: CaptureSource,
    _slot: CaptureSlot,
) -> Result<(Frame, CaptureSlot), CaptureError> {
    Err(CaptureError::Unsupported)
}

/// Sets `WDA_EXCLUDEFROMCAPTURE` on Cargo Line's main window (on the UI thread, which
/// owns it) and returns what to restore.
#[cfg(windows)]
fn exclude_main_window<R: Runtime>(app: &AppHandle<R>) -> Result<(isize, u32), CaptureError> {
    let window = app
        .get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map(|hwnd| hwnd.0 as isize)
        .ok_or_else(|| CaptureError::Failed("Cargo Line's window was not found".into()))?;
    run_on_main(app, move || super::native::exclude_from_capture(window))
        .ok_or_else(|| CaptureError::Failed("could not reach the UI thread".into()))?
        .map(|previous| (window, previous))
}

/// Runs `job` on the UI thread and waits briefly for its result.
#[cfg(windows)]
fn run_on_main<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    job: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(job());
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(2)).ok()
}

/// Startup pass (also removes unsaved captures left by an earlier session), then hourly.
/// Never panics; problems are logged.
pub fn start_cleanup<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("capture-cleanup".into())
        .spawn(move || {
            let mut startup = true;
            loop {
                match (
                    app.state::<DatabaseState>()
                        .wait_ready(CLEANUP_READY_TIMEOUT),
                    captures_dir(&app),
                ) {
                    (Ok(db), Ok(dir)) => match db.connection() {
                        Ok(conn) => {
                            let report = storage::cleanup(&conn, &dir, now_utc_ms(), startup);
                        if startup {
                            if let Err(e) = storage::fail_interrupted(&conn) {
                                eprintln!("[cargo-line] could not reset interrupted OCR states: {e}");
                            }
                        }
                            drop(conn);
                            let removed = report.expired
                                + report.unlinked
                                + report.missing_files
                                + report.orphan_files;
                            if removed > 0 || !report.errors.is_empty() {
                                eprintln!("[cargo-line] capture cleanup: {report:?}");
                            }
                            startup = false;
                        }
                        Err(e) => eprintln!("[cargo-line] capture cleanup skipped: {e}"),
                    },
                    (Err(e), _) => eprintln!("[cargo-line] capture cleanup skipped: {e}"),
                    (_, Err(e)) => eprintln!("[cargo-line] capture cleanup skipped: {e}"),
                }
                std::thread::sleep(CLEANUP_INTERVAL);
            }
        });
    if let Err(e) = spawned {
        eprintln!("[cargo-line] could not start capture cleanup: {e}");
    }
}

/// `capture://localhost/<id>` (WebView2: `http://capture.localhost/<id>`): serves a stored
/// capture's PNG. Only ids are accepted, and only files inside `captures\` are read.
pub fn serve_capture<R: Runtime>(app: &AppHandle<R>, path: &str) -> Response<Vec<u8>> {
    let not_found = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .expect("static response")
    };
    let Ok(id) = path.trim_start_matches('/').parse::<i64>() else {
        return not_found();
    };
    let file = (|| {
        let state = app.state::<DatabaseState>();
        let db = state.wait_ready(READY_TIMEOUT).ok()?;
        let dir = captures_dir(app).ok()?;
        let conn = db.connection().ok()?;
        storage::file_for_serving(&conn, &dir, id).ok().flatten()
    })();
    match file.and_then(|f| std::fs::read(f).ok()) {
        Some(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CACHE_CONTROL, "no-store")
            .body(bytes)
            .unwrap_or_else(|_| not_found()),
        None => not_found(),
    }
}
