//! Contract data exchanged with the frontend.
//!
//! Inputs carry raw user (or, later, OCR) text; Rust parses and validates
//! them, so the form is independent of how it was populated.

use serde::{Deserialize, Serialize};

use crate::database::models::{BoxSourceQuality, ContractSource, ContractStatus};

/// A contract as submitted by the Add Contract form. Numeric fields are the
/// raw text the user typed; empty optional fields may be `null` or `""`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewContractInput {
    pub title: String,
    pub reward_auec: String,
    pub requirements: Vec<NewRequirementInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewRequirementInput {
    pub commodity_name: String,
    pub quantity_scu: String,
    #[serde(default)]
    pub max_box_size_scu: Option<String>,
    pub pickups: Vec<NewStopInput>,
    pub deliveries: Vec<NewStopInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewStopInput {
    pub location_name: String,
    #[serde(default)]
    pub quantity_scu: Option<String>,
}

/// A validation problem, addressed by the input path it concerns
/// (e.g. `requirements.0.pickups.1.locationName`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

/// Fully validated contract, ready to persist. Quantities are milli-SCU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidContract {
    pub title: String,
    pub reward_auec: i64,
    pub requirements: Vec<ValidRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidRequirement {
    pub commodity_name: String,
    pub quantity_scu_milli: i64,
    pub max_box_size_scu_milli: Option<i64>,
    pub pickups: Vec<ValidStop>,
    pub deliveries: Vec<ValidStop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidStop {
    pub location_name: String,
    /// `None` = quantity at this location unknown.
    pub quantity_scu_milli: Option<i64>,
}

/// When a contract was accepted, in UTC and local terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceTime {
    pub utc_ms: i64,
    /// Local calendar date `YYYY-MM-DD`, which selects the run.
    pub local_date: String,
    pub utc_offset_minutes: i32,
}

impl AcceptanceTime {
    /// The current moment in the system's local time zone.
    pub fn now_local() -> Self {
        let now = chrono::Local::now();
        Self {
            utc_ms: now.timestamp_millis(),
            local_date: now.format("%Y-%m-%d").to_string(),
            utc_offset_minutes: now.offset().local_minus_utc() / 60,
        }
    }
}

/// A saved contract with its nested cargo, as shown in the Contracts page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractDetail {
    pub id: i64,
    pub run: RunSummary,
    pub title: String,
    pub reward_auec: i64,
    pub status: ContractStatus,
    pub source: ContractSource,
    pub accepted_at: i64,
    /// UTC ms when completed or abandoned; None while active.
    pub finished_at: Option<i64>,
    /// Sum of requirement quantities only (stops are never added).
    pub total_scu_milli: i64,
    pub requirements: Vec<RequirementDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: i64,
    pub run_number: i64,
    pub run_date: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementDetail {
    pub id: i64,
    pub commodity_name: String,
    pub quantity_scu_milli: i64,
    pub max_box_size_scu_milli: Option<i64>,
    /// Stored box breakdown; empty when none is known.
    pub box_groups: Vec<BoxGroupDetail>,
    pub pickups: Vec<StopDetail>,
    pub deliveries: Vec<StopDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxGroupDetail {
    pub box_size_scu_milli: i64,
    pub box_count: i64,
    pub source_quality: BoxSourceQuality,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopDetail {
    pub id: i64,
    pub location_name: String,
    pub quantity_scu_milli: Option<i64>,
    /// Optional Pick Up / Turn In reminder: UTC ms when ticked, None if not.
    pub reminder_done_at: Option<i64>,
}
