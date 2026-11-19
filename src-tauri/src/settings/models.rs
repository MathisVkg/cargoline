//! Typed, validated application settings.

use serde::{Deserialize, Serialize};

/// Where a contract screenshot is taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    StarCitizenWindow,
    FullDisplay,
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
    /// Focused-app capture shortcut, e.g. `"Ctrl+Shift+C"`. None = not configured.
    pub capture_shortcut: Option<String>,
    /// Capture automatically when Game.log reports an accepted contract.
    /// Requires Game.log monitoring. Off by default.
    pub auto_capture_on_accept: bool,
    /// Informational ship selection (catalog id). Never linked to contracts or runs.
    pub selected_ship_id: Option<String>,
    /// Opt-in automatic update checks. Off by default.
    pub automatic_update_checks: bool,
    pub contract_grouping: ContractGrouping,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            game_log_monitoring_enabled: false,
            game_log_path: None,
            capture_source: CaptureSource::StarCitizenWindow,
            capture_delay_ms: 1_000,
            capture_shortcut: None,
            auto_capture_on_accept: false,
            selected_ship_id: None,
            automatic_update_checks: false,
            contract_grouping: ContractGrouping::ByLocation,
        }
    }
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

        if self.capture_delay_ms > CAPTURE_DELAY_MAX_MS {
            return Err(invalid("capture_delay_ms", "must be between 0 and 5000 ms"));
        }
        if !self.capture_delay_ms.is_multiple_of(CAPTURE_DELAY_STEP_MS) {
            return Err(invalid("capture_delay_ms", "must be a multiple of 100 ms"));
        }
        if self.auto_capture_on_accept && !self.game_log_monitoring_enabled {
            return Err(invalid(
                "auto_capture_on_accept",
                "requires Game.log monitoring to be enabled",
            ));
        }
        Ok(())
    }
}
