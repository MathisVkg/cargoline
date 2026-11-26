//! Optional Game.log monitoring (Task 10). Off by default; with it off the app behaves exactly as
//! before. Never changes contract data on its own: it only reports events to the frontend.
//!
//! - [`patterns`]: every line format, in one place.
//! - [`tracker`]: hauling filter, dedupe, first-objective pairing (pure).
//! - [`tail`]: polling file reader (start at the end, rotation, partial lines).
//! - [`service`]: the background thread, its live config and status, and the Tauri events.

pub mod detect;
pub mod patterns;
pub mod service;
pub mod tail;
pub mod tracker;

use serde::Serialize;

/// The first objective, resolved against the same lists as the parser (for the cross-check).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstObjective {
    pub scu: u32,
    pub commodity: String,
    pub location: String,
    /// The log names a star system, not the destination (interstellar contracts: "… to Pyro
    /// System"). The location is then not compared with the form (Task 16).
    pub location_is_system: bool,
}

/// Sent to the frontend as the `gamelog://event` Tauri event. Only the MissionId, title, first
/// objective, state and timestamps: never raw lines.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GameLogEvent {
    Accepted {
        mission_id: String,
        title: String,
        at: Option<String>,
        objective: Option<FirstObjective>,
        /// The "Automatic capture on accept" setting when the event was read.
        auto_capture: bool,
    },
    Objective {
        mission_id: String,
        objective: FirstObjective,
    },
    /// A mission of an **active** contract ended in game. The backend never finalises: the
    /// frontend acts only on `MISSION_STATE_COMPLETED` and only per `complete_from_game`.
    Ended {
        mission_id: String,
        state: String,
        at: Option<String>,
        contract_id: i64,
        /// The "Complete contracts finished in game" setting when the event was read.
        complete_from_game: CompleteFromGame,
    },
}

/// The "Complete contracts finished in game" setting, as carried by an end event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompleteFromGame {
    /// Off: a hint with Mark complete (Task 10 behaviour).
    #[default]
    Off,
    Ask,
    Automatic,
}

impl CompleteFromGame {
    pub fn from_settings(settings: &crate::settings::AppSettings) -> Self {
        use crate::settings::CompleteFromGameMode;
        match (
            settings.complete_from_game,
            settings.complete_from_game_mode,
        ) {
            (false, _) => Self::Off,
            (true, CompleteFromGameMode::Ask) => Self::Ask,
            (true, CompleteFromGameMode::Automatic) => Self::Automatic,
        }
    }
}

/// The status line under the Game.log path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WatchStatus {
    Off,
    Watching { path: String },
    FileNotFound { path: String },
    Unreadable { path: String, reason: String },
}
