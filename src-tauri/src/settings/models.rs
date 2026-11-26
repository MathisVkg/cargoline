//! Typed, validated application settings.

use serde::{Deserialize, Serialize};

/// Where a contract screenshot is taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    StarCitizenWindow,
    FullDisplay,
}

/// What to do when Game.log reports a contract completed (Task 11 Part B).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompleteFromGameMode {
    /// Open a confirmation listing the contracts the game reports as completed.
    #[default]
    Ask,
    /// Mark them complete without asking (with a 30 s Undo).
    Automatic,
}

/// How the Contracts page groups active contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractGrouping {
    /// Operational view: cargo stops grouped by location. Earlier builds
    /// stored this as `by_stop` (or `locations`); both still load.
    #[serde(alias = "by_stop", alias = "locations")]
    ByLocation,
    ByContract,
}

/// Capture delay bounds: 0.0–5.0 s in 0.1 s steps.
pub const CAPTURE_DELAY_MAX_MS: u32 = 5_000;
pub const CAPTURE_DELAY_STEP_MS: u32 = 100;

const MAX_PATH_CHARS: usize = 32_767;
const MAX_SHORTCUT_CHARS: usize = 64;
const MAX_SHIP_ID_CHARS: usize = 128;

/// Persistent local settings. Every field has a safe default; unknown keys
/// in storage are ignored and missing keys fall back to their defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSettings {
    /// Optional Game.log monitoring. Off by default; never required.
    pub game_log_monitoring_enabled: bool,
    /// Path to Game.log, if configured. Not checked for existence here.
    pub game_log_path: Option<String>,
    pub capture_source: CaptureSource,
    /// Delay before capture, in milliseconds (multiple of 100, at most 5000).
    pub capture_delay_ms: u32,
    /// Capture a screenshot (after the delay) when the Add Contract form opens. On by default.
    pub capture_on_add_contract: bool,
    /// Focused-app capture shortcut, e.g. `"Ctrl+Shift+C"`. None = not configured.
    pub capture_shortcut: Option<String>,
    /// Capture automatically when Game.log reports an accepted contract.
    /// Requires Game.log monitoring. Off by default.
    pub auto_capture_on_accept: bool,
    /// Complete contracts that Game.log reports as completed. Requires Game.log monitoring.
    /// Off by default (the hint with a Mark complete button stays as before).
    pub complete_from_game: bool,
    pub complete_from_game_mode: CompleteFromGameMode,
    /// Informational ship selection (catalog id). Never linked to contracts or runs.
    pub selected_ship_id: Option<String>,
    /// Check GitHub for a newer version about 10 s after start, at most once a day (Task 20).
    /// On by default. (Replaces the unused opt-in `automatic_update_checks` key of earlier
    /// builds, which is ignored if stored.)
    pub check_updates_automatically: bool,
    /// When the last update check ran (UTC ms), automatic or manual.
    pub update_last_check_ms: Option<i64>,
    /// "Skip this version": the top-bar marker stays hidden for this exact version.
    pub update_skipped_version: Option<String>,
    /// Set just before the installer runs; the next start turns it into "Updated to version X".
    pub update_installing_version: Option<String>,
    pub contract_grouping: ContractGrouping,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            game_log_monitoring_enabled: false,
            game_log_path: None,
            capture_source: CaptureSource::StarCitizenWindow,
            capture_delay_ms: 1_000,
            capture_on_add_contract: true,
            capture_shortcut: None,
            auto_capture_on_accept: false,
            complete_from_game: false,
            complete_from_game_mode: CompleteFromGameMode::Ask,
            selected_ship_id: None,
            check_updates_automatically: true,
            update_last_check_ms: None,
            update_skipped_version: None,
            update_installing_version: None,
            contract_grouping: ContractGrouping::ByLocation,
        }
    }
}

/// Combinations the app or the web view already uses (editing, reload, find, print, close).
pub const RESERVED_SHORTCUTS: &[&str] = &[
    "Ctrl+A",
    "Ctrl+C",
    "Ctrl+V",
    "Ctrl+X",
    "Ctrl+Z",
    "Ctrl+Y",
    "Ctrl+Shift+Z",
    "Ctrl+R",
    "Ctrl+Shift+R",
    "Ctrl+F",
    "Ctrl+P",
    "Ctrl+W",
    "Ctrl+S",
    "Alt+F4",
    "Ctrl+Shift+I",
    "Ctrl+Shift+J",
];

