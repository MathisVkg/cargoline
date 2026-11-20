//! Turns an [`OcrResult`] into a contract for the Add Contract form.
//!
//! Separate from the OCR engine: it only reads the recognised regions (text, words,
//! confidences), so improving the parser never touches `ocr/`.
//!
//! Ground rule: a wrong value is never filled in silently. Every field is `ok`, `check`
//! (filled, to be verified) or `missing` (left empty), with a short reason.

pub mod clean;
pub mod details;
pub mod objectives;
pub mod resolve;

use serde::Serialize;

use crate::ocr::{OcrResult, RegionKind, ScreenState};
use clean::{tokens_of, Token};
pub use resolve::{Catalog, Resolution};

/// Below this mean or word confidence a filled value is flagged `check`.
pub const LOW_CONFIDENCE: f32 = 60.0;
/// Below this the reward is left empty.
pub const REWARD_MIN_CONFIDENCE: f32 = 40.0;
/// On the Offers screen the reward needs at least this (the game cursor can hide a digit).
pub const OFFER_REWARD_MIN_CONFIDENCE: f32 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Flag {
    Ok,
    Check,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field<T> {
    pub value: Option<T>,
    pub flag: Flag,
    pub reason: Option<String>,
}

impl<T> Field<T> {
    pub fn ok(value: T) -> Self {
        Self {
            value: Some(value),
            flag: Flag::Ok,
            reason: None,
        }
    }
    pub fn check(value: T, reason: &str) -> Self {
        Self {
            value: Some(value),
            flag: Flag::Check,
            reason: Some(reason.into()),
        }
    }
    pub fn missing(reason: &str) -> Self {
        Self {
            value: None,
            flag: Flag::Missing,
            reason: Some(reason.into()),
        }
    }
    /// No value, and that is expected (e.g. the split between several pickups).
    pub fn unknown() -> Self {
        Self {
            value: None,
            flag: Flag::Ok,
            reason: None,
        }
    }
}

/// A location or commodity name and how it was resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NameField {
    pub value: Option<String>,
    pub flag: Flag,
    pub reason: Option<String>,
    pub resolution: Resolution,
    /// The cleaned OCR text it came from.
    pub read: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedStop {
    pub location: NameField,
    /// Whole SCU; `value: None` with `flag: ok` means "unknown" (several pickups).
    pub scu: Field<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedRequirement {
    pub commodity: NameField,
    pub total_scu: Field<u32>,
    pub pickups: Vec<ParsedStop>,
    pub deliveries: Vec<ParsedStop>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedContract {
    pub screen: ScreenState,
    pub title: Field<String>,
    pub reward: Field<u64>,
    pub requirements: Vec<ParsedRequirement>,
    /// Things that could not be read reliably; reported, never guessed.
    pub problems: Vec<String>,
    /// Max container size from the Details text, the same for every requirement. Empty without a
    /// flag when the text states none; empty and flagged when it states several.
    pub max_box_scu: Field<u32>,
}

/// Names to resolve against: the bundled lists plus the user's own.
#[derive(Debug, Default, Clone)]
pub struct References {
    pub locations: Catalog,
    pub commodities: Catalog,
}

pub const REASON_LOW_CONFIDENCE: &str = "Check — read with low confidence";
pub const REASON_NOT_IN_LIST: &str = "Not in list";
pub const REASON_NO_REWARD: &str = "Couldn't read reward";

pub fn parse_contract(ocr: &OcrResult, refs: &References) -> ParsedContract {
    let region_tokens = |kind: RegionKind| -> Vec<Vec<Token>> {
        ocr.regions
            .iter()
            .filter(|r| r.kind == kind)
            .flat_map(|r| r.lines.iter().map(tokens_of))
            .collect()
    };
    let objectives_lines: Vec<&crate::ocr::OcrLine> = ocr
        .regions
        .iter()
        .filter(|r| r.kind == RegionKind::Objectives)
        .flat_map(|r| r.lines.iter())
        .collect();
    let mut problems = Vec::new();
    let requirements = objectives::parse_objectives(&objectives_lines, refs, &mut problems);
    ParsedContract {
        screen: ocr.screen,
        title: parse_title(&region_tokens(RegionKind::Title)),
        reward: parse_reward(&region_tokens(RegionKind::Reward), ocr.screen),
        requirements,
        problems,
        max_box_scu: details::parse_max_box(
            &ocr.regions
                .iter()
                .filter(|r| r.kind == RegionKind::Details)
                .flat_map(|r| r.lines.iter().map(|l| l.text.as_str()))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    }
}

/// Title: the region's lines joined; lines with fewer than 3 letters or digits are stray marks.
pub fn parse_title(lines: &[Vec<Token>]) -> Field<String> {
    let words: Vec<&Token> = lines
        .iter()
        .filter(|line| {
            line.iter()
                .map(|t| t.text.chars().filter(|c| c.is_alphanumeric()).count())
                .sum::<usize>()
                >= 3
        })
        .flatten()
        .collect();
    let text = clean::fold_apostrophes(
        &words
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    )
    .trim()
    .to_string();
    if text.is_empty() {
        return Field::missing("Couldn't read title");
    }
    let known: Vec<f32> = words.iter().map(|t| t.conf).filter(|c| *c >= 0.0).collect();
    let mean = if known.is_empty() {
        0.0
    } else {
        known.iter().sum::<f32>() / known.len() as f32
    };
    if mean < LOW_CONFIDENCE {
        Field::check(text, REASON_LOW_CONFIDENCE)
    } else {
        Field::ok(text)
    }
}

/// `1,234,567` / `64,250` / `500` (1–6 plain digits). Leading and trailing symbols are ignored;
/// tokens with letters (`om`, `©H476,500`) never match.
fn reward_number(text: &str) -> Option<u64> {
    let t = text.trim_matches(|c: char| !c.is_alphanumeric() && c != ',');
    let t = t.trim_matches(',');
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == ',') {
        return None;
    }
    let groups: Vec<&str> = t.split(',').collect();
    let valid = if groups.len() == 1 {
        (1..=6).contains(&t.len())
    } else {
        (1..=3).contains(&groups[0].len()) && groups[1..].iter().all(|g| g.len() == 3)
    };
    valid.then(|| groups.concat().parse().ok()).flatten()
}

/// Rules (Task 09, tightened by the fixture regression):
/// - the last token reading as a reward number; currency-glyph junk is ignored;
/// - its confidence is the lower of the word's and the weakest meaningful word on its line, so damage next to the
///   number (the game cursor over a digit, read as `lipg`) counts. The currency glyph's own
///   readings (symbols, or 1–2 character fragments such as `m`, `om`, `&`) are left out of the
///   line mean: they are always low and say nothing about the digits;
/// - empty and flagged when no token matches, below 40, or below 60 on the Offers screen;
/// - filled but flagged below 60, or when it has no thousands separator (in-game rewards are
///   written `59,500`, so a bare `250` is likely a fragment).
pub fn parse_reward(lines: &[Vec<Token>], screen: ScreenState) -> Field<u64> {
    let found = lines
        .iter()
        .flat_map(|line| {
            // The weakest meaningful word on the line (100 when there is none besides glyphs).
            let line_conf = line
                .iter()
                .filter(|t| !clean::is_symbol_junk(&t.text) && t.text.chars().count() > 2)
                .map(|t| t.conf)
                .filter(|c| *c >= 0.0)
                .fold(100.0_f32, f32::min);
            line.iter().filter_map(move |t| {
                reward_number(&t.text).map(|v| (v, t.conf.min(line_conf), t.text.contains(',')))
            })
        })
        .last();
    let Some((value, conf, grouped)) = found else {
        return Field::missing(REASON_NO_REWARD);
    };
    if conf < REWARD_MIN_CONFIDENCE
        || (screen == ScreenState::Offer && conf < OFFER_REWARD_MIN_CONFIDENCE)
    {
        return Field::missing(REASON_NO_REWARD);
    }
    if conf < LOW_CONFIDENCE || !grouped {
        Field::check(value, REASON_LOW_CONFIDENCE)
    } else {
        Field::ok(value)
    }
}

impl References {
    /// The bundled reference lists first (their spelling wins), then the user's own names.
    pub fn build(user_locations: &[String], used_commodities: &[String]) -> Self {
        let bundled_locations = crate::reference::locations()
            .map(|l| l.items.iter().map(|i| i.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        let bundled_commodities = crate::reference::commodities()
            .map(|l| l.items.iter().map(|i| i.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        Self {
            locations: Catalog::new(bundled_locations.iter().chain(user_locations)),
            commodities: Catalog::new(bundled_commodities.iter().chain(used_commodities)),
        }
    }
}
