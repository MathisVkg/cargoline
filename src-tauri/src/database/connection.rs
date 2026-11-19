//! Opening, configuring and migrating the SQLite database.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use super::migrations::{migrations, LATEST_SCHEMA_VERSION};
use super::DatabaseError;

/// How long a connection waits for a competing lock before failing with `SQLITE_BUSY`.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The application database: one serialized connection owned by Rust.
///
/// Callers run database work off the UI thread (e.g. inside
/// `tauri::async_runtime::spawn_blocking`) and hold the lock only briefly.
pub struct Database {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl Database {
    /// Opens (creating if needed) the database at `path`, configures the
    /// connection and applies any pending migrations.
    ///
    /// An existing database is never reset or overwritten: if the file is not
    /// a Cargo Line database, or was created by a newer version of the
    /// application, opening fails and the file is left untouched.
    pub fn open(path: &Path) -> Result<Self, DatabaseError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| DatabaseError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let mut conn = open_connection(path)?;
        migrate(&mut conn, path)?;

        Ok(Self {
            path: path.to_path_buf(),
            conn: Mutex::new(conn),
        })
    }

    /// Location of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Exclusive access to the connection.
    pub fn connection(&self) -> Result<MutexGuard<'_, Connection>, DatabaseError> {
        self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)
    }
}

/// SQLite `application_id` header value marking a Cargo Line database
/// (0x43474C4E, "CGLN"). Set by migration 0001.
pub const APPLICATION_ID: i32 = 0x4347_4C4E;

/// Opens a configured connection to a Cargo Line (or new, empty) database
/// file without running migrations.
pub fn open_connection(path: &Path) -> Result<Connection, DatabaseError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;
    let conn = Connection::open_with_flags(path, flags).map_err(|source| DatabaseError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(DatabaseError::Configure)?;
    // Must run before anything that writes to the file (e.g. enabling WAL).
    check_identity(&conn, path)?;
    configure(&conn)?;
    Ok(conn)
}

/// Accepts only Cargo Line databases or brand-new empty files, so a foreign
/// SQLite file at the same location is never modified.
fn check_identity(conn: &Connection, path: &Path) -> Result<(), DatabaseError> {
    let application_id: i32 = conn
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(|source| DatabaseError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    if application_id == APPLICATION_ID {
        return Ok(());
    }
    let objects: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_schema", [], |row| row.get(0))
        .map_err(DatabaseError::Configure)?;
    if application_id == 0 && objects == 0 {
        return Ok(());
    }
    Err(DatabaseError::NotCargoLineDatabase {
        path: path.to_path_buf(),
    })
}

/// Applies the per-connection settings every Cargo Line connection needs.
fn configure(conn: &Connection) -> Result<(), DatabaseError> {
    // Foreign keys are off by default in SQLite and must be enabled per connection.
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(DatabaseError::Configure)?;
    let fk_enabled: bool = conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(DatabaseError::Configure)?;
    if !fk_enabled {
        return Err(DatabaseError::Unsupported("foreign key enforcement"));
    }

    let journal_mode: String = conn
        .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
        .map_err(DatabaseError::Configure)?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(DatabaseError::Unsupported("write-ahead logging (WAL)"));
    }

    // NORMAL is durable against application crashes in WAL mode.
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(DatabaseError::Configure)?;
    Ok(())
}

fn migrate(conn: &mut Connection, path: &Path) -> Result<(), DatabaseError> {
    let found: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(DatabaseError::Configure)?;
    if found > LATEST_SCHEMA_VERSION {
        return Err(DatabaseError::SchemaTooNew {
            path: path.to_path_buf(),
            found,
            supported: LATEST_SCHEMA_VERSION,
        });
    }
    migrations()
        .to_latest(conn)
        .map_err(DatabaseError::Migration)
}
