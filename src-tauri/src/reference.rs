//! Bundled reference lists for the Add Contract suggestions: locations and
//! commodities seen in Star Citizen hauling missions (Star Citizen Wiki API,
//! one pinned game build). Static data compiled into the app; never fetched
//! at runtime and never written to the database.

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub const LOCATIONS_JSON: &str = include_str!("../resources/catalog/locations.json");
pub const COMMODITIES_JSON: &str = include_str!("../resources/catalog/commodities.json");

/// A bundled list file: provenance header plus sorted items.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceList<T> {
    pub source: String,
    pub attribution: String,
    pub game_version: String,
    pub generated_at: String,
    pub items: Vec<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceLocation {
    pub name: String,
    pub system: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceCommodity {
    pub name: String,
    pub group: Option<String>,
}

/// Anything carrying a display name, for the shared checks.
pub trait Named {
    fn name(&self) -> &str;
}
impl Named for ReferenceLocation {
    fn name(&self) -> &str {
        &self.name
    }
}
impl Named for ReferenceCommodity {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Parses and checks a list: non-empty trimmed names, unique ignoring case
/// and whitespace, sorted case-insensitively, no unresolved mission tokens.
pub fn parse_list<T>(json: &str) -> Result<ReferenceList<T>, String>
where
    T: Named + for<'de> Deserialize<'de>,
{
    let list: ReferenceList<T> =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    let mut seen = HashSet::new();
    let mut previous: Option<String> = None;
    for item in &list.items {
        let name = item.name();
        if name.trim().is_empty() || name.trim() != name {
            return Err(format!("empty or untrimmed name {name:?}"));
        }
        if name.contains("~mission(")
            || name.contains("<EM4>")
            || name.contains('<')
            || name.contains('>')
        {
            return Err(format!("unresolved mission token in {name:?}"));
        }
        let key = name
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if !seen.insert(key) {
            return Err(format!("duplicate name {name:?}"));
        }
        let lower = name.to_lowercase();
        if previous.as_ref().is_some_and(|p| *p > lower) {
            return Err(format!("not sorted at {name:?}"));
        }
        previous = Some(lower);
    }
    Ok(list)
}

static LOCATIONS: OnceLock<Result<ReferenceList<ReferenceLocation>, String>> = OnceLock::new();
static COMMODITIES: OnceLock<Result<ReferenceList<ReferenceCommodity>, String>> = OnceLock::new();

/// The bundled location list, parsed once. An invalid file is reported as an
/// error (and fails the tests), never a crash.
pub fn locations() -> Result<&'static ReferenceList<ReferenceLocation>, String> {
    LOCATIONS
        .get_or_init(|| parse_list(LOCATIONS_JSON))
        .as_ref()
        .map_err(|e| format!("bundled locations list: {e}"))
}

/// The bundled commodity list, parsed once.
pub fn commodities() -> Result<&'static ReferenceList<ReferenceCommodity>, String> {
    COMMODITIES
        .get_or_init(|| parse_list(COMMODITIES_JSON))
        .as_ref()
        .map_err(|e| format!("bundled commodities list: {e}"))
}
