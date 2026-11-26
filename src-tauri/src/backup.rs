//! Backup and restore of the whole database (Task 17).
//!
//! - **Export:** `VACUUM INTO` a temporary file next to the destination, then rename it into
//!   place, then re-validate the written file (integrity, `application_id`, schema version). A
//!   consistent copy is made while the app runs. Screenshot *files* are never included (their rows
//!   may be; the existing cleanup drops rows whose file is missing).
//! - **Validation** ([`inspect`]): the chosen file is first copied to a scratch file, and only the
//!   copy is opened, so the user's file is never modified. Refused: not SQLite, not Cargo Line
//!   (`application_id`), a newer schema, a failed `integrity_check`. Older schemas are accepted and
//!   migrated by the normal startup migrations.
//! - **Restore** never swaps a live database: a safety copy of the current data is written to
//!   `backups/before-restore-<UTC>.sqlite3` (the 5 most recent are kept), the backup is staged as
//!   `restore-pending.sqlite3`, and the app restarts. At the next start, **before the database is
//!   opened**, [`apply_pending`] validates the staged file again and swaps it in (the old main file
//!   and any `-wal` / `-shm` are moved aside first and put back if anything fails), so the app is
//!   never left without a database.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::database::connection::APPLICATION_ID;
use crate::database::migrations::LATEST_SCHEMA_VERSION;

/// The staged backup, swapped in at the next start.
pub const PENDING_FILE_NAME: &str = "restore-pending.sqlite3";
/// Folder (inside the app data folder) for the safety copies taken before a restore.
pub const BACKUPS_DIR_NAME: &str = "backups";
pub const SAFETY_PREFIX: &str = "before-restore-";
/// How many safety copies are kept.
pub const SAFETY_COPIES_KEPT: usize = 5;
const SCRATCH_FILE_NAME: &str = "restore-check.tmp";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackupError {
    #[error("This file is not a Cargo Line backup (it is not a SQLite database).")]
    NotSqlite,
    #[error("This file is not a Cargo Line backup (it belongs to another program).")]
    NotCargoLine,
    #[error("This backup was made by a newer version of Cargo Line.")]
    TooNew { found: i64, supported: i64 },
    #[error("This backup is damaged and cannot be used ({0}).")]
    Corrupted(String),
    #[error("Choose a file other than the database Cargo Line is using.")]
    SameAsLive,
    #[error("The file could not be read or written: {0}")]
    Io(String),
    #[error("A database operation failed: {0}")]
    Sqlite(String),
}

impl From<std::io::Error> for BackupError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<rusqlite::Error> for BackupError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e.to_string())
    }
}

/// What a backup contains (shown before restoring, and after an export or a restore).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub schema_version: i64,
    pub active_contracts: i64,
    pub finished_contracts: i64,
    pub runs: i64,
    pub bytes: u64,
}

