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
pub mod locations;
pub mod ocr;
pub mod parser;
pub mod reference;
pub mod settings;
pub mod ships;

use database::{Database, DatabaseState, DATABASE_FILE_NAME};
use tauri::{Manager, Runtime};

/// Debug builds only: `CARGO_LINE_DEV_DATA_DIR` replaces the app data folders (database,
/// captures, OCR temp files) so a desktop test session never touches the real data. Release
/// builds ignore it.
pub const DEV_DATA_DIR_ENV: &str = "CARGO_LINE_DEV_DATA_DIR";

fn dev_data_dir() -> Option<std::path::PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os(DEV_DATA_DIR_ENV)
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
}

/// The roaming app data folder (database, captures), or the dev override.
pub fn data_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<std::path::PathBuf> {
    match dev_data_dir() {
        Some(dir) => Ok(dir),
        None => app.path().app_data_dir(),
    }
}

/// The local app data folder (OCR temp files), or `<dev override>/local`.
pub fn local_data_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<std::path::PathBuf> {
    match dev_data_dir() {
        Some(dir) => Ok(dir.join("local")),
        None => app.path().app_local_data_dir(),
    }
}

/// Builds and runs the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(DatabaseState::default())
        .manage(capture::CaptureState::default())
        .manage(ocr::service::OcrState::default())
        .manage(gamelog::service::GameLogState::default())
        .manage(backup::StartupNoticeState::default())
        .register_asynchronous_uri_scheme_protocol("capture", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || {
                responder.respond(capture::service::serve_capture(&app, &path))
            });
        })
        .setup(|app| {
            start_database(app.handle());
            capture::service::start_cleanup(app.handle());
            gamelog::service::start(app.handle());
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
            commands::get_about_info
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
            if let Some(dir) = dev_data_dir() {
                eprintln!("[cargo-line] dev data folder override: {}", dir.display());
            }
            let outcome = data_dir(&app)
                .map_err(|e| {
                    database::DatabaseError::Unavailable(format!("no app data directory: {e}"))
                })
                .and_then(|dir| {
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
                    Database::open(&db_file)
                });
            match &outcome {
                Ok(db) => {
                    eprintln!("[cargo-line] database ready at {}", db.path().display());
                    // The bundled ship catalog is upserted before commands can read it
                    // (idempotent; ships that left the list are hidden, never deleted).
                    match ships::bundled().and_then(|list| {
                        let mut conn = db.connection().map_err(|e| e.to_string())?;
                        ships::sync_catalog(&mut conn, list, database::now_utc_ms())
                            .map_err(|e| e.to_string())
                    }) {
                        Ok(r) if r.upserted > 0 || r.removed > 0 => eprintln!(
                            "[cargo-line] ship catalog synced: {} updated, {} removed",
                            r.upserted, r.removed
                        ),
                        Ok(_) => {}
                        Err(e) => eprintln!("[cargo-line] ship catalog not loaded: {e}"),
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
                }
                Err(e) => eprintln!("[cargo-line] database initialization failed: {e}"),
            }
            app.state::<DatabaseState>().set(outcome);
        })
        .expect("failed to spawn the database initialization thread");
}
