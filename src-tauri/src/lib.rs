//! Cargo Line native core.
//!
//! The Rust side owns business logic, local IO and native capabilities,
//! including the SQLite database. The frontend never accesses SQL directly.

pub mod capture;
pub mod commands;
pub mod contracts;
pub mod database;
pub mod gamelog;
pub mod locations;
pub mod ocr;
pub mod parser;
pub mod reference;
pub mod settings;

use database::{Database, DatabaseState, DATABASE_FILE_NAME};
use tauri::{Manager, Runtime};

/// Builds and runs the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(DatabaseState::default())
        .manage(capture::CaptureState::default())
        .manage(ocr::service::OcrState::default())
        .manage(gamelog::service::GameLogState::default())
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
            commands::update_capture_shortcut
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
            let outcome = app
                .path()
                .app_data_dir()
                .map_err(|e| {
                    database::DatabaseError::Unavailable(format!("no app data directory: {e}"))
                })
                .and_then(|dir| Database::open(&dir.join(DATABASE_FILE_NAME)));
            match &outcome {
                Ok(db) => {
                    eprintln!("[cargo-line] database ready at {}", db.path().display());
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
