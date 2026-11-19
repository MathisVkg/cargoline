//! Picks the Star Citizen game window from a list of top-level windows.
//!
//! Kept free of Win32 calls so the rules can be tested with a mocked list.

use super::CaptureError;

pub const STAR_CITIZEN_EXE: &str = "StarCitizen.exe";
pub const STAR_CITIZEN_CLASS: &str = "CryENGINE";

/// A top-level window as seen by the enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowCandidate {
    /// Opaque native handle (an `HWND` on Windows).
    pub handle: isize,
    /// Executable file name of the owning process, e.g. `StarCitizen.exe`.
    pub exe: String,
    pub class: String,
    pub visible: bool,
    pub minimized: bool,
    /// Hidden by DWM (another virtual desktop, suspended app, …).
    pub cloaked: bool,
    pub width: i32,
    pub height: i32,
}

impl WindowCandidate {
    fn is_star_citizen(&self) -> bool {
        self.exe.eq_ignore_ascii_case(STAR_CITIZEN_EXE)
            && self.class.eq_ignore_ascii_case(STAR_CITIZEN_CLASS)
            && self.visible
            && !self.cloaked
    }

    fn area(&self) -> i64 {
        i64::from(self.width.max(0)) * i64::from(self.height.max(0))
    }
}

/// Rules:
/// 1. Only visible, uncloaked windows of `StarCitizen.exe` with class `CryENGINE`.
/// 2. None → [`CaptureError::WindowNotFound`].
/// 3. Among the ones not minimised, the largest wins (ties: first in the list).
/// 4. All of them minimised → [`CaptureError::Minimized`] (WGC delivers no frames).
pub fn select_star_citizen(windows: &[WindowCandidate]) -> Result<&WindowCandidate, CaptureError> {
    let matching: Vec<&WindowCandidate> = windows.iter().filter(|w| w.is_star_citizen()).collect();
    if matching.is_empty() {
        return Err(CaptureError::WindowNotFound);
    }
    matching
        .iter()
        .copied()
        .filter(|w| !w.minimized)
        .fold(None::<&WindowCandidate>, |best, w| match best {
            Some(b) if b.area() >= w.area() => Some(b),
            _ => Some(w),
        })
        .ok_or(CaptureError::Minimized)
}

/// Any Star Citizen window, minimised or not (display mode only needs its monitor).
pub fn any_star_citizen(windows: &[WindowCandidate]) -> Option<&WindowCandidate> {
    select_star_citizen(windows)
        .ok()
        .or_else(|| windows.iter().find(|w| w.is_star_citizen()))
}
