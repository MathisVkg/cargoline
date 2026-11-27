//! Follows a growing log file by polling.
//!
//! - **Starts at the end** when the file exists at the first poll: old events are never replayed.
//!   A file that appears later (the game started) is read from its start.
//! - The file is opened, read and closed on every poll, so the game can rotate or replace it
//!   freely (no handle is kept).
//! - Rotation: if the file shrinks, or its creation time changes (replaced), it is read again
//!   from the start.
//! - Partial lines are buffered until their newline; lines split on `\n` with an optional `\r`;
//!   bytes are decoded as lossy UTF-8.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// At most this much is read per poll (a huge backlog is caught up over several polls).
const MAX_READ: u64 = 4 * 1024 * 1024;
/// A partial line longer than this is dropped (protects memory against a file without newlines).
const MAX_PARTIAL: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailError {
    NotFound,
    Unreadable(String),
}

/// What a poll does, decided from the file's size and creation time only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollPlan {
    /// Read again from the start (the file was replaced or truncated).
    pub restart: bool,
    /// Read from this offset; nothing is opened or read when it equals the length.
    pub from: u64,
}

impl PollPlan {
    pub fn reads(&self, len: u64) -> bool {
        self.from < len
    }
}

/// Decides a poll from the last position and creation time and the file's current metadata.
/// An unchanged file (same length, same creation time) gives a plan that reads nothing.
pub fn plan_poll(
    pos: u64,
    known_created: Option<SystemTime>,
    len: u64,
    created: Option<SystemTime>,
) -> PollPlan {
    let replaced = known_created.is_some() && created.is_some() && created != known_created;
    let restart = replaced || len < pos;
    PollPlan {
        restart,
        from: if restart { 0 } else { pos },
    }
}

#[derive(Debug)]
pub struct Tail {
    path: PathBuf,
    pos: u64,
    created: Option<SystemTime>,
    partial: Vec<u8>,
    first_poll: bool,
}

impl Tail {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            pos: 0,
            created: None,
            partial: Vec::new(),
            first_poll: true,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn restart(&mut self) {
        self.pos = 0;
        self.partial.clear();
    }

    /// Complete new lines since the last poll.
    pub fn poll(&mut self) -> Result<Vec<String>, TailError> {
        let first = std::mem::replace(&mut self.first_poll, false);
        let meta = match std::fs::metadata(&self.path) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => return Err(TailError::Unreadable("not a file".into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Gone: whatever appears next is a new file, read from its start.
                self.restart();
                self.created = None;
                return Err(TailError::NotFound);
            }
            Err(e) => return Err(TailError::Unreadable(e.kind().to_string())),
        };
        let len = meta.len();
        let created = meta.created().ok();
        if first {
            self.pos = len; // never replay old events
            self.created = created;
            return Ok(Vec::new());
        }
        let plan = plan_poll(self.pos, self.created, len, created);
        self.created = created;
        if plan.restart {
            self.restart(); // replaced by a new file, or truncated
        }
        if !plan.reads(len) {
            return Ok(Vec::new()); // unchanged: the file is not opened
        }
        let mut file =
            File::open(&self.path).map_err(|e| TailError::Unreadable(e.kind().to_string()))?;
        file.seek(SeekFrom::Start(self.pos))
            .map_err(|e| TailError::Unreadable(e.kind().to_string()))?;
        let mut chunk = Vec::new();
        let read = file
            .take((len - self.pos).min(MAX_READ))
            .read_to_end(&mut chunk)
            .map_err(|e| TailError::Unreadable(e.kind().to_string()))?;
        self.pos += read as u64;
        self.partial.extend_from_slice(&chunk);

        let mut lines = Vec::new();
        let mut start = 0;
        while let Some(offset) = self.partial[start..].iter().position(|&b| b == b'\n') {
            let mut line = &self.partial[start..start + offset];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            lines.push(String::from_utf8_lossy(line).into_owned());
            start += offset + 1;
        }
        self.partial.drain(..start);
        if self.partial.len() > MAX_PARTIAL {
            self.partial.clear();
        }
        Ok(lines)
    }
}
