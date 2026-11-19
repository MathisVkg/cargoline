//! Screenshot files under `<app data>\captures\` and their `contract_captures` rows.
//!
//! - Files are named `<UTC timestamp>-<random>.png` and written once (never overwritten).
//! - A row is inserted only after its file is fully written, so a row never points at
//!   a half-written file.
//! - Files are only ever touched through `captures_dir.join(<validated name>)`: a stored
//!   path can never make cleanup delete something outside the folder.
//! - Retention: 7 days. Unsaved captures (no contract) are deleted when the form is
//!   closed, replaced by a recapture, or found at the next startup.

use std::collections::hash_map::RandomState;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::{CaptureBackend, CaptureSourceKind, Frame};

/// Folder name inside the app data directory.
pub const CAPTURES_DIR_NAME: &str = "captures";
/// How long a capture is kept: 7 days (the schema's maximum).
pub const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Files without a row are only removed once they are this old, so a capture that is
/// being written right now is never mistaken for an orphan.
pub const ORPHAN_GRACE: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("could not write the screenshot: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not encode the screenshot: {0}")]
    Encode(#[from] png::EncodingError),
    #[error("could not record the screenshot: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("the frame is empty or its size does not match its pixels")]
    BadFrame,
}

/// A stored capture as the frontend sees it (the file path stays in the backend).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInfo {
    pub id: i64,
    pub contract_id: Option<i64>,
    pub captured_at: i64,
    pub expires_at: i64,
    pub source: Option<CaptureSourceKind>,
    pub backend: Option<CaptureBackend>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(skip)]
    pub file_path: String,
}

/// What a cleanup pass did. Failures are listed, never raised.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CleanupReport {
    pub expired: usize,
    pub unlinked: usize,
    pub missing_files: usize,
    pub orphan_files: usize,
    pub errors: Vec<String>,
}

/* ---------------------------------------------------------------- naming */

/// `2026-11-19 16:10:12.345 UTC` → `20261119T161012345Z`.
pub fn utc_stamp(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}{millis:03}Z",
        tod / 3600,
        tod % 3600 / 60,
        tod % 60
    )
}

/// Days since 1970-01-01 → (year, month, day), proleptic Gregorian (H. Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// 8 hex characters, unique within the process and unpredictable across runs.
fn random_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    format!("{:08x}", h.finish() as u32)
}

pub fn file_name(captured_at_ms: i64, suffix: &str) -> String {
    format!("{}-{suffix}.png", utc_stamp(captured_at_ms))
}

/// Accepts only names this module produces: `[0-9A-Za-z-]+.png`, no separators or dots.
pub fn is_capture_file_name(name: &str) -> bool {
    name.strip_suffix(".png").is_some_and(|stem| {
        !stem.is_empty()
            && stem.len() <= 64
            && stem.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// The file inside `dir` for a stored path, or None if the stored name is not ours.
pub fn file_in_dir(dir: &Path, stored_path: &str) -> Option<PathBuf> {
    let name = Path::new(stored_path).file_name()?.to_str()?;
    is_capture_file_name(name).then(|| dir.join(name))
}

/* ------------------------------------------------------------------ PNG */

/// Encodes BGRA8 as an RGB PNG (alpha is always opaque for screen captures).
pub fn encode_png(frame: &Frame, out: impl Write) -> Result<(), StorageError> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    if w == 0 || h == 0 || frame.bgra.len() != w * h * 4 {
        return Err(StorageError::BadFrame);
    }
    let mut encoder = png::Encoder::new(out, frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header()?;
    let mut rgb = Vec::with_capacity(w * h * 3);
    for [b, g, r, _] in frame.bgra.as_chunks::<4>().0 {
        rgb.extend_from_slice(&[*r, *g, *b]);
    }
    writer.write_image_data(&rgb)?;
    writer.finish()?;
    Ok(())
}

/* ------------------------------------------------------------ rows/files */

const SELECT_INFO: &str = "SELECT id, contract_id, captured_at, expires_at, capture_source,
            capture_backend, width, height, file_path FROM contract_captures";

fn info_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CaptureInfo> {
    let source: Option<String> = row.get(4)?;
    let backend: Option<String> = row.get(5)?;
    Ok(CaptureInfo {
        id: row.get(0)?,
        contract_id: row.get(1)?,
        captured_at: row.get(2)?,
        expires_at: row.get(3)?,
        source: source.as_deref().and_then(|s| match s {
            "window" => Some(CaptureSourceKind::Window),
            "display" => Some(CaptureSourceKind::Display),
            _ => None,
        }),
        backend: backend.as_deref().and_then(|s| match s {
            "wgc" => Some(CaptureBackend::Wgc),
            "print_window" => Some(CaptureBackend::PrintWindow),
            _ => None,
        }),
        width: row.get(6)?,
        height: row.get(7)?,
        file_path: row.get(8)?,
    })
}

pub fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<CaptureInfo>> {
    conn.query_row(&format!("{SELECT_INFO} WHERE id = ?1"), [id], info_from_row)
        .optional()
}

/// Writes the frame to a new file in `dir`, then records it (unlinked, trigger `manual`).
/// If recording fails, the file is removed again.
pub fn store(
    conn: &Connection,
    dir: &Path,
    frame: &Frame,
    captured_at_ms: i64,
) -> Result<CaptureInfo, StorageError> {
    fs::create_dir_all(dir)?;
    let path = dir.join(file_name(captured_at_ms, &random_suffix()));
    {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut out = BufWriter::new(file);
        let written = encode_png(frame, &mut out).and_then(|()| {
            out.flush()?;
            out.get_ref().sync_all()?;
            Ok(())
        });
        if let Err(e) = written {
            drop(out);
            let _ = fs::remove_file(&path);
            return Err(e);
        }
    }
    let path_text = path.to_string_lossy().into_owned();
    let inserted = conn.execute(
        "INSERT INTO contract_captures (contract_id, file_path, captured_at, expires_at,
             capture_method, capture_source, capture_backend, width, height)
         VALUES (NULL, ?1, ?2, ?3, 'manual', ?4, ?5, ?6, ?7)",
        params![
            path_text,
            captured_at_ms,
            captured_at_ms + RETENTION_MS,
            frame.source.as_str(),
            frame.backend.as_str(),
            frame.width,
            frame.height
        ],
    );
    match inserted {
        Ok(_) => {
            Ok(get(conn, conn.last_insert_rowid())?.ok_or(rusqlite::Error::QueryReturnedNoRows)?)
        }
        Err(e) => {
            let _ = fs::remove_file(&path);
            Err(e.into())
        }
    }
}

/// Links an unsaved capture to a contract. Call inside the contract's save transaction.
/// Returns false (and changes nothing) if the capture is gone or already linked: the
/// contract is saved either way.
pub fn link(conn: &Connection, capture_id: i64, contract_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE contract_captures SET contract_id = ?2 WHERE id = ?1 AND contract_id IS NULL",
        params![capture_id, contract_id],
    )? == 1)
}