/// `[Ctrl+][Alt+][Shift+]<Key>`, in that order. Key: `A`–`Z`, `0`–`9` or `F1`–`F12`.
/// At least Ctrl or Alt (a plain key, or Shift + key, would be typing). Reserved combinations
/// are refused.
pub fn validate_shortcut(text: &str) -> Result<(), String> {
    let parts: Vec<&str> = text.split('+').collect();
    let (key, modifiers) = parts.split_last().ok_or("Press a key combination.")?;
    let order = ["Ctrl", "Alt", "Shift"];
    let mut last = None;
    for m in modifiers {
        let index = order
            .iter()
            .position(|o| o == m)
            .ok_or_else(|| format!("Unknown modifier \"{m}\"."))?;
        if last.is_some_and(|l| index <= l) {
            return Err("Write modifiers once, in the order Ctrl+Alt+Shift.".into());
        }
        last = Some(index);
    }
    let valid_key = (key.len() == 1
        && key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
        || key
            .strip_prefix('F')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=12).contains(&n));
    if !valid_key {
        return Err("Use a letter, a digit or F1–F12 with Ctrl or Alt.".into());
    }
    if !modifiers.iter().any(|m| *m == "Ctrl" || *m == "Alt") {
        return Err("Add Ctrl or Alt: a single key would be typed into fields.".into());
    }
    if RESERVED_SHORTCUTS.contains(&text) {
        return Err(format!("{text} is already used by the app."));
    }
    Ok(())
}

/// A setting value that failed validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid setting `{field}`: {reason}")]
pub struct InvalidSetting {
    pub field: &'static str,
    pub reason: String,
}

fn invalid(field: &'static str, reason: impl Into<String>) -> InvalidSetting {
    InvalidSetting {
        field,
        reason: reason.into(),
    }
}

/// Optional text must be non-blank, bounded and free of control characters.
fn check_text(
    field: &'static str,
    value: &Option<String>,
    max: usize,
) -> Result<(), InvalidSetting> {
    let Some(text) = value else { return Ok(()) };
    if text.trim().is_empty() {
        return Err(invalid(field, "must not be blank (use no value instead)"));
    }
    if text.trim() != text {
        return Err(invalid(field, "must not start or end with whitespace"));
    }
    if text.chars().count() > max {
        return Err(invalid(field, format!("must be at most {max} characters")));
    }
    if text.chars().any(char::is_control) {
        return Err(invalid(field, "must not contain control characters"));
    }
    Ok(())
}

impl AppSettings {
    /// Checks every field and cross-field rule.
    pub fn validate(&self) -> Result<(), InvalidSetting> {
        check_text("game_log_path", &self.game_log_path, MAX_PATH_CHARS)?;
        check_text(
            "capture_shortcut",
            &self.capture_shortcut,
            MAX_SHORTCUT_CHARS,
        )?;
        check_text(
            "selected_ship_id",
            &self.selected_ship_id,
            MAX_SHIP_ID_CHARS,
        )?;

        for (field, value) in [
            ("update_skipped_version", &self.update_skipped_version),
            ("update_installing_version", &self.update_installing_version),
        ] {
            if value
                .as_deref()
                .is_some_and(|v| semver::Version::parse(v).is_err())
            {
                return Err(invalid(field, "must be a semver version"));
            }
        }
        if self.update_last_check_ms.is_some_and(|t| t < 0) {
            return Err(invalid("update_last_check_ms", "must not be negative"));
        }
        if self.capture_delay_ms > CAPTURE_DELAY_MAX_MS {
            return Err(invalid("capture_delay_ms", "must be between 0 and 5000 ms"));
        }
        if !self.capture_delay_ms.is_multiple_of(CAPTURE_DELAY_STEP_MS) {
            return Err(invalid("capture_delay_ms", "must be a multiple of 100 ms"));
        }
        if let Some(path) = &self.game_log_path {
            let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
            if !file.eq_ignore_ascii_case("Game.log") {
                return Err(invalid(
                    "game_log_path",
                    "must point to a file named Game.log",
                ));
            }
        }
        if let Some(shortcut) = &self.capture_shortcut {
            validate_shortcut(shortcut).map_err(|reason| invalid("capture_shortcut", reason))?;
        }
        if self.auto_capture_on_accept && !self.game_log_monitoring_enabled {
            return Err(invalid(
                "auto_capture_on_accept",
                "requires Game.log monitoring to be enabled",
            ));
        }
        if self.complete_from_game && !self.game_log_monitoring_enabled {
            return Err(invalid(
                "complete_from_game",
                "requires Game.log monitoring to be enabled",
            ));
        }
        Ok(())
    }
}
