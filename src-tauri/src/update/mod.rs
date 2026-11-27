//! Update check and in-app update for the installed version.
//!
//! - **Check:** `GET` the GitHub releases list ([`net::API_URL`]), pick the highest newer release
//!   ([`feed::select_offer`]). Automatically about 10 s after the first screen, at most once every
//!   24 h, when "Check for updates automatically" is on; or with "Check now".
//! - **Installed copy:** download `SHA256SUMS.txt` and the setup exe to `<local data>\updates\`,
//!   verify size and SHA-256, then on confirmation close the app and run the installer, which
//!   starts the new version.
//! - **Portable copy / dev build:** a notice and a link to the release page only.
//!
//! [`net`] is the only module that opens network connections.

pub mod feed;
pub mod install;
pub mod net;
pub mod verify;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use semver::Version;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::database::{now_utc_ms, DatabaseState};
use feed::Offer;

/// Event sent to the frontend whenever the update status changes.
pub const STATUS_EVENT: &str = "update-status";
pub const AUTO_CHECK_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;
/// After the first screen (the startup order).
pub const AUTO_CHECK_DELAY: Duration = Duration::from_secs(10);
pub const MAX_INSTALLER_BYTES: u64 = 100 * 1024 * 1024;
const MAX_SUMS_BYTES: u64 = 64 * 1024;
const PARTIAL_SUFFIX: &str = ".partial";
pub const DAMAGED_MESSAGE: &str = "The download was damaged; nothing was installed.";
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether the automatic check is due (pure). A clock that went backwards counts as due.
pub fn auto_check_due(enabled: bool, last_check_ms: Option<i64>, now_ms: i64) -> bool {
    enabled
        && last_check_ms.is_none_or(|last| now_ms < last || now_ms - last >= AUTO_CHECK_INTERVAL_MS)
}

/// The result of the last check, shown in words by the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CheckResult {
    UpToDate { version: String },
    Available { version: String },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum DownloadState {
    Idle,
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    Verifying,
    /// Verified and kept: "Install now" needs no new download.
    Ready {
        version: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone)]
struct ReadyFile {
    path: PathBuf,
    version: String,
    size: u64,
    hash: String,
}

struct Inner {
    checking: bool,
    result: Option<CheckResult>,
    offer: Option<Offer>,
    download: DownloadState,
    ready: Option<ReadyFile>,
}

pub struct UpdateState {
    inner: Mutex<Inner>,
    cancel: Arc<AtomicBool>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                checking: false,
                result: None,
                offer: None,
                download: DownloadState::Idle,
                ready: None,
            }),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// What Settings → Updates and the top-bar marker show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub mode: crate::layout::AppMode,
    /// Download and install in the app (a real installed copy); otherwise a link only.
    pub in_app_install: bool,
    pub automatic: bool,
    pub last_check_ms: Option<i64>,
    pub skipped_version: Option<String>,
    pub checking: bool,
    pub result: Option<CheckResult>,
    pub offer: Option<Offer>,
    /// The top-bar marker: an offer that isn't the skipped version.
    pub marker: bool,
    pub download: DownloadState,
}

fn current_version<R: Runtime>(app: &AppHandle<R>) -> Version {
    let text = app.package_info().version.to_string();
    Version::parse(&text).unwrap_or_else(|_| Version::new(0, 0, 0))
}

fn user_agent<R: Runtime>(app: &AppHandle<R>) -> String {
    format!("Cargo-Line/{}", app.package_info().version)
}

fn feed_url() -> (String, Option<tauri::Url>) {
    match net::feed_override() {
        Some(feed) => (feed.to_string(), Some(feed)),
        None => (net::API_URL.to_string(), None),
    }
}

fn settings_now<R: Runtime>(app: &AppHandle<R>) -> Option<crate::settings::AppSettings> {
    let db = app.state::<DatabaseState>();
    let db = db.wait_ready(READY_TIMEOUT).ok()?;
    let conn = db.connection().ok()?;
    crate::settings::load(&conn).ok()
}

fn save_settings<R: Runtime>(
    app: &AppHandle<R>,
    change: impl FnOnce(&mut crate::settings::AppSettings),
) -> Result<crate::settings::AppSettings, String> {
    let db = app.state::<DatabaseState>();
    let db = db.wait_ready(READY_TIMEOUT).map_err(|e| e.to_string())?;
    let mut conn = db.connection().map_err(|e| e.to_string())?;
    crate::settings::update(&mut conn, change).map_err(|e| e.to_string())
}