/// Deletes an unsaved capture's row and file. Linked captures are never touched.
/// Returns whether a row was deleted.
pub fn discard_unlinked(conn: &Connection, dir: &Path, id: i64) -> Result<bool, StorageError> {
    let path: Option<String> = conn
        .query_row(
            "DELETE FROM contract_captures WHERE id = ?1 AND contract_id IS NULL RETURNING file_path",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(path) = path else { return Ok(false) };
    if let Some(file) = file_in_dir(dir, &path) {
        remove_if_exists(&file)?;
    }
    Ok(true)
}

/// The file to serve for a capture id, only if it is a valid name inside `dir` and exists.
pub fn file_for_serving(
    conn: &Connection,
    dir: &Path,
    id: i64,
) -> rusqlite::Result<Option<PathBuf>> {
    let path: Option<String> = conn
        .query_row(
            "SELECT file_path FROM contract_captures WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(path
        .and_then(|p| file_in_dir(dir, &p))
        .filter(|f| f.is_file()))
}

fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/* -------------------------------------------------------------- cleanup */

/// One cleanup pass. Never fails: every problem is listed in the report.
/// - expired rows (`expires_at <= now`) and their files;
/// - with `remove_unlinked` (startup only): unsaved captures left by an earlier session;
/// - rows whose file no longer exists;
/// - `*.png` files in `dir` without a row, older than [`ORPHAN_GRACE`].
pub fn cleanup(conn: &Connection, dir: &Path, now_ms: i64, remove_unlinked: bool) -> CleanupReport {
    let mut report = CleanupReport::default();

    let mut delete_where = |condition: &str, counter: fn(&mut CleanupReport) -> &mut usize| {
        let rows: Vec<(i64, String)> = match conn
            .prepare(&format!(
                "SELECT id, file_path FROM contract_captures WHERE {condition}"
            ))
            .and_then(|mut s| {
                s.query_map(params![now_ms], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<_>>()
            }) {
            Ok(rows) => rows,
            Err(e) => {
                report.errors.push(format!("listing captures: {e}"));
                return;
            }
        };
        for (id, path) in rows {
            if let Some(file) = file_in_dir(dir, &path) {
                if let Err(e) = remove_if_exists(&file) {
                    report
                        .errors
                        .push(format!("deleting {}: {e}", file.display()));
                    continue; // keep the row so the next pass retries
                }
            }
            match conn.execute("DELETE FROM contract_captures WHERE id = ?1", [id]) {
                Ok(_) => *counter(&mut report) += 1,
                Err(e) => report
                    .errors
                    .push(format!("deleting capture row {id}: {e}")),
            }
        }
    };

    delete_where("expires_at <= ?1", |r| &mut r.expired);
    if remove_unlinked {
        delete_where("contract_id IS NULL AND ?1 IS NOT NULL", |r| {
            &mut r.unlinked
        });
    }

    // Rows whose file is gone (deleted by hand, or never readable).
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT id, file_path FROM contract_captures")
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()
        })
        .unwrap_or_else(|e| {
            report.errors.push(format!("listing captures: {e}"));
            Vec::new()
        });
    let mut known = HashSet::new();
    for (id, path) in rows {
        let exists = file_in_dir(dir, &path).is_some_and(|f| f.is_file());
        if exists {
            if let Some(name) = Path::new(&path).file_name() {
                known.insert(name.to_os_string());
            }
            continue;
        }
        match conn.execute("DELETE FROM contract_captures WHERE id = ?1", [id]) {
            Ok(_) => report.missing_files += 1,
            Err(e) => report
                .errors
                .push(format!("deleting capture row {id}: {e}")),
        }
    }

    // Orphan files: ours by name, no row, and not being written right now.
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return report,
        Err(e) => {
            report
                .errors
                .push(format!("reading {}: {e}", dir.display()));
            return report;
        }
    };
    let now = UNIX_EPOCH + Duration::from_millis(now_ms.max(0) as u64);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(text) = name.to_str() else { continue };
        if !is_capture_file_name(text) || known.contains(&name) {
            continue;
        }
        let old_enough = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|modified| {
                now.duration_since(modified)
                    .is_ok_and(|age| age >= ORPHAN_GRACE)
            })
            .unwrap_or(false);
        if !old_enough || !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        match remove_if_exists(&entry.path()) {
            Ok(()) => report.orphan_files += 1,
            Err(e) => report
                .errors
                .push(format!("deleting {}: {e}", entry.path().display())),
        }
    }
    report
}
