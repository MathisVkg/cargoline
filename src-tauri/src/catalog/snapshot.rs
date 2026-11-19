//! The bundled reference snapshot, embedded at compile time.
//!
//! Produced by `tools/catalog-import/import.mjs` from the Star Citizen Wiki
//! API. The app never fetches catalog data at runtime.

use std::collections::HashSet;

use serde::Deserialize;

use super::normalize_key;

/// The snapshot file shipped with this build.
pub const EMBEDDED_SNAPSHOT: &str = include_str!("../../resources/catalog/reference-catalog.json");

/// Snapshot format version this build understands.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    /// Changes whenever the data changes; drives the startup sync.
    pub catalog_version: String,
    pub source: SnapshotSource,
    pub game_version: String,
    /// UTC date `YYYY-MM-DD`.
    pub fetched_at: String,
    pub attribution: String,
    #[serde(default)]
    pub license: Option<String>,
    pub commodities: Vec<SnapshotCommodity>,
    pub locations: Vec<SnapshotLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SnapshotSource {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub docs: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SnapshotCommodity {
    pub source_id: String,
    pub name: String,
    pub key: String,
    pub kind: Option<String>,
    pub seen_in_hauling: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SnapshotLocation {
    pub source_id: String,
    pub name: String,
    pub key: String,
    pub kind: String,
    pub body: Option<String>,
    pub system: Option<String>,
    pub seen_in_hauling: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("the reference catalog snapshot could not be parsed: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("the reference catalog snapshot is invalid: {0}")]
    Invalid(String),
}

const LOCATION_KINDS: &[&str] = &["station", "city", "outpost", "distribution_centre", "other"];

impl Snapshot {
    pub fn parse(json: &str) -> Result<Self, SnapshotError> {
        let snapshot: Self = serde_json::from_str(json)?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// The snapshot compiled into this build.
    pub fn embedded() -> Result<Self, SnapshotError> {
        Self::parse(EMBEDDED_SNAPSHOT)
    }

    /// Structural checks: supported schema, non-empty names, keys that match
    /// the names, unique keys and source ids per table, known location kinds.
    pub fn validate(&self) -> Result<(), SnapshotError> {
        let invalid = |m: String| Err(SnapshotError::Invalid(m));
        if self.schema_version != SUPPORTED_SCHEMA_VERSION {
            return invalid(format!(
                "unsupported schema version {}",
                self.schema_version
            ));
        }
        if self.catalog_version.trim().is_empty() {
            return invalid("empty catalog_version".into());
        }
        let check = |table: &str,
                     rows: &mut dyn Iterator<Item = (&str, &str, &str)>|
         -> Result<(), SnapshotError> {
            let (mut keys, mut ids) = (HashSet::new(), HashSet::new());
            for (id, name, key) in rows {
                if name.trim().is_empty() || id.trim().is_empty() {
                    return Err(SnapshotError::Invalid(format!(
                        "{table}: empty name or source id"
                    )));
                }
                if normalize_key(name) != key {
                    return Err(SnapshotError::Invalid(format!(
                        "{table}: key {key:?} does not match {name:?}"
                    )));
                }
                if !keys.insert(key) || !ids.insert(id) {
                    return Err(SnapshotError::Invalid(format!(
                        "{table}: duplicate key or id for {name:?}"
                    )));
                }
            }
            Ok(())
        };
        check(
            "commodities",
            &mut self
                .commodities
                .iter()
                .map(|c| (c.source_id.as_str(), c.name.as_str(), c.key.as_str())),
        )?;
        check(
            "locations",
            &mut self
                .locations
                .iter()
                .map(|l| (l.source_id.as_str(), l.name.as_str(), l.key.as_str())),
        )?;
        if let Some(bad) = self
            .locations
            .iter()
            .find(|l| !LOCATION_KINDS.contains(&l.kind.as_str()))
        {
            return invalid(format!("locations: unknown kind {:?}", bad.kind));
        }
        Ok(())
    }
}
