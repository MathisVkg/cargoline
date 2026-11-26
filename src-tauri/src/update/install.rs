//! Running the installer for an in-app update (Task 20).
//!
//! - In-app installing is offered only for a real installed copy: `uninstall.exe` next to the exe
//!   **and** the Uninstall registry key's `InstallLocation` equal to the exe folder. A portable
//!   copy whose `portable.txt` was deleted reports `installed` but gets the notice and link.
//! - Command line: `"<setup>" /P /UPDATE /R [/ARGS ...] /D=<exe folder>` (Tauri's NSIS script):
//!   `/P` passive progress window, no questions; `/UPDATE` installs over the existing copy
//!   without uninstalling and never creates shortcuts (existing ones are kept); `/R` starts the
//!   app again when done; `/D=` the existing folder (must be last).
//! - The app is restarted through the installer, which doesn't pass this process's environment
//!   on. Test overrides that are set (data folder, feed, WebView2 profile) are therefore forwarded
//!   as one `--forward=<hex>` argument (UTF-8 `NAME=VALUE` lines, hex-encoded), so a test install
//!   never falls back to the real data folder. One token without spaces or quotes: NSIS's `/ARGS`
//!   parsing strips the outer quotes of a quoted list, which merged separate quoted arguments
//!   into one (seen in the Task 20 desktop session).

use std::path::{Path, PathBuf};

use crate::layout::AppMode;

/// `HKCU\<this>` holds the installer's `InstallLocation`.
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Cargo Line";
pub const UNINSTALLER: &str = "uninstall.exe";

/// Variables the restarted app gets back through `/ARGS` (only those that are set).
pub const FORWARDED_ENV: &[&str] = &[
    crate::DATA_DIR_ENV,
    super::net::FEED_ENV,
    crate::layout::WEBVIEW2_USER_DATA_ENV,
    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    crate::trace::TRACE_ENV,
];
const FORWARD_ARG: &str = "--forward=";

/// Whether this copy may install updates itself (pure; inputs read by [`current_allows_install`]).
pub fn in_app_install_allowed(
    mode: AppMode,
    exe_dir: &Path,
    uninstaller_exists: bool,
    install_location: Option<&str>,
) -> bool {
    mode == AppMode::Installed
        && uninstaller_exists
        && install_location
            .is_some_and(|l| same_dir(Path::new(l.trim().trim_matches('"')), exe_dir))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    !norm(a).is_empty() && norm(a) == norm(b)
}

/// The decision for the running process.
pub fn current_allows_install() -> bool {
    let layout = crate::layout::current();
    let Some(exe_dir) = layout.exe_dir.as_deref() else {
        return false;
    };
    in_app_install_allowed(
        layout.mode,
        exe_dir,
        exe_dir.join(UNINSTALLER).is_file(),
        read_install_location().as_deref(),
    )
}

/// `NAME=VALUE` lines as one hex token (no space, quote or `/` for NSIS to split on).
pub fn encode_forwarded(forwarded: &[(String, String)]) -> Result<String, String> {
    let mut text = String::new();
    for (name, value) in forwarded {
        if value.contains('\n') || name.contains(['\n', '=']) {
            return Err(format!("{name} can't be passed to the restarted app"));
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("{name}={value}"));
    }
    Ok(text.bytes().map(|b| format!("{b:02x}")).collect())
}

/// The installer arguments.
pub fn installer_arguments(
    exe_dir: &Path,
    forwarded: &[(String, String)],
) -> Result<String, String> {
    let mut args = String::from("/P /UPDATE /R");
    if !forwarded.is_empty() {
        args.push_str(&format!(
            " /ARGS {FORWARD_ARG}{}",
            encode_forwarded(forwarded)?
        ));
    }
    let dir = exe_dir.to_string_lossy();
    if dir.contains('"') {
        return Err("the install folder name can't be used".into());
    }
    // `/D=` must be last and is never quoted (NSIS rule); spaces are fine there.
    args.push_str(&format!(" /D={}", dir.trim_end_matches('\\')));
    Ok(args)
}

/// The forwarded variables currently set.
pub fn forwarded_now() -> Vec<(String, String)> {
    FORWARDED_ENV
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty())
                .map(|v| (name.to_string(), v))
        })
        .collect()
}

/// The `--forward=<hex>` argument, decoded; only allowed names; anything malformed is ignored.
pub fn parse_forwarded_env<I: IntoIterator<Item = String>>(args: I) -> Vec<(String, String)> {
    let Some(hex) = args
        .into_iter()
        .find_map(|a| a.trim().strip_prefix(FORWARD_ARG).map(str::to_string))
    else {
        return Vec::new();
    };
    let Some(text) = decode_hex(&hex).and_then(|b| String::from_utf8(b).ok()) else {
        return Vec::new();
    };
    text.split('\n')
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            (FORWARDED_ENV.contains(&name) && !value.is_empty())
                .then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) || hex.len() > 16 * 1024 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Applies forwarded variables to this process. Called first thing in `run()`, before anything
/// reads the environment.
pub fn apply_forwarded_env() {
    for (name, value) in parse_forwarded_env(std::env::args().skip(1)) {
        std::env::set_var(name, value);
    }
}

/// Starts the installer detached from this process (it outlives the app).
pub fn spawn_installer(setup: &Path, arguments: &str) -> std::io::Result<()> {
    let mut command = std::process::Command::new(setup);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command
            .raw_arg(arguments)
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(windows))]
    command.args(arguments.split(' '));
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

/// `HKCU\...\Uninstall\Cargo Line` → `InstallLocation`, if any.
#[cfg(windows)]
pub fn read_install_location() -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let key: Vec<u16> = UNINSTALL_KEY.encode_utf16().chain(Some(0)).collect();
    let value: Vec<u16> = "InstallLocation".encode_utf16().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: NUL-terminated names; `len` is the buffer size in bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
            Some(&mut len),
        )
    };
    if status.is_err() {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

#[cfg(not(windows))]
pub fn read_install_location() -> Option<String> {
    None
}

/// Where the setup file is downloaded: `<local data folder>\updates\`.
pub const UPDATES_DIR: &str = "updates";

pub fn updates_dir(local_data: &Path) -> PathBuf {
    local_data.join(UPDATES_DIR)
}
