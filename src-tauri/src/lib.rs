//! Cargo Line native core.
//!
//! The Rust side owns business logic, local IO and native capabilities,
//! including the SQLite database. The frontend never accesses SQL directly.

pub mod backup;
pub mod capture;
pub mod commands;
pub mod contracts;
pub mod database;
pub mod gamelog;
pub mod history;
pub mod layout;
pub mod locations;
pub mod ocr;
pub mod parser;
pub mod reference;
pub mod settings;
pub mod ships;
pub mod startup;
pub mod trace;
pub mod update;

use database::{Database, DatabaseState, DATABASE_FILE_NAME};
use tauri::{Manager, Runtime};

/// Debug builds only: `CARGO_LINE_DEV_DATA_DIR` replaces the app data folders (database,
/// captures, OCR temp files) so a desktop test session never touches the real data. Release
/// builds ignore it.
pub const DEV_DATA_DIR_ENV: &str = "CARGO_LINE_DEV_DATA_DIR";

/// Any build (Task 18, for measuring a release build): `CARGO_LINE_DATA_DIR` replaces the app
/// data folders the same way. It must be an **absolute** path; anything else is ignored. Nothing
/// happens unless the variable is set explicitly.
pub const DATA_DIR_ENV: &str = "CARGO_LINE_DATA_DIR";

/// The data folder override, if any: `CARGO_LINE_DATA_DIR` (absolute paths only, any build),
/// then `CARGO_LINE_DEV_DATA_DIR` (debug builds only).
pub fn data_dir_override(
    debug_build: bool,
    data_dir: Option<std::ffi::OsString>,
    dev_data_dir: Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    let explicit = data_dir
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute());
    explicit.or_else(|| {
        dev_data_dir
            .filter(|v| debug_build && !v.is_empty())
            .map(std::path::PathBuf::from)
    })
}

/// The roaming app data folder (database, captures, backups' safety copies, the trace): the
/// override, the portable `data\` folder, or `%APPDATA%\io.github.mathisvkg.sccargoapp`
/// (see [`layout`]).
pub fn data_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<std::path::PathBuf> {
    match layout::current().data.path() {
        Some(dir) => Ok(dir.to_path_buf()),
        None => app.path().app_data_dir(),
    }
}

/// The local app data folder (OCR temp files): `<override or portable folder>\local`, or
/// `%LOCALAPPDATA%\io.github.mathisvkg.sccargoapp`.
pub fn local_data_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<std::path::PathBuf> {
    match layout::current().data.path() {
        Some(dir) => Ok(dir.join("local")),
        None => app.path().app_local_data_dir(),
    }
}

