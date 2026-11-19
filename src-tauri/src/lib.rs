//! Cargo Line native core.
//!
//! The Rust side owns business logic, local IO and native capabilities,
//! including the SQLite database. The frontend never accesses SQL directly.

pub mod capture;
pub mod commands;
pub mod contracts;
pub mod database;
pub mod locations;
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
            commands::discard_capture
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
                Ok(db) => eprintln!("[cargo-line] database ready at {}", db.path().display()),
                Err(e) => eprintln!("[cargo-line] database initialization failed: {e}"),
            }
            app.state::<DatabaseState>().set(outcome);
        })
        .expect("failed to spawn the database initialization thread");
}