/// Removes a database file and its `-wal` / `-shm` companions (missing files are fine).
fn remove_db_files(path: &Path) {
    for p in [
        path.to_path_buf(),
        sibling(path, "-wal"),
        sibling(path, "-shm"),
    ] {
        let _ = std::fs::remove_file(p);
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn is_not_a_database(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::NotADatabase)
}

/// Checks a candidate backup without ever writing to it: it is copied to a scratch file in
/// `work_dir` and only the copy is opened.
pub fn inspect(file: &Path, work_dir: &Path) -> Result<BackupInfo, BackupError> {
    let bytes = std::fs::metadata(file)?.len();
    std::fs::create_dir_all(work_dir)?;
    let scratch = work_dir.join(SCRATCH_FILE_NAME);
    remove_db_files(&scratch);
    std::fs::copy(file, &scratch)?;
    let result = inspect_copy(&scratch, bytes);
    remove_db_files(&scratch);
    result
}

fn inspect_copy(scratch: &Path, bytes: u64) -> Result<BackupInfo, BackupError> {
    // Read-write but never create: the scratch copy is ours, and WAL files may need creating.
    let conn = Connection::open_with_flags(
        scratch,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let application_id: i32 = match conn.pragma_query_value(None, "application_id", |r| r.get(0)) {
        Ok(v) => v,
        Err(e) if is_not_a_database(&e) => return Err(BackupError::NotSqlite),
        Err(e) => return Err(BackupError::Corrupted(e.to_string())),
    };
    if application_id != APPLICATION_ID {
        return Err(BackupError::NotCargoLine);
    }
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > LATEST_SCHEMA_VERSION {
        return Err(BackupError::TooNew {
            found: version,
            supported: LATEST_SCHEMA_VERSION,
        });
    }
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| BackupError::Corrupted(e.to_string()))?;
    if integrity != "ok" {
        return Err(BackupError::Corrupted(integrity));
    }
    let count = |sql: &str| {
        conn.query_row(sql, [], |r| r.get::<_, i64>(0))
            .map_err(|e| BackupError::Corrupted(e.to_string()))
    };
    Ok(BackupInfo {
        schema_version: version,
        active_contracts: count("SELECT COUNT(*) FROM contracts WHERE status = 'active'")?,
        finished_contracts: count("SELECT COUNT(*) FROM contracts WHERE status <> 'active'")?,
        runs: count("SELECT COUNT(*) FROM runs")?,
        bytes,
    })
}

/// Writes a consistent copy of the live database to `dest` and validates it.
pub fn export(conn: &Connection, dest: &Path, work_dir: &Path) -> Result<BackupInfo, BackupError> {
    let partial = sibling(dest, ".partial");
    remove_db_files(&partial);
    let target = partial.to_string_lossy().into_owned();
    if let Err(e) = conn.execute("VACUUM INTO ?1", [&target]) {
        remove_db_files(&partial);
        return Err(e.into());
    }
    if let Err(e) = std::fs::rename(&partial, dest) {
        remove_db_files(&partial);
        return Err(e.into());
    }
    inspect(dest, work_dir)
}

/// `before-restore-<UTC timestamp>.sqlite3` in `backups_dir`; keeps the newest
/// [`SAFETY_COPIES_KEPT`] and deletes older ones (only files with that naming).
pub fn safety_copy(
    conn: &Connection,
    backups_dir: &Path,
    now_ms: i64,
) -> Result<PathBuf, BackupError> {
    std::fs::create_dir_all(backups_dir)?;
    let mut path = backups_dir.join(format!(
        "{SAFETY_PREFIX}{}.sqlite3",
        crate::capture::storage::utc_stamp(now_ms)
    ));
    let mut n = 1;
    while path.exists() {
        path = backups_dir.join(format!(
            "{SAFETY_PREFIX}{}-{n}.sqlite3",
            crate::capture::storage::utc_stamp(now_ms)
        ));
        n += 1;
    }
    conn.execute("VACUUM INTO ?1", [path.to_string_lossy().into_owned()])?;
    prune_safety_copies(backups_dir)?;
    Ok(path)
}

/// The safety copies, newest first (by name: the UTC timestamp sorts in time order).
pub fn safety_copies(backups_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    if !backups_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(backups_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(SAFETY_PREFIX) && n.ends_with(".sqlite3"))
        })
        .collect();
    files.sort();
    files.reverse();
    Ok(files)
}

fn prune_safety_copies(backups_dir: &Path) -> std::io::Result<()> {
    for old in safety_copies(backups_dir)?
        .into_iter()
        .skip(SAFETY_COPIES_KEPT)
    {
        remove_db_files(&old);
    }
    Ok(())
}

/// Copies a validated backup to `<data_dir>/restore-pending.sqlite3` (swapped in at next start).
pub fn stage_restore(backup: &Path, data_dir: &Path) -> Result<PathBuf, BackupError> {
    let pending = data_dir.join(PENDING_FILE_NAME);
    let partial = sibling(&pending, ".partial");
    remove_db_files(&partial);
    std::fs::copy(backup, &partial)?;
    std::fs::rename(&partial, &pending)?;
    Ok(pending)
}

