//! Optional startup timing marks.
//!
//! **Off by default.** With `CARGO_LINE_STARTUP_TRACE=1`, each mark is written as one line to
//! stderr and to `startup-trace.log` in the data folder (replaced at every launch):
//!
//! ```text
//! [trace]    412.6 ms  database opened (schema 6)
//! ```
//!
//! Offsets are measured from the **process creation time** given by Windows (elsewhere, from the
//! first call to [`init`]). Marks carry names and counts only: never paths, titles, the handle or
//! Game.log text. Frontend marks arrive through the `trace_marks` command with their wall-clock
//! time and are placed on the same scale.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TRACE_ENV: &str = "CARGO_LINE_STARTUP_TRACE";
pub const TRACE_FILE_NAME: &str = "startup-trace.log";
/// Longest mark name kept; longer names are cut.
pub const MAX_NAME_CHARS: usize = 64;

struct Tracer {
    /// Unix epoch ms of the process start.
    origin_ms: f64,
    out: Mutex<Output>,
}

#[derive(Default)]
struct Output {
    /// Lines written before the data folder was known.
    buffered: Vec<String>,
    file: Option<PathBuf>,
    once: HashSet<String>,
}

static TRACER: OnceLock<Option<Tracer>> = OnceLock::new();

/// Whether the variable's value switches tracing on (`1`, `true`, `yes`, `on`).
pub fn is_enabled_value(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// Reads the variable once. Call first thing in `main`.
pub fn init() {
    TRACER.get_or_init(|| {
        let on = is_enabled_value(std::env::var(TRACE_ENV).ok().as_deref());
        on.then(|| Tracer {
            origin_ms: process_start_ms().unwrap_or_else(now_ms),
            out: Mutex::new(Output::default()),
        })
    });
}

fn tracer() -> Option<&'static Tracer> {
    TRACER.get().and_then(Option::as_ref)
}

pub fn enabled() -> bool {
    tracer().is_some()
}

/// Unix epoch ms, fractional.
pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

#[cfg(windows)]
fn process_start_ms() -> Option<f64> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: the pseudo handle of the current process needs no closing; the out pointers are
    // valid locals.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    }
    .ok()?;
    let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    // FILETIME counts 100 ns since 1601-01-01; the Unix epoch is 11 644 473 600 s later.
    Some(ticks as f64 / 10_000.0 - 11_644_473_600_000.0)
}

#[cfg(not(windows))]
fn process_start_ms() -> Option<f64> {
    None
}

/// Keeps letters, digits, spaces and `:/_.-()`, cut to [`MAX_NAME_CHARS`]; anything else becomes
/// `?`. Frontend names pass through this too.
pub fn sanitize_name(name: &str) -> String {
    name.trim()
        .chars()
        .take(MAX_NAME_CHARS)
        .map(|c| {
            if c.is_ascii_alphanumeric() || " :/_.-()".contains(c) {
                c
            } else {
                '?'
            }
        })
        .collect()
}

/// One trace line: the offset in ms with one decimal, then the name.
pub fn format_line(offset_ms: f64, name: &str) -> String {
    format!("[trace] {offset_ms:>9.1} ms  {}", sanitize_name(name))
}

/// A mark at the current time.
pub fn mark(name: &str) {
    mark_at(name, now_ms());
}

/// A mark at a given wall-clock time (Unix epoch ms).
pub fn mark_at(name: &str, at_ms: f64) {
    if let Some(t) = tracer() {
        write(t, format_line(at_ms - t.origin_ms, name));
    }
}

/// Like [`mark`], only the first time this name is used.
pub fn mark_once(name: &str) {
    if let Some(t) = tracer() {
        let first = t
            .out
            .lock()
            .map(|mut o| o.once.insert(name.to_string()))
            .unwrap_or(false);
        if first {
            write(t, format_line(now_ms() - t.origin_ms, name));
        }
    }
}

fn write(t: &Tracer, line: String) {
    eprintln!("{line}");
    if let Ok(mut out) = t.out.lock() {
        match &out.file {
            Some(file) => append(file, &[line]),
            None => out.buffered.push(line),
        }
    }
}

fn append(file: &Path, lines: &[String]) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(file) {
        for line in lines {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Starts the trace file in the data folder (replacing the previous launch's) and writes the
/// marks made so far.
pub fn set_output_dir(dir: &Path) {
    if let Some(t) = tracer() {
        if let Ok(mut out) = t.out.lock() {
            if out.file.is_some() {
                return;
            }
            let file = dir.join(TRACE_FILE_NAME);
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(&file, "");
            let lines = std::mem::take(&mut out.buffered);
            append(&file, &lines);
            out.file = Some(file);
        }
    }
}
