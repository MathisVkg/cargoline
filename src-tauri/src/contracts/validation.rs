//! Authoritative validation of new contracts.
//!
//! The React form mirrors these rules for immediate feedback, but only
//! values accepted here are ever persisted. Numbers are parsed from text
//! with integer arithmetic: no floating point, no silent rounding.

use std::collections::HashSet;

use super::models::*;
use crate::database::models::MILLI_SCU_PER_SCU;

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_COMMODITY_CHARS: usize = 100;
pub const MAX_LOCATION_CHARS: usize = 200;
pub const MAX_REQUIREMENTS: usize = 50;
pub const MAX_STOPS_PER_OPERATION: usize = 20;
/// Largest accepted reward (1 trillion aUEC).
pub const MAX_REWARD_AUEC: i64 = 1_000_000_000_000;
/// Largest accepted single quantity (1 million SCU).
pub const MAX_SCU_MILLI: i64 = 1_000_000 * MILLI_SCU_PER_SCU;

/// Parses a non-negative decimal SCU amount ("12", "4.5", "0,125") into
/// milli-SCU. At most three decimal places are accepted.
pub fn parse_scu_milli(text: &str) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter a quantity.".into());
    }
    let normalized = text.replacen(',', ".", 1);
    let (whole, fraction) = match normalized.split_once('.') {
        Some((w, f)) => (w, f),
        None => (normalized.as_str(), ""),
    };
    let digits_only = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty()
        || !digits_only(whole)
        || !digits_only(fraction)
        || (normalized.contains('.') && fraction.is_empty())
    {
        return Err("Enter a number such as 12 or 4.5.".into());
    }
    if fraction.len() > 3 {
        return Err("Use at most 3 decimal places.".into());
    }
    let whole = whole.trim_start_matches('0');
    if whole.len() > 7 {
        return Err("Quantity is too large.".into());
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| "Quantity is too large.")?
    };
    let fraction: i64 = format!("{fraction:0<3}")
        .parse()
        .map_err(|_| "Enter a valid number.")?;
    let milli = whole * MILLI_SCU_PER_SCU + fraction;
    if milli > MAX_SCU_MILLI {
        return Err("Quantity is too large.".into());
    }
    Ok(milli)
}

/// Parses a whole aUEC amount. Digits only, optionally grouped in threes
/// with spaces or commas ("48250", "48,250", "48 250").
pub fn parse_reward_auec(text: &str) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter the reward (0 if none).".into());
    }
    let separator = text.chars().find(|c| *c == ',' || *c == ' ');
    let digits: String = match separator {
        None => text.to_string(),
        Some(sep) => {
            let groups: Vec<&str> = text.split(sep).collect();
            let grouped = groups[0].len() <= 3
                && !groups[0].is_empty()
                && groups[1..].iter().all(|g| g.len() == 3);
            if !grouped {
                return Err("Enter a whole number, such as 48250.".into());
            }
            groups.concat()
        }
    };
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Enter a whole number, such as 48250.".into());
    }
    let digits = digits.trim_start_matches('0');
    if digits.len() > 13 {
        return Err("Reward is too large.".into());
    }
    let value: i64 = if digits.is_empty() {
        0
    } else {
        digits.parse().map_err(|_| "Reward is too large.")?
    };
    if value > MAX_REWARD_AUEC {
        return Err("Reward is too large.".into());
    }
    Ok(value)
}

struct Issues(Vec<ValidationIssue>);

impl Issues {
    fn add(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.0.push(ValidationIssue {
            path: path.into(),
            message: message.into(),
        });
    }

    /// Trimmed required text, or an issue.
    fn text(&mut self, path: String, value: &str, label: &str, max: usize) -> String {
        let value = value.trim();
        if value.is_empty() {
            self.add(path, format!("{label} is required."));
        } else if value.chars().count() > max {
            self.add(path, format!("{label} must be at most {max} characters."));
        } else if value.chars().any(char::is_control) {
            self.add(path, format!("{label} contains invalid characters."));
        }
        value.to_string()
    }
}