/// The current status.
pub fn status<R: Runtime>(app: &AppHandle<R>) -> UpdateStatus {
    let settings = settings_now(app).unwrap_or_default();
    let state = app.state::<UpdateState>();
    let inner = state.inner.lock().unwrap_or_else(|p| p.into_inner());
    let marker = inner.offer.as_ref().is_some_and(|o| {
        feed::marker_visible(&o.version, settings.update_skipped_version.as_deref())
    });
    UpdateStatus {
        current_version: app.package_info().version.to_string(),
        mode: crate::layout::current().mode,
        in_app_install: install::current_allows_install(),
        automatic: settings.check_updates_automatically,
        last_check_ms: settings.update_last_check_ms,
        skipped_version: settings.update_skipped_version.clone(),
        checking: inner.checking,
        result: inner.result.clone(),
        offer: inner.offer.clone(),
        marker,
        download: inner.download.clone(),
    }
}

/// Sends the current status to the frontend (the top-bar marker and Settings listen).
pub fn notify<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit(STATUS_EVENT, status(app));
}

fn with_inner<R: Runtime, T>(app: &AppHandle<R>, f: impl FnOnce(&mut Inner) -> T) -> T {
    let state = app.state::<UpdateState>();
    let mut inner = state.inner.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut inner)
}

/// Asks GitHub (blocking). Records the time of the check, keeps the offer, and notifies.
pub fn check<R: Runtime>(app: &AppHandle<R>) -> CheckResult {
    with_inner(app, |i| i.checking = true);
    notify(app);
    let current = current_version(app);
    let (url, feed) = feed_url();
    let agent = user_agent(app);
    let fetched = net::get_bytes(
        &url,
        feed.as_ref(),
        &net::Get {
            user_agent: &agent,
            accept: "application/vnd.github+json",
            max_bytes: feed::MAX_FEED_BYTES as u64,
            cancel: None,
        },
    );
    let outcome = fetched
        .map_err(|e| match e {
            net::NetError::Busy => "GitHub is busy, try later".to_string(),
            other => other.to_string(),
        })
        .and_then(|body| feed::parse_releases(&body).map_err(|e| e.to_string()));
    let result = match outcome {
        Ok(releases) => match feed::select_offer(&current, &releases) {
            Some(offer) => {
                let version = offer.version.clone();
                with_inner(app, |i| {
                    // A kept download for another version no longer applies.
                    if i.ready.as_ref().is_some_and(|r| r.version != version) {
                        i.ready = None;
                        i.download = DownloadState::Idle;
                    }
                    i.offer = Some(offer);
                });
                CheckResult::Available { version }
            }
            None => {
                with_inner(app, |i| i.offer = None);
                CheckResult::UpToDate {
                    version: current.to_string(),
                }
            }
        },
        Err(reason) => CheckResult::Failed {
            reason: feed::short(&reason),
        },
    };
    let now = now_utc_ms();
    if let Err(e) = save_settings(app, |s| s.update_last_check_ms = Some(now)) {
        eprintln!("[cargo-line] update check time not saved: {e}");
    }
    with_inner(app, |i| {
        i.checking = false;
        i.result = Some(result.clone());
    });
    notify(app);
    result
}

/// Startup work, off the critical path: clears `updates\`, then runs the automatic check about
/// 10 s after the first screen if it is on and due. Failures go to the log only.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let _ = app
                .state::<crate::startup::FirstScreenGate>()
                .wait(crate::startup::FIRST_SCREEN_WAIT);
            if let Ok(local) = crate::local_data_dir(&app) {
                clear_updates_dir(&install::updates_dir(&local));
            }
            std::thread::sleep(AUTO_CHECK_DELAY);
            let Some(settings) = settings_now(&app) else {
                return;
            };
            if !auto_check_due(
                settings.check_updates_automatically,
                settings.update_last_check_ms,
                now_utc_ms(),
            ) {
                return;
            }
            if let CheckResult::Failed { reason } = check(&app) {
                eprintln!("[cargo-line] automatic update check failed: {reason}");
            }
        });
    if let Err(e) = spawned {
        eprintln!("[cargo-line] update check thread not started: {e}");
    }
}

/// Deletes the download folder (leftovers of an earlier update, finished or not).
pub fn clear_updates_dir(dir: &std::path::Path) {
    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(dir) {
            eprintln!("[cargo-line] could not remove the updates folder: {e}");
        }
    }
}

/// The notice after an update attempt, from the version recorded before the installer ran
/// (pure). None when no update was attempted.
pub fn after_update_notice(installing: Option<&str>, running: &str) -> Option<(bool, String)> {
    let target = installing?;
    if target == running {
        Some((true, format!("Updated to version {running}")))
    } else {
        Some((
            false,
            format!("The update to version {target} wasn't installed. Version {running} is still installed."),
        ))
    }
}

