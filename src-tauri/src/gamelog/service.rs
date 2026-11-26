//! The Game.log watcher thread: polls about every 500 ms, applies setting changes live (start,
//! stop, path), keeps a status, and emits [`GameLogEvent`]s. Errors only change the status.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::patterns::{parse_line, STATE_COMPLETED};
use super::tail::{Tail, TailError};
use super::tracker::{RawObjective, TrackedEvent, Tracker};
use super::{CompleteFromGame, FirstObjective, GameLogEvent, WatchStatus};
use crate::database::DatabaseState;
use crate::parser::References;

pub const POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const EVENT_NAME: &str = "gamelog://event";
pub const STATUS_EVENT_NAME: &str = "gamelog://status";
const READY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchConfig {
    pub enabled: bool,
    pub path: Option<String>,
    pub auto_capture: bool,
    pub complete: CompleteFromGame,
}

#[derive(Debug)]
struct Shared {
    config: WatchConfig,
    generation: u64,
    status: WatchStatus,
}

/// Shared between the commands and the watcher thread.
#[derive(Debug, Clone)]
pub struct GameLogState(Arc<Mutex<Shared>>);

impl Default for GameLogState {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Shared {
            config: WatchConfig::default(),
            generation: 0,
            status: WatchStatus::Off,
        })))
    }
}

impl GameLogState {
    /// Applies new settings; the watcher picks them up at its next poll (no restart needed).
    pub fn configure(&self, config: WatchConfig) {
        if let Ok(mut shared) = self.0.lock() {
            if shared.config != config {
                shared.config = config;
                shared.generation += 1;
            }
        }
    }

    pub fn status(&self) -> WatchStatus {
        self.0
            .lock()
            .map(|s| s.status.clone())
            .unwrap_or(WatchStatus::Off)
    }

    fn snapshot(&self) -> (WatchConfig, u64) {
        self.0
            .lock()
            .map(|s| (s.config.clone(), s.generation))
            .unwrap_or_default()
    }

    /// Returns true when the status changed.
    fn set_status(&self, status: WatchStatus) -> bool {
        match self.0.lock() {
            Ok(mut shared) if shared.status != status => {
                shared.status = status;
                true
            }
            _ => false,
        }
    }
}

