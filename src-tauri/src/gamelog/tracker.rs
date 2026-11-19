//! Turns parsed lines into game events: hauling filter, dedupe by MissionId, and the first
//! objective paired with its accept. Pure (no IO), so it is tested line by line.
//!
//! Accepts are held until the end of each read batch: the hauling marker comes a few ms before
//! the accept and the first objective a few ms after, so both are normally in the same batch.

use std::collections::{HashMap, HashSet};

use super::patterns::{is_hauling_template, LogLine};

/// The first `Deliver …` objective of a mission, as logged (resolution happens later).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawObjective {
    pub scu: u32,
    pub commodity: String,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackedEvent {
    /// A hauling contract was accepted in game (once per MissionId).
    Accepted {
        mission_id: String,
        title: String,
        at: Option<String>,
        objective: Option<RawObjective>,
    },
    /// The first objective arrived after its accept was already reported.
    Objective {
        mission_id: String,
        objective: RawObjective,
    },
    /// A mission ended (any contract type; the app keeps only its own active contracts).
    Ended {
        mission_id: String,
        state: String,
        at: Option<String>,
    },
}

#[derive(Debug, Default)]
pub struct Tracker {
    templates: HashMap<String, String>,
    accepted: HashSet<String>,
    reported: HashSet<String>,
    with_objective: HashSet<String>,
    ended: HashSet<String>,
    pending: Vec<(String, String, Option<String>, Option<RawObjective>)>,
    out: Vec<TrackedEvent>,
}

/// Bounds memory on very long sessions; template markers are only needed for a few ms.
const MAX_TEMPLATES: usize = 2_000;

impl Tracker {
    pub fn feed(&mut self, line: LogLine) {
        match line {
            LogLine::Marker {
                mission_id,
                template,
            } => {
                if self.templates.len() >= MAX_TEMPLATES {
                    self.templates.clear();
                }
                self.templates.entry(mission_id).or_insert(template);
            }
            LogLine::Accepted {
                mission_id,
                title,
                at,
            } => {
                if self.accepted.insert(mission_id.clone()) {
                    self.pending.push((mission_id, title, at, None));
                }
            }
            LogLine::Objective { mission_id, text } => {
                let Some((scu, commodity, location)) =
                    super::patterns::parse_first_objective(&text)
                else {
                    return;
                };
                if !self.with_objective.insert(mission_id.clone()) {
                    return; // only the first Deliver objective
                }
                let objective = RawObjective {
                    scu,
                    commodity,
                    location,
                };
                if let Some(p) = self.pending.iter_mut().find(|p| p.0 == mission_id) {
                    p.3 = Some(objective);
                } else if self.reported.contains(&mission_id) {
                    self.out.push(TrackedEvent::Objective {
                        mission_id,
                        objective,
                    });
                } else {
                    self.with_objective.remove(&mission_id); // not ours (yet): allow a later one
                }
            }
            LogLine::Ended {
                mission_id,
                state,
                at,
            } => {
                if self.ended.insert(mission_id.clone()) {
                    self.out.push(TrackedEvent::Ended {
                        mission_id,
                        state,
                        at,
                    });
                }
            }
        }
    }

    /// Ends a read batch: reports pending hauling accepts (non-hauling ones are dropped).
    pub fn flush(&mut self) -> Vec<TrackedEvent> {
        for (mission_id, title, at, objective) in std::mem::take(&mut self.pending) {
            let hauling = self
                .templates
                .get(&mission_id)
                .is_some_and(|t| is_hauling_template(t));
            if hauling {
                self.reported.insert(mission_id.clone());
                self.out.push(TrackedEvent::Accepted {
                    mission_id,
                    title,
                    at,
                    objective,
                });
            }
        }
        std::mem::take(&mut self.out)
    }

    /// Mission ids already handled (e.g. saved contracts) are never reported again.
    pub fn mark_known(&mut self, mission_id: &str) {
        self.accepted.insert(mission_id.to_string());
    }
}