fn optional_text(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// Validates a submitted contract, reporting every problem at once.
pub fn validate_new_contract(
    input: &NewContractInput,
) -> Result<ValidContract, Vec<ValidationIssue>> {
    let mut issues = Issues(Vec::new());

    let title = issues.text(
        "title".into(),
        &input.title,
        "Contract title",
        MAX_TITLE_CHARS,
    );
    let reward_auec = parse_reward_auec(&input.reward_auec)
        .map_err(|m| issues.add("rewardAuec", m))
        .unwrap_or(0);

    if input.requirements.is_empty() {
        issues.add("requirements", "Add at least one cargo objective.");
    } else if input.requirements.len() > MAX_REQUIREMENTS {
        issues.add(
            "requirements",
            format!("At most {MAX_REQUIREMENTS} cargo objectives are supported."),
        );
    }

    let requirements = input
        .requirements
        .iter()
        .enumerate()
        .map(|(i, req)| validate_requirement(&mut issues, &format!("requirements.{i}"), req))
        .collect();

    if issues.0.is_empty() {
        Ok(ValidContract {
            title,
            reward_auec,
            requirements,
        })
    } else {
        Err(issues.0)
    }
}

fn validate_requirement(
    issues: &mut Issues,
    path: &str,
    req: &NewRequirementInput,
) -> ValidRequirement {
    let commodity_name = issues.text(
        format!("{path}.commodityName"),
        &req.commodity_name,
        "Commodity",
        MAX_COMMODITY_CHARS,
    );

    let quantity = match parse_scu_milli(&req.quantity_scu) {
        Ok(0) => {
            issues.add(
                format!("{path}.quantityScu"),
                "Total SCU must be greater than 0.",
            );
            None
        }
        Ok(v) => Some(v),
        Err(m) => {
            issues.add(format!("{path}.quantityScu"), m);
            None
        }
    };

    let max_box_size_scu_milli =
        optional_text(&req.max_box_size_scu).and_then(|text| match parse_scu_milli(text) {
            Ok(0) => {
                issues.add(
                    format!("{path}.maxBoxSizeScu"),
                    "Max box size must be greater than 0.",
                );
                None
            }
            Ok(v) => Some(v),
            Err(m) => {
                issues.add(format!("{path}.maxBoxSizeScu"), m);
                None
            }
        });

    let pickups = validate_stops(
        issues,
        &format!("{path}.pickups"),
        &req.pickups,
        "pickup",
        quantity,
    );
    let deliveries = validate_stops(
        issues,
        &format!("{path}.deliveries"),
        &req.deliveries,
        "delivery",
        quantity,
    );

    ValidRequirement {
        commodity_name,
        quantity_scu_milli: quantity.unwrap_or(0),
        max_box_size_scu_milli,
        pickups,
        deliveries,
    }
}

fn validate_stops(
    issues: &mut Issues,
    path: &str,
    stops: &[NewStopInput],
    kind: &str,
    total: Option<i64>,
) -> Vec<ValidStop> {
    if stops.is_empty() {
        issues.add(path, format!("Add at least one {kind} location."));
    } else if stops.len() > MAX_STOPS_PER_OPERATION {
        issues.add(
            path,
            format!("At most {MAX_STOPS_PER_OPERATION} {kind} locations are supported."),
        );
    }

    let mut seen = HashSet::new();
    let mut quantity_ok = true;
    let valid: Vec<ValidStop> = stops
        .iter()
        .enumerate()
        .map(|(i, stop)| {
            let location_name = crate::locations::normalize_display(&issues.text(
                format!("{path}.{i}.locationName"),
                &stop.location_name,
                "Location",
                MAX_LOCATION_CHARS,
            ));
            if !location_name.is_empty()
                && !seen.insert(crate::locations::normalize_key(&location_name))
            {
                issues.add(
                    format!("{path}.{i}.locationName"),
                    format!("This {kind} location is listed twice."),
                );
            }
            let quantity_scu_milli =
                optional_text(&stop.quantity_scu).and_then(|text| match parse_scu_milli(text) {
                    Ok(0) => {
                        quantity_ok = false;
                        issues.add(
                            format!("{path}.{i}.quantityScu"),
                            "Leave empty if unknown, or enter more than 0.",
                        );
                        None
                    }
                    Ok(v) => Some(v),
                    Err(m) => {
                        quantity_ok = false;
                        issues.add(format!("{path}.{i}.quantityScu"), m);
                        None
                    }
                });
            ValidStop {
                location_name,
                quantity_scu_milli,
            }
        })
        .collect();

    // Known allocations must agree with the requirement total; unknown ones stay unknown.
    if let (Some(total), true) = (total, quantity_ok && !valid.is_empty()) {
        let known: i64 = valid.iter().filter_map(|s| s.quantity_scu_milli).sum();
        let unknown = valid
            .iter()
            .filter(|s| s.quantity_scu_milli.is_none())
            .count();
        if unknown == 0 && known != total {
            issues.add(
                path,
                format!("The {kind} quantities must add up to the total SCU."),
            );
        } else if unknown > 0 && known >= total && known > 0 {
            issues.add(
                path,
                format!(
                    "Known {kind} quantities leave nothing for the locations without a quantity."
                ),
            );
        }
    }
    valid
}
