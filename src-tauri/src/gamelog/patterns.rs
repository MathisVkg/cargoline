//! Every Game.log line format Cargo Line understands, in one place, so a game patch only needs
//! changes here. Measured on a 4.10.200 log (Task 06 §B5, Task 10). Only the fields below are
//! extracted; the rest of each line (handle, account ids, IP-like values) is never kept.
//!
//! ```text
//! <ts> [Notice] <SHUDEvent_OnNotification> Added notification "Contract Accepted:  <title>: " [11] to queue. … MissionId: [<uuid>], ObjectiveId: [] …
//! <ts> [Notice] <CLocalMissionPhaseMarker::CreateMarker> Creating objective marker: missionId [<uuid>], generator name [Covalex_Hauling], contract [HaulCargo_AToB_…], …
//! <ts> [Notice] <SHUDEvent_OnNotification> Added notification "New Objective: Deliver 0/84 SCU of Aluminum to Baijini Point: " [12] … MissionId: [<uuid>], …
//! <ts> [Notice] <MissionEnded> Received MissionEnded push message for: mission_id <uuid> - mission_state MISSION_STATE_COMPLETED …
//! ```
//! The accepted notification is echoed on lines without a MissionId: only the line carrying
//! one counts.

/// Contract template prefixes treated as hauling (`contract [<prefix>_…]`). The Task 06 log only
/// contains `HaulCargo` (generator `Covalex_Hauling`); salvage uses `Adaigo` and is ignored.
pub const HAULING_TEMPLATE_PREFIXES: &[&str] = &["HaulCargo"];
pub const STATE_COMPLETED: &str = "MISSION_STATE_COMPLETED";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogLine {
    Accepted {
        mission_id: String,
        title: String,
        at: Option<String>,
    },
    Marker {
        mission_id: String,
        template: String,
    },
    Objective {
        mission_id: String,
        text: String,
    },
    Ended {
        mission_id: String,
        state: String,
        at: Option<String>,
    },
}

/// `<2026-11-19T12:10:58.978Z>` at the start of a line.
fn timestamp(line: &str) -> Option<String> {
    let rest = line.strip_prefix('<')?;
    let end = rest.find('>')?;
    let ts = &rest[..end];
    (ts.len() <= 40 && ts.contains('T')).then(|| ts.to_string())
}

/// `8-4-4-4-12` hex digits.
pub fn is_mission_id(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The text inside `<key>[` … `]`, if it is a mission id.
fn bracketed_id(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = line[start..].trim_start().strip_prefix('[')?;
    let id = &rest[..rest.find(']')?];
    is_mission_id(id).then(|| id.to_ascii_lowercase())
}

/// The quoted notification text after `Added notification "`, without the closing `: ` / ` : `.
fn notification(line: &str, prefix: &str) -> Option<String> {
    let marker = format!("Added notification \"{prefix}");
    let start = line.find(&marker)? + marker.len();
    let rest = &line[start..];
    let text = &rest[..rest.find('"')?];
    let text = text.trim().trim_end_matches(':').trim_end();
    (!text.is_empty()).then(|| text.to_string())
}

pub fn parse_line(line: &str) -> Option<LogLine> {
    if line.contains("Added notification \"Contract Accepted:") {
        let mission_id = bracketed_id(line, "MissionId:")?; // echoes have no MissionId
        let title = notification(line, "Contract Accepted:")?;
        return Some(LogLine::Accepted {
            mission_id,
            title,
            at: timestamp(line),
        });
    }
    if line.contains("CreateMarker") {
        let mission_id = bracketed_id(line, "missionId")?;
        let start = line.find("contract [")? + "contract [".len();
        let template = &line[start..start + line[start..].find(']')?];
        return Some(LogLine::Marker {
            mission_id,
            template: template.to_string(),
        });
    }
    if line.contains("Added notification \"New Objective:") {
        let mission_id = bracketed_id(line, "MissionId:")?;
        let text = notification(line, "New Objective:")?;
        return Some(LogLine::Objective { mission_id, text });
    }
    if line.contains("<MissionEnded>") {
        let start = line.find("mission_id ")? + "mission_id ".len();
        let id = line[start..].split_whitespace().next()?;
        if !is_mission_id(id) {
            return None;
        }
        let s = line.find("mission_state ")? + "mission_state ".len();
        let state: String = line[s..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if state.is_empty() {
            return None;
        }
        return Some(LogLine::Ended {
            mission_id: id.to_ascii_lowercase(),
            state,
            at: timestamp(line),
        });
    }
    None
}

pub fn is_hauling_template(template: &str) -> bool {
    HAULING_TEMPLATE_PREFIXES.iter().any(|p| {
        template
            .strip_prefix(p)
            .is_some_and(|rest| rest.starts_with('_'))
    })
}

/// `Deliver 0/84 SCU of Aluminum to Baijini Point` → (84, "Aluminum", "Baijini Point").
pub fn parse_first_objective(text: &str) -> Option<(u32, String, String)> {
    let rest = text.strip_prefix("Deliver ")?;
    let (progress, rest) = rest.split_once(' ')?;
    let total: u32 = progress.split_once('/')?.1.parse().ok()?;
    let rest = rest.strip_prefix("SCU of ")?;
    let (commodity, location) = rest.split_once(" to ")?;
    let location = location.trim().trim_end_matches('.');
    (total > 0 && !commodity.trim().is_empty() && !location.is_empty())
        .then(|| (total, commodity.trim().to_string(), location.to_string()))
}