/// Called once the database is open: turns a recorded update attempt into the startup notice.
pub fn note_finished_update<R: Runtime>(app: &AppHandle<R>, conn: &mut rusqlite::Connection) {
    let Ok(settings) = crate::settings::load(conn) else {
        return;
    };
    let running = app.package_info().version.to_string();
    let Some((ok, message)) =
        after_update_notice(settings.update_installing_version.as_deref(), &running)
    else {
        return;
    };
    let _ = crate::settings::update(conn, |s| s.update_installing_version = None);
    eprintln!("[cargo-line] {message}");
    if let Ok(mut slot) = app.state::<crate::backup::StartupNoticeState>().0.lock() {
        // A restore notice from the same start wins (it is about the data).
        if slot.is_none() {
            *slot = Some(crate::backup::StartupNotice { ok, message });
        }
    }
}

/// Downloads `SHA256SUMS.txt` and the setup exe of the offer, then verifies it (background thread).
pub fn start_download<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let offer = with_inner(app, |i| i.offer.clone()).ok_or("No update to download.")?;
    let (Some(setup), Some(sums)) = (offer.setup.clone(), offer.sums.clone()) else {
        return Err("This release can't be installed from the app.".into());
    };
    if !install::current_allows_install() {
        return Err("This copy is updated by hand.".into());
    }
    let busy = with_inner(app, |i| {
        matches!(
            i.download,
            DownloadState::Downloading { .. } | DownloadState::Verifying
        )
    });
    if busy {
        return Ok(());
    }
    let dir = install::updates_dir(&crate::local_data_dir(app).map_err(|e| e.to_string())?);
    let cancel = Arc::clone(&app.state::<UpdateState>().cancel);
    cancel.store(false, Ordering::Relaxed);
    with_inner(app, |i| {
        i.ready = None;
        i.download = DownloadState::Downloading {
            received: 0,
            total: Some(setup.size),
        };
    });
    notify(app);
    let app = app.clone();
    std::thread::Builder::new()
        .name("update-download".into())
        .spawn(move || {
            let result = download_and_verify(&app, &dir, &offer.version, &setup, &sums, &cancel);
            let state = match result {
                Ok(ready) => {
                    let version = ready.version.clone();
                    with_inner(&app, |i| i.ready = Some(ready));
                    DownloadState::Ready { version }
                }
                Err(message) => DownloadState::Failed { message },
            };
            with_inner(&app, |i| i.download = state);
            notify(&app);
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn cancel_download<R: Runtime>(app: &AppHandle<R>) {
    app.state::<UpdateState>()
        .cancel
        .store(true, Ordering::Relaxed);
}

/// Writes to `<name>.partial`, renames when complete. A partial file never counts as complete.
pub fn download_to_file(
    dir: &std::path::Path,
    name: &str,
    write: impl FnOnce(&mut std::fs::File) -> Result<(), String>,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let final_path = dir.join(name);
    let partial = dir.join(format!("{name}{PARTIAL_SUFFIX}"));
    let _ = std::fs::remove_file(&final_path);
    let mut file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let written = write(&mut file).and_then(|_| file.sync_all().map_err(|e| e.to_string()));
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    std::fs::rename(&partial, &final_path).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        e.to_string()
    })?;
    Ok(final_path)
}