/// Whether a contract (active or not) already carries this MissionId.
pub fn mission_known(conn: &Connection, mission_id: &str) -> rusqlite::Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM contracts WHERE game_mission_id = ?1 LIMIT 1",
            [mission_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// The active contract carrying this MissionId, if any.
pub fn active_contract_for(conn: &Connection, mission_id: &str) -> rusqlite::Result<Option<i64>> {
    Ok(active_contract_with_time(conn, mission_id)?.map(|(id, _)| id))
}

/// The active contract carrying this MissionId, with its `accepted_at` (UTC ms).
fn active_contract_with_time(
    conn: &Connection,
    mission_id: &str,
) -> rusqlite::Result<Option<(i64, i64)>> {
    conn.query_row(
        "SELECT id, accepted_at FROM contracts WHERE game_mission_id = ?1 AND status = 'active' ORDER BY id LIMIT 1",
        [mission_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// Game.log timestamp (`2026-11-19T12:10:58.978Z`) as UTC ms. None when it does not parse.
pub fn log_time_ms(at: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|t| t.timestamp_millis())
}

/// An end event older than the contract's acceptance belongs to an earlier session and is
/// ignored. A missing or unreadable timestamp is not a reason to drop it.
pub fn ended_before_accept(at: Option<&str>, accepted_at_ms: i64) -> bool {
    at.and_then(log_time_ms).is_some_and(|t| t < accepted_at_ms)
}

/// "Pyro System", "Stanton system", or a bare system name of the bundled list ("Pyro").
pub fn is_system_name(location: &str) -> bool {
    let name = location.trim();
    let lower = name.to_lowercase();
    if lower.ends_with(" system") && name.len() > " system".len() {
        return true;
    }
    crate::reference::locations().is_ok_and(|list| {
        list.items
            .iter()
            .filter_map(|l| l.system.as_deref())
            .any(|s| s.eq_ignore_ascii_case(name))
    })
}

fn resolve_objective(raw: RawObjective, refs: &References) -> FirstObjective {
    let location_is_system = is_system_name(&raw.location);
    FirstObjective {
        scu: raw.scu,
        commodity: refs.commodities.resolve(&raw.commodity, false).0,
        location: if location_is_system {
            raw.location.trim().to_string()
        } else {
            refs.locations.resolve(&raw.location, true).0
        },
        location_is_system,
    }
}

fn references(conn: &Connection) -> References {
    let locations: Vec<String> = crate::locations::list_locations(conn)
        .map(|l| l.into_iter().map(|e| e.name).collect())
        .unwrap_or_default();
    let commodities = crate::locations::list_used_commodities(conn).unwrap_or_default();
    References::build(&locations, &commodities)
}

/// Turns tracked events into app events: drops accepts already saved as contracts, keeps ends
/// of active contracts only, resolves first objectives.
pub fn to_app_events(
    conn: &Connection,
    events: Vec<TrackedEvent>,
    auto_capture: bool,
    complete: CompleteFromGame,
) -> Vec<GameLogEvent> {
    let mut refs: Option<References> = None;
    let mut out = Vec::new();
    for event in events {
        match event {
            TrackedEvent::Accepted {
                mission_id,
                title,
                at,
                objective,
            } => {
                if mission_known(conn, &mission_id).unwrap_or(false) {
                    continue;
                }
                let objective = objective
                    .map(|o| resolve_objective(o, refs.get_or_insert_with(|| references(conn))));
                out.push(GameLogEvent::Accepted {
                    mission_id,
                    title,
                    at,
                    objective,
                    auto_capture,
                });
            }
            TrackedEvent::Objective {
                mission_id,
                objective,
            } => {
                let objective =
                    resolve_objective(objective, refs.get_or_insert_with(|| references(conn)));
                out.push(GameLogEvent::Objective {
                    mission_id,
                    objective,
                });
            }
            TrackedEvent::Ended {
                mission_id,
                state,
                at,
            } => {
                if state != STATE_COMPLETED {
                    // State names only (no ids, no personal data): collected for later patches.
                    eprintln!("[cargo-line] Game.log: mission ended with state {state}");
                }
                if let Ok(Some((contract_id, accepted_at))) =
                    active_contract_with_time(conn, &mission_id)
                {
                    if ended_before_accept(at.as_deref(), accepted_at) {
                        continue;
                    }
                    out.push(GameLogEvent::Ended {
                        mission_id,
                        state,
                        at,
                        contract_id,
                        complete_from_game: complete,
                    });
                }
            }
        }
    }
    out
}

/// Starts the watcher thread (idle until monitoring is enabled).
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("gamelog-watcher".into())
        .spawn(move || {
            let state = app.state::<GameLogState>().inner().clone();
            let mut tail: Option<Tail> = None;
            let mut tracker = Tracker::default();
            let mut seen_generation = u64::MAX;
            crate::trace::mark("watcher thread started");
            loop {
                let (config, generation) = state.snapshot();
                if generation != seen_generation {
                    seen_generation = generation;
                    // A new path or a restart starts again at the end of the file.
                    tail = match (&config.enabled, &config.path) {
                        (true, Some(path)) => Some(Tail::new(path)),
                        _ => None,
                    };
                    if tail.is_none() && state.set_status(WatchStatus::Off) {
                        let _ = app.emit(STATUS_EVENT_NAME, WatchStatus::Off);
                    }
                }
                if let Some(t) = tail.as_mut() {
                    let path = t.path().display().to_string();
                    let polled = t.poll();
                    crate::trace::mark_once("watcher first poll");
                    let status = match polled {
                        Ok(lines) => {
                            for line in &lines {
                                if let Some(parsed) = parse_line(line) {
                                    tracker.feed(parsed);
                                }
                            }
                            let events = tracker.flush();
                            if !events.is_empty() {
                                emit_events(&app, events, &config);
                            }
                            WatchStatus::Watching { path }
                        }
                        Err(TailError::NotFound) => WatchStatus::FileNotFound { path },
                        Err(TailError::Unreadable(reason)) => {
                            WatchStatus::Unreadable { path, reason }
                        }
                    };
                    if state.set_status(status.clone()) {
                        let _ = app.emit(STATUS_EVENT_NAME, status);
                    }
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        });
    if let Err(e) = spawned {
        eprintln!("[cargo-line] could not start the Game.log watcher: {e}");
    }
}

fn emit_events<R: Runtime>(app: &AppHandle<R>, events: Vec<TrackedEvent>, config: &WatchConfig) {
    let db_state = app.state::<DatabaseState>();
    let Ok(db) = db_state.wait_ready(READY_TIMEOUT) else {
        return;
    };
    let Ok(conn) = db.connection() else { return };
    let app_events = to_app_events(&conn, events, config.auto_capture, config.complete);
    drop(conn);
    for event in app_events {
        let _ = app.emit(EVENT_NAME, event);
    }
}

/// Loads the saved settings into the watcher at startup.
pub fn configure_from_settings(state: &GameLogState, settings: &crate::settings::AppSettings) {
    state.configure(WatchConfig {
        enabled: settings.game_log_monitoring_enabled,
        path: settings.game_log_path.clone(),
        auto_capture: settings.auto_capture_on_accept,
        complete: CompleteFromGame::from_settings(settings),
    });
}