/// What happened at startup with a staged restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupRestore {
    /// No staged restore.
    None,
    /// The backup is now the database.
    Restored(BackupInfo),
    /// The staged file was refused or could not be swapped in; the current database is kept.
    Failed(String),
}

/// Runs at startup **before** the database is opened. Never leaves the app without a database:
/// the current files are moved aside first and put back if the swap fails; the pending file is
/// always removed.
pub fn apply_pending(data_dir: &Path, db_file: &Path) -> StartupRestore {
    let pending = data_dir.join(PENDING_FILE_NAME);
    if !pending.is_file() {
        return StartupRestore::None;
    }
    let info = match inspect(&pending, data_dir) {
        Ok(info) => info,
        Err(e) => {
            remove_db_files(&pending);
            return StartupRestore::Failed(format!(
                "The backup could not be restored: {e} Your data was not changed."
            ));
        }
    };
    // Move the current database files aside (main file, -wal, -shm).
    let parts = [
        (db_file.to_path_buf(), sibling(db_file, ".replaced")),
        (sibling(db_file, "-wal"), sibling(db_file, ".replaced-wal")),
        (sibling(db_file, "-shm"), sibling(db_file, ".replaced-shm")),
    ];
    let mut moved = Vec::new();
    for (from, to) in &parts {
        let _ = std::fs::remove_file(to);
        if from.exists() {
            if let Err(e) = std::fs::rename(from, to) {
                put_back(&moved);
                remove_db_files(&pending);
                return StartupRestore::Failed(format!(
                    "The backup could not be restored ({e}). Your data was not changed."
                ));
            }
            moved.push((from.clone(), to.clone()));
        }
    }
    if let Err(e) = std::fs::rename(&pending, db_file) {
        put_back(&moved);
        remove_db_files(&pending);
        return StartupRestore::Failed(format!(
            "The backup could not be restored ({e}). Your data was not changed."
        ));
    }
    for (_, aside) in &moved {
        let _ = std::fs::remove_file(aside);
    }
    StartupRestore::Restored(info)
}

fn put_back(moved: &[(PathBuf, PathBuf)]) {
    for (original, aside) in moved {
        let _ = std::fs::rename(aside, original);
    }
}

/// The notice shown once after startup (Task 17).
pub fn restore_notice(outcome: &StartupRestore) -> Option<(bool, String)> {
    match outcome {
        StartupRestore::None => None,
        StartupRestore::Restored(info) => Some((
            true,
            format!(
                "Backup restored ({} active contract{}, {} in history). A safety copy of your previous data is in the backups folder.",
                info.active_contracts,
                if info.active_contracts == 1 { "" } else { "s" },
                info.finished_contracts
            ),
        )),
        StartupRestore::Failed(message) => Some((false, message.clone())),
    }
}

/// The startup notice, kept until the frontend takes it (once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupNotice {
    pub ok: bool,
    pub message: String,
}

#[derive(Default)]
pub struct StartupNoticeState(pub std::sync::Mutex<Option<StartupNotice>>);

/* ------------------------------------------------------------ allow-lists */

/// Links the About group may open. The frontend sends a key, never a URL.
pub fn link_url(key: &str) -> Option<&'static str> {
    match key {
        "repository" => Some("https://github.com/MathisVkg/cargoline"),
        "license" => Some("https://github.com/MathisVkg/cargoline/blob/main/LICENSE"),
        "wikiApi" => Some("https://api.star-citizen.wiki"),
        _ => None,
    }
}

/// The program and argument used to show the app's own data folder; the path never comes from
/// the frontend.
pub fn open_folder_command(data_dir: &Path) -> (&'static str, PathBuf) {
    ("explorer.exe", data_dir.to_path_buf())
}