fn download_and_verify<R: Runtime>(
    app: &AppHandle<R>,
    dir: &std::path::Path,
    version: &str,
    setup: &feed::Asset,
    sums: &feed::Asset,
    cancel: &AtomicBool,
) -> Result<ReadyFile, String> {
    let feed = net::feed_override();
    let agent = user_agent(app);
    let reason = |e: net::NetError| match e {
        net::NetError::Cancelled => "Download cancelled.".to_string(),
        net::NetError::Busy => "Couldn't reach GitHub: GitHub is busy, try later".to_string(),
        other => format!("Couldn't download the update: {other}"),
    };
    let sums_text = net::get_bytes(
        &sums.url,
        feed.as_ref(),
        &net::Get {
            user_agent: &agent,
            accept: "application/octet-stream",
            max_bytes: MAX_SUMS_BYTES,
            cancel: Some(cancel),
        },
    )
    .map_err(reason)?;
    let expected = verify::expected_hash(&String::from_utf8_lossy(&sums_text), &setup.name)
        .ok_or_else(|| {
            format!("{DAMAGED_MESSAGE} (SHA256SUMS.txt has no line for the installer)")
        })?;
    if setup.size == 0 || setup.size > MAX_INSTALLER_BYTES {
        return Err("The installer is larger than 100 MB; nothing was downloaded.".into());
    }
    let mut last_emit = Instant::now();
    let path = download_to_file(dir, &setup.name, |file| {
        use std::io::Write;
        let mut received = 0u64;
        net::get(
            &setup.url,
            feed.as_ref(),
            &net::Get {
                user_agent: &agent,
                accept: "application/octet-stream",
                max_bytes: MAX_INSTALLER_BYTES,
                cancel: Some(cancel),
            },
            &mut |chunk, total| {
                file.write_all(chunk)
                    .map_err(|e| net::NetError::Connection(e.to_string()))?;
                received += chunk.len() as u64;
                if last_emit.elapsed() >= Duration::from_millis(250) {
                    last_emit = Instant::now();
                    with_inner(app, |i| {
                        i.download = DownloadState::Downloading {
                            received,
                            total: total.or(Some(setup.size)),
                        }
                    });
                    notify(app);
                }
                Ok(())
            },
        )
        .map(|_| ())
        .map_err(reason)
    })?;
    with_inner(app, |i| i.download = DownloadState::Verifying);
    notify(app);
    verify::verify_file(&path, setup.size, &expected).map_err(|e| match e {
        verify::VerifyError::Io(m) => format!("The installer couldn't be checked: {m}"),
        _ => DAMAGED_MESSAGE.to_string(),
    })?;
    Ok(ReadyFile {
        path,
        version: version.to_string(),
        size: setup.size,
        hash: expected,
    })
}

/// Closes the app and runs the verified installer. Refuses while a capture or OCR reading runs.
pub fn install_now<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if !install::current_allows_install() {
        return Err("This copy is updated by hand.".into());
    }
    let ready = with_inner(app, |i| i.ready.clone()).ok_or("Download the update first.")?;
    if let Err(e) = verify::verify_file(&ready.path, ready.size, &ready.hash) {
        with_inner(app, |i| {
            i.ready = None;
            i.download = DownloadState::Failed {
                message: match e {
                    verify::VerifyError::Io(m) => format!("The installer couldn't be checked: {m}"),
                    _ => DAMAGED_MESSAGE.to_string(),
                },
            };
        });
        notify(app);
        return Err(DAMAGED_MESSAGE.into());
    }
    if app.state::<crate::capture::CaptureState>().is_busy()
        || app.state::<crate::ocr::service::OcrState>().is_busy()
    {
        return Err("A screenshot is being taken or read. Try again when it has finished.".into());
    }
    let exe_dir = crate::layout::current()
        .exe_dir
        .clone()
        .ok_or("The program folder is unknown.")?;
    let arguments = install::installer_arguments(&exe_dir, &install::forwarded_now())?;
    // Recorded first, so the next start can say whether the update went in.
    save_settings(app, |s| {
        s.update_installing_version = Some(ready.version.clone())
    })?;
    // No more Game.log reads; the capture cleanup and every command now wait on the database.
    app.state::<crate::gamelog::service::GameLogState>()
        .configure(crate::gamelog::service::WatchConfig::default());
    let db = app.state::<DatabaseState>();
    let db = db.wait_ready(READY_TIMEOUT).map_err(|e| e.to_string())?;
    let conn = db.connection().map_err(|e| e.to_string())?;
    // Like a clean close: everything in the WAL goes into the database file.
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
        eprintln!("[cargo-line] checkpoint before the update: {e}");
    }
    eprintln!(
        "[cargo-line] starting the installer: \"{}\" {arguments}",
        ready.path.display()
    );
    if let Err(e) = install::spawn_installer(&ready.path, &arguments) {
        drop(conn);
        let _ = save_settings(app, |s| s.update_installing_version = None);
        return Err(format!("The installer couldn't be started: {e}"));
    }
    // The connection stays locked until the process ends: nothing writes after the checkpoint.
    std::mem::forget(conn);
    app.exit(0);
    Ok(())
}

/// The release page link for "Open download page" and the full notes (allow-listed pattern:
/// `https://github.com/MathisVkg/cargoline/releases/tag/v<semver>`).
pub fn release_page<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let url = with_inner(app, |i| i.offer.as_ref().map(|o| o.html_url.clone()))?;
    release_page_allowed(&url).then_some(url)
}

pub fn release_page_allowed(url: &str) -> bool {
    url.strip_prefix("https://github.com/MathisVkg/cargoline/releases/tag/")
        .and_then(feed::tag_version)
        .is_some()
}
