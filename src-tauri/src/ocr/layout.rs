//! Finds the contract panel in the anchor pass and derives the regions to read.
//!
//! Anchors (Task 06 / 08 measurements): the `PRIMARY OBJECTIVES` header, the `Reward` label of the
//! info card, and the `ABANDON` / `ACCEPT OFFER` button. Geometry is relative to the anchors' own
//! text height and width, so it follows resolution and UI scale. A missing anchor falls back to
//! fixed fractions of the frame (measured on 4K borderless captures).

use super::{AnchorsFound, ButtonState, OcrBox, OcrLine, RegionKind, RegionSource};

/// A region to recognise, in source-image pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRegion {
    pub kind: RegionKind,
    pub bbox: OcrBox,
    pub source: RegionSource,
}

/// Fixed fallbacks as fractions (x, y, w, h) of the frame.
const FALLBACK_OBJECTIVES: [f64; 4] = [0.63, 0.31, 0.32, 0.48];
const FALLBACK_REWARD: [f64; 4] = [0.70, 0.165, 0.25, 0.045];
const FALLBACK_TITLE: [f64; 4] = [0.34, 0.18, 0.32, 0.07];

#[derive(Debug, Clone, Default)]
pub struct Anchors<'a> {
    pub header: Option<&'a OcrLine>,
    pub reward: Option<&'a OcrLine>,
    pub button: Option<&'a OcrLine>,
}

impl Anchors<'_> {
    pub fn found(&self) -> AnchorsFound {
        AnchorsFound {
            header: self.header.is_some(),
            reward: self.reward.is_some(),
            button: self.button.is_some(),
        }
    }

    pub fn any(&self) -> bool {
        self.header.is_some() || self.reward.is_some() || self.button.is_some()
    }
}

fn words_upper(line: &OcrLine) -> Vec<String> {
    line.words
        .iter()
        .map(|w| {
            w.text
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_uppercase()
        })
        .collect()
}

fn has_pair(words: &[String], a: &str, b: &str) -> bool {
    words.windows(2).any(|p| p[0] == a && p[1].starts_with(b))
}

/// `ABANDON`, tolerating a damaged ending: the game's green highlight glow can cover "DON"
/// (a live capture read "ABANSOR>"). `ACCEPTED` (the tab) never matches.
fn is_abandon(word: &str) -> bool {
    word == "ABANDON" || (word.starts_with("ABAN") && (6..=9).contains(&word.len()))
}

fn is_button(words: &[String]) -> bool {
    words.iter().any(|w| is_abandon(w)) || has_pair(words, "ACCEPT", "OFF")
}

pub fn find_anchors(lines: &[OcrLine]) -> Anchors<'_> {
    let mut anchors = Anchors::default();
    for line in lines {
        let words = words_upper(line);
        if anchors.header.is_none() && has_pair(&words, "PRIMARY", "OBJ") {
            anchors.header = Some(line);
        }
        if anchors.reward.is_none() && words.first().is_some_and(|w| w == "REWARD") {
            anchors.reward = Some(line);
        }
        if anchors.button.is_none() && is_button(&words) {
            anchors.button = Some(line);
        }
    }
    anchors
}

/// `ABANDON` = the Accepted tab; `ACCEPT OFFER` = the Offers tab.
pub fn button_state(anchors: &Anchors<'_>) -> ButtonState {
    match anchors.button {
        Some(line) if words_upper(line).iter().any(|w| is_abandon(w)) => ButtonState::Accepted,
        Some(_) => ButtonState::Offer,
        None => ButtonState::Unknown,
    }
}

fn clamp(x: f64, y: f64, w: f64, h: f64, width: u32, height: u32) -> Option<OcrBox> {
    let (fw, fh) = (f64::from(width), f64::from(height));
    let x0 = x.clamp(0.0, fw);
    let y0 = y.clamp(0.0, fh);
    let x1 = (x + w).clamp(0.0, fw);
    let y1 = (y + h).clamp(0.0, fh);
    let b = OcrBox {
        x: x0.floor() as i32,
        y: y0.floor() as i32,
        width: (x1 - x0).floor() as i32,
        height: (y1 - y0).floor() as i32,
    };
    (b.width >= 8 && b.height >= 8).then_some(b)
}

fn fraction(f: [f64; 4], width: u32, height: u32) -> OcrBox {
    let (fw, fh) = (f64::from(width), f64::from(height));
    clamp(f[0] * fw, f[1] * fh, f[2] * fw, f[3] * fh, width, height)
        .expect("fallback fractions are inside the frame")
}

/// The title, reward and objectives regions for a frame of `width` × `height`.
pub fn plan_regions(anchors: &Anchors<'_>, width: u32, height: u32) -> Vec<PlannedRegion> {
    let fh = f64::from(height);
    let mut objectives = None;
    if let Some(header) = anchors.header {
        let hb = header.bbox;
        let line_h = f64::from(hb.height.max(1));
        let (hx, hy, hw) = (f64::from(hb.x), f64::from(hb.y), f64::from(hb.width));
        let bottom = match anchors.button {
            Some(button) if f64::from(button.bbox.y) > hy + line_h => {
                f64::from(button.bbox.y) - line_h * 0.5
            }
            _ => hy + f64::from(hb.height) + fh * 0.45,
        };
        // Start at the header itself: starting below it cut the first objective line (Task 08).
        let top = hy - line_h * 0.3;
        objectives = clamp(
            hx - line_h,
            top,
            hw * 2.25 + line_h,
            bottom - top,
            width,
            height,
        );
    }

    let (mut reward, mut title) = (None, None);
    if let Some(label) = anchors.reward {
        let rb = label.bbox;
        let line_h = f64::from(rb.height.max(1));
        let (rx, ry, rw) = (f64::from(rb.x), f64::from(rb.y), f64::from(rb.width.max(1)));
        // The value is right-aligned in the info card, ~7.7 label widths to the right.
        reward = clamp(
            rx + rw,
            ry - line_h * 0.6,
            rw * 9.5,
            line_h * 2.2,
            width,
            height,
        );
        // The title card is left of the info card; its large text starts below the label's top.
        title = clamp(
            rx - f64::from(width) * 0.34,
            ry + line_h * 0.5,
            f64::from(width) * 0.32,
            line_h * 5.0,
            width,
            height,
        );
    }

    let pick = |kind, found: Option<OcrBox>, fallback| match found {
        Some(bbox) => PlannedRegion {
            kind,
            bbox,
            source: RegionSource::Anchor,
        },
        None => PlannedRegion {
            kind,
            bbox: fraction(fallback, width, height),
            source: RegionSource::Fallback,
        },
    };
    vec![
        pick(RegionKind::Title, title, FALLBACK_TITLE),
        pick(RegionKind::Reward, reward, FALLBACK_REWARD),
        pick(RegionKind::Objectives, objectives, FALLBACK_OBJECTIVES),
    ]
}

/// Whether recognised objectives text looks like a contract (used when no anchor was found).
pub fn looks_like_objectives(lines: &[OcrLine]) -> bool {
    lines.iter().any(|l| {
        let t = l.text.to_lowercase();
        t.contains("deliver") || t.contains("collect")
    })
}
