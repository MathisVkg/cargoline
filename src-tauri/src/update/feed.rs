//! The GitHub releases list and which release to offer (pure functions).
//!
//! - Drafts are ignored.
//! - A pre-release build (`1.0.0-beta.1`) is offered every newer release, pre-release or not; a
//!   stable build only newer stable releases.
//! - Versions are compared with semver, pre-release ordering included
//!   (`1.0.0-beta.2` < `1.0.0-beta.10` < `1.0.0`). The tag must be `v<semver>`; others are skipped.
//! - The offered release needs `Cargo-Line_<version>_x64-setup.exe` and `SHA256SUMS.txt` to be
//!   installable; without them it is a notice only.

use semver::Version;
use serde::{Deserialize, Serialize};

/// Largest API response accepted.
pub const MAX_FEED_BYTES: usize = 2 * 1024 * 1024;
/// Release notes shown in the app (characters); longer notes link to GitHub.
pub const MAX_NOTES_CHARS: usize = 4_000;
pub const SUMS_ASSET: &str = "SHA256SUMS.txt";

/// The setup file name for a version (the release layout).
pub fn setup_asset_name(version: &Version) -> String {
    format!("Cargo-Line_{version}_x64-setup.exe")
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiRelease {
    #[serde(default)]
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<ApiAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiAsset {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub browser_download_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub url: String,
}

/// The release offered to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub version: String,
    pub tag: String,
    pub html_url: String,
    pub published_at: Option<String>,
    /// Plain text, capped at [`MAX_NOTES_CHARS`].
    pub notes: String,
    pub notes_truncated: bool,
    /// Both present: the release can be installed in the app.
    pub setup: Option<Asset>,
    pub sums: Option<Asset>,
}

impl Offer {
    pub fn installable(&self) -> bool {
        self.setup.is_some() && self.sums.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FeedError {
    #[error("the answer from GitHub is too large")]
    TooLarge,
    #[error("the answer from GitHub couldn't be read: {0}")]
    Invalid(String),
}

/// Parses the `/releases` list; refuses a body over [`MAX_FEED_BYTES`].
pub fn parse_releases(body: &[u8]) -> Result<Vec<ApiRelease>, FeedError> {
    if body.len() > MAX_FEED_BYTES {
        return Err(FeedError::TooLarge);
    }
    serde_json::from_slice(body).map_err(|e| FeedError::Invalid(short(&e.to_string())))
}

/// `v1.2.3` → 1.2.3; anything else (no `v`, not semver) → None.
pub fn tag_version(tag: &str) -> Option<Version> {
    tag.strip_prefix('v').and_then(|v| Version::parse(v).ok())
}

/// Whether `candidate` counts as an update for `current` (the channel rule).
pub fn is_update(current: &Version, candidate: &Version) -> bool {
    if candidate <= current {
        return false;
    }
    // A stable build only moves to stable releases; a pre-release build takes either.
    !current.pre.is_empty() || candidate.pre.is_empty()
}

/// The highest release that counts as an update, or None when up to date.
pub fn select_offer(current: &Version, releases: &[ApiRelease]) -> Option<Offer> {
    let best = releases
        .iter()
        .filter(|r| !r.draft)
        .filter_map(|r| tag_version(&r.tag_name).map(|v| (v, r)))
        .filter(|(v, _)| is_update(current, v))
        .max_by(|a, b| a.0.cmp(&b.0))?;
    let (version, release) = best;
    let find = |name: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == name && !a.browser_download_url.is_empty())
            .map(|a| Asset {
                name: a.name.clone(),
                size: a.size,
                url: a.browser_download_url.clone(),
            })
    };
    let (notes, notes_truncated) = plain_notes(release.body.as_deref().unwrap_or(""));
    Some(Offer {
        version: version.to_string(),
        tag: release.tag_name.clone(),
        html_url: release.html_url.clone(),
        published_at: release.published_at.clone(),
        notes,
        notes_truncated,
        setup: find(&setup_asset_name(&version)),
        sums: find(SUMS_ASSET),
    })
}

/// The release body as plain text: line endings normalised, control characters removed, capped.
/// It is shown as text, never rendered as Markdown or HTML.
pub fn plain_notes(body: &str) -> (String, bool) {
    let cleaned: String = body
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.chars().count() <= MAX_NOTES_CHARS {
        return (cleaned, false);
    }
    let cut: String = cleaned.chars().take(MAX_NOTES_CHARS).collect();
    (format!("{}…", cut.trim_end()), true)
}

/// Whether the top-bar marker shows: hidden only for the exact version the user skipped.
pub fn marker_visible(offer_version: &str, skipped: Option<&str>) -> bool {
    skipped != Some(offer_version)
}

/// A one-line reason, at most 120 characters.
pub fn short(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() <= 120 {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(119).collect::<String>())
    }
}
