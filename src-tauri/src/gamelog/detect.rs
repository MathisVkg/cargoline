//! Finding `Game.log` without opening the game's process.
//!
//! Sources, all read-only, merged and ranked:
//! 1. the running `StarCitizen.exe` image path, queried without a process handle
//!    (`NtQuerySystemInformation(SystemProcessIdInformation)`, see `capture::native`);
//! 2. the RSI Launcher's own log (`%APPDATA%\rsilauncher\logs\log.log`), which records the install
//!    and channel folders;
//! 3. common install folders on every drive.
//!
//! Ranking: `LIVE` first, then the most recently modified. Nothing is ever written.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

/// Game channels, in preference order for ties.
pub const CHANNELS: [&str; 5] = ["LIVE", "PTU", "EPTU", "TECH-PREVIEW", "HOTFIX"];
const RSI: &str = "Roberts Space Industries";
const GAME_DIR: &str = "StarCitizen";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DetectMethod {
    /// The running game's executable path.
    Process,
    /// The RSI Launcher's log.
    Launcher,
    /// A common install folder.
    Folders,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundLog {
    pub path: String,
    pub channel: String,
    /// Last modification, ms since the epoch.
    pub modified_ms: Option<i64>,
    pub method: DetectMethod,
}

/// `…\StarCitizen\LIVE\Bin64\StarCitizen.exe` → `…\StarCitizen\LIVE\Game.log`.
pub fn game_log_for_exe(exe: &Path) -> Option<PathBuf> {
    let bin = exe.parent()?;
    let channel = if bin
        .file_name()?
        .to_string_lossy()
        .eq_ignore_ascii_case("Bin64")
    {
        bin.parent()?
    } else {
        bin
    };
    Some(channel.join("Game.log"))
}

/// The channel folder name of a `…\<channel>\Game.log` path.
pub fn channel_of(log: &Path) -> String {
    log.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `StarCitizen` folders recorded in the launcher log (JSON-escaped `\\` or plain `\`).
pub fn roots_from_launcher_log(text: &str) -> Vec<PathBuf> {
    let text = text.replace("\\\\", "\\");
    let needle = format!("{RSI}\\{GAME_DIR}");
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(&needle) {
        let end = from + found + needle.len();
        let head = &text[..from + found];
        // Walk back to the drive letter ("X:\") that starts this path.
        let start = head.rfind(":\\").and_then(|colon| {
            let letter = colon.checked_sub(1)?;
            // A path never spans a line, a quote or a comma (the launcher lists paths with commas).
            let segment = &head[letter..];
            let ok = head.as_bytes()[letter].is_ascii_alphabetic()
                && (letter == 0 || !head.as_bytes()[letter - 1].is_ascii_alphanumeric())
                && from + found - colon <= 200
                && !segment.contains(['\n', '\r', '"', ',', '<', '>', '|']);
            ok.then_some(letter)
        });
        if let Some(start) = start {
            let root = PathBuf::from(&text[start..end]);
            if !roots
                .iter()
                .any(|r| r.as_os_str().eq_ignore_ascii_case(root.as_os_str()))
            {
                roots.push(root);
            }
        }
        from = end;
    }
    roots
}

/// Common `StarCitizen` folders: `%ProgramFiles%\RSI\StarCitizen`, then on every drive
/// `X:\RSI\StarCitizen` and `X:\Program Files\RSI\StarCitizen`.
pub fn common_roots(program_files: Option<&Path>, drives: &[char]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(pf) = program_files {
        roots.push(pf.join(RSI).join(GAME_DIR));
    }
    for d in drives {
        let drive = PathBuf::from(format!("{d}:\\"));
        roots.push(drive.join(RSI).join(GAME_DIR));
        roots.push(drive.join("Program Files").join(RSI).join(GAME_DIR));
    }
    roots
}

fn modified_ms(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

/// Every existing `<root>\<channel>\Game.log`.
pub fn logs_in_roots(roots: &[PathBuf], method: DetectMethod) -> Vec<FoundLog> {
    let mut out = Vec::new();
    for root in roots {
        for channel in CHANNELS {
            let log = root.join(channel).join("Game.log");
            if log.is_file() {
                out.push(FoundLog {
                    path: log.to_string_lossy().into_owned(),
                    channel: channel.to_string(),
                    modified_ms: modified_ms(&log),
                    method,
                });
            }
        }
    }
    out
}

pub fn found_from_exe(exe: &Path) -> Option<FoundLog> {
    let log = game_log_for_exe(exe)?;
    log.is_file().then(|| FoundLog {
        path: log.to_string_lossy().into_owned(),
        channel: channel_of(&log),
        modified_ms: modified_ms(&log),
        method: DetectMethod::Process,
    })
}

/// Dedupes (case-insensitive, the first source wins) and ranks: `LIVE` first, then the most
/// recently modified, then the channel order.
pub fn rank(found: Vec<FoundLog>) -> Vec<FoundLog> {
    let mut unique: Vec<FoundLog> = Vec::new();
    for f in found {
        if !unique.iter().any(|u| u.path.eq_ignore_ascii_case(&f.path)) {
            unique.push(f);
        }
    }
    let order = |c: &str| {
        CHANNELS
            .iter()
            .position(|x| x.eq_ignore_ascii_case(c))
            .unwrap_or(CHANNELS.len())
    };
    unique.sort_by(|a, b| {
        let live = |f: &FoundLog| !f.channel.eq_ignore_ascii_case("LIVE");
        live(a)
            .cmp(&live(b))
            .then(b.modified_ms.cmp(&a.modified_ms))
            .then(order(&a.channel).cmp(&order(&b.channel)))
    });
    unique
}

/// The message next to Detect for each outcome.
pub fn detect_message(found: &[FoundLog]) -> String {
    match found {
        [] => "Couldn't find Game.log — use Browse to select it".into(),
        [one] => format!("Found Game.log ({})", one.channel),
        many => format!("Found {} Game.log files — choose one", many.len()),
    }
}

/// Where Browse… opens: the current path's folder if it exists, otherwise the first existing
/// channel folder of the common roots, otherwise nothing (the system default).
pub fn browse_start(current: Option<&str>, roots: &[PathBuf]) -> Option<PathBuf> {
    if let Some(dir) = current
        .map(Path::new)
        .and_then(Path::parent)
        .filter(|d| d.is_dir())
    {
        return Some(dir.to_path_buf());
    }
    roots
        .iter()
        .flat_map(|r| CHANNELS.iter().map(move |c| r.join(c)))
        .find(|d| d.is_dir())
        .or_else(|| roots.iter().find(|r| r.is_dir()).cloned())
}
