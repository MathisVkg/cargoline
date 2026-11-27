//! Where the app keeps its data, and which kind of copy is running.
//!
//! - **Installed** (the NSIS installer): data in `%APPDATA%\io.github.mathisvkg.sccargoapp\`.
//! - **Portable** (the zip): a `portable.txt` marker next to the exe; data in `<exe folder>\data\`,
//!   WebView2 profile in `<exe folder>\data\webview\`. Nothing is written to `%APPDATA%` or
//!   `%LOCALAPPDATA%`, and there is never a silent fall back to them.
//! - **Dev**: a debug build.
//!
//! The decisions are pure functions so they can be tested; [`current`] applies them once to the
//! running process.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;

/// The marker file that turns a copy into the portable version.
pub const PORTABLE_MARKER: &str = "portable.txt";
/// Data folder of the portable version, next to the exe.
pub const PORTABLE_DATA_DIR: &str = "data";
/// WebView2 profile inside the portable data folder.
pub const PORTABLE_WEBVIEW_DIR: &str = "webview";
/// Read by WebView2 itself; set in portable mode before the window is created.
pub const WEBVIEW2_USER_DATA_ENV: &str = "WEBVIEW2_USER_DATA_FOLDER";

/// Which kind of copy is running. The update check uses it: only `installed` updates itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    Installed,
    Portable,
    Dev,
}

impl AppMode {
    /// `dev` for a debug build, `portable` when the marker is next to the exe, else `installed`.
    pub fn resolve(debug_build: bool, marker_exists: bool) -> Self {
        if debug_build {
            AppMode::Dev
        } else if marker_exists {
            AppMode::Portable
        } else {
            AppMode::Installed
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AppMode::Installed => "installed",
            AppMode::Portable => "portable",
            AppMode::Dev => "dev",
        }
    }
}

/// Where the data goes, in order of precedence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataDir {
    /// `CARGO_LINE_DATA_DIR` (absolute, any build), or `CARGO_LINE_DEV_DATA_DIR` (debug only).
    Override(PathBuf),
    /// `<exe folder>\data\`, because `portable.txt` is next to the exe.
    Portable(PathBuf),
    /// The normal app data folders (`%APPDATA%` / `%LOCALAPPDATA%`), resolved by Tauri.
    Default,
}

impl DataDir {
    /// The roaming data folder, unless Tauri's default applies.
    pub fn path(&self) -> Option<&Path> {
        match self {
            DataDir::Override(p) | DataDir::Portable(p) => Some(p),
            DataDir::Default => None,
        }
    }
}

/// The inputs of the data folder decision.
#[derive(Debug, Clone, Default)]
pub struct DataDirInputs {
    pub debug_build: bool,
    /// `CARGO_LINE_DATA_DIR`.
    pub data_dir_env: Option<OsString>,
    /// `CARGO_LINE_DEV_DATA_DIR`.
    pub dev_data_dir_env: Option<OsString>,
    /// The folder that holds the running exe.
    pub exe_dir: Option<PathBuf>,
    /// Whether `portable.txt` is in `exe_dir`.
    pub marker_exists: bool,
}

/// The data folder: the override wins, then the portable folder, then the default.
/// A relative `CARGO_LINE_DATA_DIR` is ignored, and the dev override only counts in debug builds.
pub fn resolve_data_dir(inputs: &DataDirInputs) -> DataDir {
    if let Some(dir) = crate::data_dir_override(
        inputs.debug_build,
        inputs.data_dir_env.clone(),
        inputs.dev_data_dir_env.clone(),
    ) {
        return DataDir::Override(dir);
    }
    match (&inputs.exe_dir, inputs.marker_exists) {
        (Some(exe_dir), true) => DataDir::Portable(exe_dir.join(PORTABLE_DATA_DIR)),
        _ => DataDir::Default,
    }
}

/// The WebView2 profile folder to set for a portable copy, unless the variable is already set.
pub fn portable_webview_dir(data: &DataDir, already_set: bool) -> Option<PathBuf> {
    match data {
        DataDir::Portable(dir) if !already_set => Some(dir.join(PORTABLE_WEBVIEW_DIR)),
        _ => None,
    }
}

/// Folders that may hold the OCR bundle (`tesseract\`), in lookup order: next to the exe first,
/// then the resource folder. Duplicates are removed (on Windows both are usually the exe folder).
pub fn ocr_bundle_candidates(
    exe_dir: Option<&Path>,
    resource_dir: Option<&Path>,
    bundle: &str,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for base in [exe_dir, resource_dir].into_iter().flatten() {
        let candidate = base.join(bundle);
        if !out.iter().any(|c| same_path(c, &candidate)) {
            out.push(candidate);
        }
    }
    out
}

