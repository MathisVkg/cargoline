//! Local SQLite storage, owned entirely by the Rust backend.
//!
//! The frontend never executes SQL; future Tauri commands expose validated
//! operations only.

pub mod connection;
pub mod migrations;
pub mod models;

use std::path::PathBuf;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use connection::Database;

/// File name of the application database inside the app data directory.
///
/// Deliberately distinct from `cargo-line.sqlite3`, a pre-existing file in the
/// same directory left by an earlier, unrelated build; that file is never used.
pub const DATABASE_FILE_NAME: &str = "sc-cargo-app.sqlite3";

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("could not create the data directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not open the database at {path}: {source}")]
    Open {
        path: PathBuf,
        source: rusqlite::Error,
    },
    #[error("could not configure the database connection: {0}")]
    Configure(rusqlite::Error),
    #[error(
        "the file at {path} is not a Cargo Line database (it may belong to another program \
         or an older prototype); it was left unchanged"
    )]
    NotCargoLineDatabase { path: PathBuf },
    #[error("the SQLite build does not support {0}")]
    Unsupported(&'static str),
    #[error(
        "the database at {path} uses schema version {found}, but this version of Cargo Line \
         only supports up to version {supported}; the file was left unchanged"
    )]
    SchemaTooNew {
        path: PathBuf,
        found: i64,
        supported: i64,
    },
    #[error("database migration failed and was rolled back: {0}")]
    Migration(rusqlite_migration::Error),
    #[error("the database connection lock was poisoned by an earlier panic")]
    LockPoisoned,
    #[error("the database is not available: {0}")]
    Unavailable(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// Tauri-managed handle to the database, filled in once background
/// initialization has finished (successfully or not).
#[derive(Default)]
pub struct DatabaseState {
    slot: OnceLock<Result<Database, String>>,
    ready: (Mutex<()>, Condvar),
}

impl DatabaseState {
    /// Records the outcome of initialization and wakes any waiters.
    /// Later calls are ignored.
    pub fn set(&self, outcome: Result<Database, DatabaseError>) {
        let _ = self.slot.set(outcome.map_err(|e| e.to_string()));
        let _guard = self.ready.0.lock();
        self.ready.1.notify_all();
    }

    /// Like [`Self::get`], but first waits up to `timeout` for a still-running
    /// initialization to finish. Call from a blocking worker thread only.
    pub fn wait_ready(&self, timeout: Duration) -> Result<&Database, DatabaseError> {
        if self.slot.get().is_none() {
            if let Ok(guard) = self.ready.0.lock() {
                let _ = self
                    .ready
                    .1
                    .wait_timeout_while(guard, timeout, |_| self.slot.get().is_none());
            }
        }
        self.get()
    }

    /// The database, or a clear reason why it is unavailable.
    pub fn get(&self) -> Result<&Database, DatabaseError> {
        match self.slot.get() {
            Some(Ok(db)) => Ok(db),
            Some(Err(reason)) => Err(DatabaseError::Unavailable(reason.clone())),
            None => Err(DatabaseError::Unavailable(
                "initialization is still running".into(),
            )),
        }
    }
}

/// Current time as UTC milliseconds since the Unix epoch.
pub fn now_utc_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
