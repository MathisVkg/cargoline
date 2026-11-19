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
        if self.created.is_some() && created.is_some() && created != self.created {
            self.restart(); // replaced by a new file
        }
        self.created = created;
        if len < self.pos {
            self.restart(); // truncated
        }
        if len == self.pos {
            return Ok(Vec::new());
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