/// Path equality ignoring case and `\` vs `/` (Windows paths), without touching the disk.
fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// Whether `path` is inside `parent` (string comparison, case-insensitive).
pub fn is_within(path: &Path, parent: &Path) -> bool {
    let norm = |p: &Path| {
        let mut s = p.to_string_lossy().replace('/', "\\").to_lowercase();
        if !s.ends_with('\\') {
            s.push('\\');
        }
        s
    };
    let parent = norm(parent);
    parent.len() > 1 && norm(path).starts_with(&parent)
}

/// Why a portable copy can't start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortableProblem {
    /// The folder can't be written (for example a zip opened in Explorer without extracting).
    NotWritable(String),
    /// The exe runs from a temporary folder: Explorer runs a program from a zip that way, and the
    /// data would be lost when Windows cleans that folder.
    InTempFolder,
}

/// The message shown before the app quits; it tells the user what to do.
pub fn portable_problem_message(problem: &PortableProblem) -> String {
    let base = "Cargo Line (portable) can't keep its data next to the program.";
    let advice = "Extract the whole zip to a normal folder first (right-click it, Extract All), \
                  then start cargo-line.exe from there.";
    match problem {
        PortableProblem::NotWritable(detail) => format!(
            "{base}\n\nThe folder can't be written: {detail}\n\n{advice}\n\n\
             To use the normal app data folder instead, delete portable.txt."
        ),
        PortableProblem::InTempFolder => format!(
            "{base}\n\nIt is running from a temporary folder, which Windows may empty at any time.\n\n{advice}"
        ),
    }
}

/// Creates the portable data folder and checks that it can be written (a probe file is created
/// and removed). Never falls back to another folder.
pub fn prepare_portable_dir(data: &Path) -> Result<(), PortableProblem> {
    std::fs::create_dir_all(data).map_err(|e| PortableProblem::NotWritable(e.to_string()))?;
    let probe = data.join(".write-check");
    std::fs::write(&probe, b"ok").map_err(|e| PortableProblem::NotWritable(e.to_string()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// The layout of the running process, decided once.
#[derive(Debug, Clone)]
pub struct Layout {
    pub mode: AppMode,
    pub data: DataDir,
    pub exe_dir: Option<PathBuf>,
}

static LAYOUT: OnceLock<Layout> = OnceLock::new();

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

/// The layout of this process (environment and exe folder are read on first use).
pub fn current() -> &'static Layout {
    LAYOUT.get_or_init(|| {
        let exe_dir = exe_dir();
        let marker_exists = exe_dir
            .as_ref()
            .is_some_and(|d| d.join(PORTABLE_MARKER).is_file());
        let inputs = DataDirInputs {
            debug_build: cfg!(debug_assertions),
            data_dir_env: std::env::var_os(crate::DATA_DIR_ENV),
            dev_data_dir_env: std::env::var_os(crate::DEV_DATA_DIR_ENV),
            exe_dir: exe_dir.clone(),
            marker_exists,
        };
        Layout {
            mode: AppMode::resolve(inputs.debug_build, marker_exists),
            data: resolve_data_dir(&inputs),
            exe_dir,
        }
    })
}

/// Runs before Tauri creates the window. For a portable copy: refuses a temporary or read-only
/// folder, and points WebView2 at `data\webview\`. Returns the problem when the app must quit.
pub fn prepare_process() -> Result<(), PortableProblem> {
    let layout = current();
    let DataDir::Portable(data) = &layout.data else {
        return Ok(());
    };
    if let Some(exe_dir) = &layout.exe_dir {
        if is_within(exe_dir, &std::env::temp_dir()) {
            return Err(PortableProblem::InTempFolder);
        }
    }
    prepare_portable_dir(data)?;
    let already_set = std::env::var_os(WEBVIEW2_USER_DATA_ENV).is_some_and(|v| !v.is_empty());
    if let Some(webview) = portable_webview_dir(&layout.data, already_set) {
        // Single-threaded at this point: nothing else reads the environment yet.
        std::env::set_var(WEBVIEW2_USER_DATA_ENV, webview);
    }
    Ok(())
}

/// Shows a blocking error box (the release exe has no console).
#[cfg(windows)]
pub fn show_fatal(message: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let text = wide(message);
    let title = wide("Cargo Line");
    // SAFETY: both buffers are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
pub fn show_fatal(message: &str) {
    eprintln!("{message}");
}