/// Builds and runs the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Test overrides passed on by an in-app update's restart (`--env=NAME=VALUE`, Task 20).
    update::install::apply_forwarded_env();
    trace::init();
    trace::mark("main entered");
    // Portable copy: refuse a read-only or temporary folder, and keep WebView2's profile in
    // `data\webview\`. This has to happen before Tauri creates the window.
    if let Err(problem) = layout::prepare_process() {
        layout::show_fatal(&layout::portable_problem_message(&problem));
        std::process::exit(1);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(DatabaseState::default())
        .manage(capture::CaptureState::default())
        .manage(ocr::service::OcrState::default())
        .manage(gamelog::service::GameLogState::default())
        .manage(backup::StartupNoticeState::default())
        .manage(startup::FirstScreenGate::default())
        .manage(update::UpdateState::default())
        .register_asynchronous_uri_scheme_protocol("capture", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || {
                responder.respond(capture::service::serve_capture(&app, &path))
            });
        })
        .on_page_load(|_, payload| match payload.event() {
            tauri::webview::PageLoadEvent::Started => trace::mark_once("page load started"),
            tauri::webview::PageLoadEvent::Finished => trace::mark_once("page load finished"),
        })
        .setup(|app| {
            trace::mark("setup begin");
            if app.get_webview_window("main").is_some() {
                trace::mark("window exists at setup");
            }
            start_database(app.handle());
            capture::service::start_cleanup(app.handle());
            gamelog::service::start(app.handle());
            update::start(app.handle());
            trace::mark("setup end");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_contract,
            commands::list_active_contracts,
            commands::get_contract_grouping,
            commands::set_contract_grouping,
            commands::list_locations,
            commands::list_used_commodities,
            commands::list_reference_locations,
            commands::list_reference_commodities,
            commands::complete_contract,
            commands::abandon_contract,
            commands::finalize_active_contracts,
            commands::update_contract,
            commands::set_stop_reminder,
            commands::get_settings,
            commands::update_capture_settings,
            commands::capture_screenshot,
            commands::discard_capture,
            commands::recognize_capture,
            commands::cancel_recognition,
            commands::read_contract_from_capture,
            commands::update_game_log_settings,
            commands::get_game_log_status,
            commands::detect_game_log_path,
            commands::game_log_browse_start,
            commands::update_capture_shortcut,
            commands::reactivate_contracts,
            commands::get_history,
            commands::restore_contracts,
            commands::clear_history,
            commands::list_ships,
            commands::get_selected_ship,
            commands::set_selected_ship,
            commands::set_ship_favorite,
            commands::get_ship_catalog_info,
            commands::create_custom_ship,
            commands::update_custom_ship,
            commands::delete_custom_ship,
            commands::get_data_info,
            commands::open_data_folder,
            commands::export_backup,
            commands::inspect_backup,
            commands::restore_backup,
            commands::take_startup_notice,
            commands::open_link,
            commands::get_about_info,
            commands::trace_marks,
            commands::get_update_status,
            commands::check_for_updates,
            commands::set_update_automatic,
            commands::skip_update_version,
            commands::download_update,
            commands::cancel_update_download,
            commands::install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running Cargo Line");
}

/// Opens and migrates the database on a background thread so the UI thread
/// is never blocked. The outcome is stored in [`DatabaseState`]; failures are
/// reported clearly and never cause an existing database to be reset.
fn start_database<R: Runtime>(app: &tauri::AppHandle<R>) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("database-init".into())
        .spawn(move || {
            trace::mark("database thread start");
            match &layout::current().data {
                layout::DataDir::Override(dir) => {
                    eprintln!("[cargo-line] data folder override: {}", dir.display())
                }
                layout::DataDir::Portable(dir) => {
                    eprintln!("[cargo-line] portable data folder: {}", dir.display())
                }
                layout::DataDir::Default => {}
            }
            let outcome = data_dir(&app)
                .map_err(|e| {
                    database::DatabaseError::Unavailable(format!("no app data directory: {e}"))
                })
                .and_then(|dir| {
                    trace::set_output_dir(&dir);
                    // A restore staged before the restart is swapped in before the database
                    // is opened (Task 17). On failure the current database is kept.
                    let db_file = dir.join(DATABASE_FILE_NAME);
                    let restored = backup::apply_pending(&dir, &db_file);
                    if let Some((ok, message)) = backup::restore_notice(&restored) {
                        eprintln!(
                            "[cargo-line] restore at startup: {}",
                            if ok { "done" } else { "failed" }
                        );
                        if let Ok(mut slot) = app.state::<backup::StartupNoticeState>().0.lock() {
                            *slot = Some(backup::StartupNotice { ok, message });
                        }
                    }
                    trace::mark("pending restore checked");
                    Database::open(&db_file)
                });
            match &outcome {
                Ok(db) => {
                    trace::mark("database opened, migrations done");
                    eprintln!("[cargo-line] database ready at {}", db.path().display());
                    // The bundled ship catalog is upserted before commands can read it
                    // (idempotent; ships that left the list are hidden, never deleted). When
                    // the stored version is the bundled one, it is only checked (Task 18).
                    match ships::bundled().and_then(|list| {
                        let mut conn = db.connection().map_err(|e| e.to_string())?;
                        ships::sync_catalog_if_changed(&mut conn, list, database::now_utc_ms())
                            .map_err(|e| e.to_string())
                    }) {
                        Ok(Some(r)) => {
                            trace::mark("ship catalog sync done (written)");
                            if r.upserted > 0 || r.removed > 0 {
                                eprintln!(
                                    "[cargo-line] ship catalog synced: {} updated, {} removed",
                                    r.upserted, r.removed
                                );
                            }
                        }
                        Ok(None) => trace::mark("ship catalog sync done (up to date)"),
                        Err(e) => eprintln!("[cargo-line] ship catalog not loaded: {e}"),
                    }
                }
                Err(e) => eprintln!("[cargo-line] database initialization failed: {e}"),
            }
            // Commands can run from here; the watcher configuration below only reads settings.
            app.state::<DatabaseState>().set(outcome);
            trace::mark("database ready for commands");
            if let Ok(db) = app.state::<DatabaseState>().get() {
                // After an in-app update: "Updated to version X" (Task 20).
                if let Ok(mut conn) = db.connection() {
                    update::note_finished_update(&app, &mut conn);
                }
                // Game.log monitoring starts only if it was switched on in the settings.
                if let Ok(saved) = db
                    .connection()
                    .map_err(|e| e.to_string())
                    .and_then(|c| settings::load(&c).map_err(|e| e.to_string()))
                {
                    gamelog::service::configure_from_settings(
                        &app.state::<gamelog::service::GameLogState>(),
                        &saved,
                    );
                }
                trace::mark("watcher configured from settings");
            }
        })
        .expect("failed to spawn the database initialization thread");
}
