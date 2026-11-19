//! Cargo Line native core.
//!
//! The Rust side owns business logic, local IO and native capabilities,
//! including the SQLite database. The frontend never accesses SQL directly.

pub mod catalog;
pub mod commands;
pub mod contracts;
pub mod database;
pub mod locations;
pub mod settings;

use database::{Database, DatabaseState, DATABASE_FILE_NAME};
use tauri::{Manager, Runtime};

/// Builds and runs the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(DatabaseState::default())
        .setup(|app| {
            start_database(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_contract,
            commands::list_active_contracts,
            commands::get_contract_grouping,
            commands::set_contract_grouping,
            commands::list_locations,
            commands::list_commodities,
            commands::get_reference_catalog_info
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
                    sync_bundled_catalog(db);
                }
                Err(e) => eprintln!("[cargo-line] database initialization failed: {e}"),
            }
            app.state::<DatabaseState>().set(outcome);
        })
        .expect("failed to spawn the database initialization thread");
}

/// Applies the embedded reference snapshot when its version changed. Runs
/// before the database is handed to commands. A failure is logged and the
/// previous catalog is kept; contracts keep working either way.
fn sync_bundled_catalog(db: &Database) {
    let result = catalog::snapshot::Snapshot::embedded()
        .map_err(|e| e.to_string())
        .and_then(|snapshot| {
            let mut conn = db.connection().map_err(|e| e.to_string())?;
            catalog::sync::sync_reference_catalog(&mut conn, &snapshot, database::now_utc_ms())
                .map_err(|e| e.to_string())
        });
    match result {
        Ok(catalog::sync::SyncOutcome::UpToDate) => {}
        Ok(catalog::sync::SyncOutcome::Applied(stats)) => {
            eprintln!("[cargo-line] reference catalog synced: {stats:?}")
        }
        Err(e) => {
            eprintln!("[cargo-line] reference catalog sync failed, keeping previous catalog: {e}")
        }
    }
}
