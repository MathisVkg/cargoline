//! Screenshots of the Star Citizen window (or its display) for the Add
//! Contract form.
//!
//! - [`select`]: which window is Star Citizen (pure, testable without a desktop).
//! - [`storage`]: PNG files under `captures\`, their metadata rows, linking and cleanup.
//! - `native` (Windows only): Windows.Graphics.Capture, the PrintWindow fallback and
//!   display capture.
//!
//! Capture never focuses, moves or resizes a window and never sends input.

pub mod select;
pub mod service;
pub mod storage;

#[cfg(windows)]
pub mod native;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// What was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSourceKind {
    /// The Star Citizen window only.
    Window,
    /// The whole display Star Citizen is on (or the primary display).
    Display,
}

impl CaptureSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Display => "display",
        }
    }
}

/// Which native API produced the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureBackend {
    /// Windows.Graphics.Capture.
    Wgc,
    /// `PrintWindow(PW_RENDERFULLCONTENT)`, used only when WGC fails.
    PrintWindow,
}

impl CaptureBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wgc => "wgc",
            Self::PrintWindow => "print_window",
        }
    }
}

/// One captured frame: tightly packed BGRA8 at native resolution.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub source: CaptureSourceKind,
    pub backend: CaptureBackend,
}

/// Why a capture did not produce an image.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("Star Citizen window not found")]
    WindowNotFound,
    #[error("Star Citizen is minimised")]
    Minimized,
    #[error("A capture is already running")]
    Busy,
    #[error("The capture took too long and was stopped")]
    Timeout,
    #[error("Screen capture is not supported on this system")]
    Unsupported,
    #[error("Capture failed: {0}")]
    Failed(String),
}

/// Allows one capture at a time across the whole app.
#[derive(Debug, Default)]
pub struct CaptureState {
    busy: Arc<AtomicBool>,
}

impl CaptureState {
    /// Claims the capture slot, or returns [`CaptureError::Busy`].
    pub fn try_begin(&self) -> Result<CaptureSlot, CaptureError> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| CaptureSlot {
                busy: Arc::clone(&self.busy),
            })
            .map_err(|_| CaptureError::Busy)
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}

/// Held while a capture runs, possibly by a capture thread that outlives a timed-out
/// request; frees the slot when dropped (also on panic).
#[derive(Debug)]
pub struct CaptureSlot {
    busy: Arc<AtomicBool>,
}

impl Drop for CaptureSlot {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::Release);
    }
}
