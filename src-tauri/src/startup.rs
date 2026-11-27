//! Startup ordering: background work that is not needed for the first screen waits
//! until the first contracts list has been served, so it never competes with it for the
//! database connection. It still runs at every startup: the wait has a time limit.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// How long deferred startup work waits for the first screen at most.
pub const FIRST_SCREEN_WAIT: Duration = Duration::from_secs(5);

/// Opened once the first contracts list has been served.
#[derive(Debug, Default)]
pub struct FirstScreenGate {
    opened: Mutex<bool>,
    changed: Condvar,
}

/// Why deferred work went ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// The first list was served.
    Opened,
    /// The time limit passed first (no window, or the list failed): run anyway.
    TimedOut,
}

impl FirstScreenGate {
    /// Marks the first screen as served and wakes the waiters. Later calls do nothing.
    pub fn open(&self) {
        if let Ok(mut opened) = self.opened.lock() {
            if !*opened {
                *opened = true;
                self.changed.notify_all();
            }
        }
    }

    pub fn is_open(&self) -> bool {
        self.opened.lock().map(|o| *o).unwrap_or(true)
    }

    /// Waits until [`Self::open`] or `timeout`. Call from a background thread only.
    pub fn wait(&self, timeout: Duration) -> GateOutcome {
        let Ok(guard) = self.opened.lock() else {
            return GateOutcome::TimedOut;
        };
        match self
            .changed
            .wait_timeout_while(guard, timeout, |opened| !*opened)
        {
            Ok((opened, _)) if *opened => GateOutcome::Opened,
            _ => GateOutcome::TimedOut,
        }
    }
}
