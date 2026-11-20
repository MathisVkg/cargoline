//! Finds the contract panel in the anchor pass and derives the regions to read.
//!
//! Only the top (title, reward card) and the middle (Details, Primary Objectives) are read. The
//! bottom buttons (`ABANDON`, `ACCEPT OFFER`, `SHARE`, `TRACK`) are game controls and are never
//! used (Task 10.1): they animate right after accepting and read poorly.
//!
//! Anchors: the `PRIMARY OBJECTIVES` and `DETAILS` headers and the `Reward` label. Geometry is
//! relative to the anchors and the frame, measured on the Task 06 fixtures (4K borderless):
//! - the headers sit about 0.51 × frame height above the button row, so the panel content ends at
//!   0.46 × height below them (clear of the buttons);
//! - `DETAILS` sits about 0.29 × frame width left of `PRIMARY OBJECTIVES` (used when the
//!   `DETAILS` anchor is not read).
//!
//! A missing anchor falls back to fixed fractions of the frame.
//!
//! Screen state comes from the reward card: `Contract Deadline` = Accepted,
//! `Contract Availability` = Offers.

use super::{AnchorsFound, OcrBox, OcrLine, RegionKind, RegionSource, ScreenState};

/// A region to recognise, in source-image pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRegion {
    pub kind: RegionKind,
    pub bbox: OcrBox,
    pub source: RegionSource,
}

/// Fixed fallbacks as fractions (x, y, w, h) of the frame.
const FALLBACK_OBJECTIVES: [f64; 4] = [0.63, 0.30, 0.34, 0.46];
const FALLBACK_REWARD: [f64; 4] = [0.70, 0.165, 0.25, 0.045];
const FALLBACK_TITLE: [f64; 4] = [0.34, 0.18, 0.32, 0.07];
const FALLBACK_DETAILS: [f64; 4] = [0.34, 0.27, 0.29, 0.49];

/// Panel content below the section headers, as a fraction of the frame height.
const PANEL_BELOW_HEADERS: f64 = 0.46;
/// `DETAILS` is this fraction of the frame width left of `PRIMARY OBJECTIVES`.
const DETAILS_LEFT_OF_OBJECTIVES: f64 = 0.295;

#[derive(Debug, Clone, Default)]
pub struct Anchors<'a> {
    pub header: Option<&'a OcrLine>,
    pub details: Option<&'a OcrLine>,
    pub reward: Option<&'a OcrLine>,
    /// The reward card's second label (`Contract Deadline` / `Contract Availability`).
    pub card_label: Option<(&'a OcrLine, ScreenState)>,
}

impl Anchors<'_> {
    pub fn found(&self) -> AnchorsFound {
        AnchorsFound {
            header: self.header.is_some(),
            details: self.details.is_some(),
            reward: self.reward.is_some(),
        }
    }

    pub fn any(&self) -> bool {
        self.header.is_some() || self.details.is_some() || self.reward.is_some()
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

/// Levenshtein distance (short words only).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb))
                .min(row[j] + 1)
                .min(row[j + 1] + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

fn near(word: &str, target: &str, max: usize) -> bool {
    word.len() + max >= target.len() && edit_distance(word, target) <= max
}

/// `Contract Deadline` → Accepted, `Contract Availability` → Offers, tolerating OCR noise
/// (`Contract Deadiine`, `Contract Availabllity`, `ContractDeadline`). `ACCEPTED (0/10)` (the tab)
/// never matches.
pub fn card_label_state(line: &OcrLine) -> Option<ScreenState> {
    let words = words_upper(line);
    let label = |w: &str| {
        if near(w, "DEADLINE", 2) {
            Some(ScreenState::Accepted)
        } else if near(w, "AVAILABILITY", 3) {
            Some(ScreenState::Offer)
        } else {
            None
        }
    };
    for pair in words.windows(2) {
        if near(&pair[0], "CONTRACT", 2) {
            if let Some(state) = label(&pair[1]) {
                return Some(state);
            }
        }
    }
    // Glued: `CONTRACTDEADLINE`.
    words.iter().find_map(|w| {
        let rest = w.strip_prefix("CONTRACT")?;
        label(rest)
    })
}

pub fn find_anchors(lines: &[OcrLine]) -> Anchors<'_> {
    let mut anchors = Anchors::default();
    for line in lines {
        let words = words_upper(line);
        if anchors.header.is_none() && has_pair(&words, "PRIMARY", "OBJ") {
            anchors.header = Some(line);
        }
        if anchors.details.is_none()
            && words.len() <= 2
            && words.first().is_some_and(|w| near(w, "DETAILS", 1))
        {
            anchors.details = Some(line);
        }
        if anchors.reward.is_none() && words.first().is_some_and(|w| w == "REWARD") {
            anchors.reward = Some(line);
        }
    }
    // The card label must sit in the reward card (just below the `Reward` label) when that
    // label was read; otherwise any `Contract Deadline / Availability` line counts.
    anchors.card_label = lines.iter().find_map(|line| {
        let state = card_label_state(line)?;
        let in_card = anchors.reward.is_none_or(|r| {
            let h = r.bbox.height.max(1);
            (line.bbox.y - r.bbox.y).abs() <= h * 4 && (line.bbox.x - r.bbox.x).abs() <= h * 4
        });
        in_card.then_some((line, state))
    });
    anchors
}

/// Accepted / Offers from the reward card; unknown when neither label was read.
pub fn screen_state(anchors: &Anchors<'_>) -> ScreenState {
    anchors
        .card_label
        .map(|(_, s)| s)
        .unwrap_or(ScreenState::Unknown)
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

/// The title, reward, objectives and Details regions for a frame of `width` × `height`.
pub fn plan_regions(anchors: &Anchors<'_>, width: u32, height: u32) -> Vec<PlannedRegion> {
    let (fw, fh) = (f64::from(width), f64::from(height));
    let (mut objectives, mut details) = (None, None);
    // Panel content bottom: from whichever section header was read.
    let header_line = anchors.header.or(anchors.details);
    let bottom =
        header_line.map(|h| (f64::from(h.bbox.y) + fh * PANEL_BELOW_HEADERS).min(fh * 0.80));
    if let (Some(header), Some(bottom)) = (anchors.header, bottom) {
        let hb = header.bbox;
        let line_h = f64::from(hb.height.max(1));
        let (hx, hy, hw) = (f64::from(hb.x), f64::from(hb.y), f64::from(hb.width));
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
    if let Some(bottom) = bottom {
        // From the DETAILS header (or its usual place left of the objectives) to the objectives.
        let (dx, dy, line_h) = match (anchors.details, anchors.header) {
            (Some(d), _) => (
                f64::from(d.bbox.x),
                f64::from(d.bbox.y),
                f64::from(d.bbox.height.max(1)),
            ),
            (None, Some(h)) => (
                f64::from(h.bbox.x) - fw * DETAILS_LEFT_OF_OBJECTIVES,
                f64::from(h.bbox.y),
                f64::from(h.bbox.height.max(1)),
            ),
            (None, None) => (0.0, 0.0, 1.0),
        };
        let right = anchors
            .header
            .map(|h| f64::from(h.bbox.x) - line_h)
            .unwrap_or(dx + fw * 0.28);
        let top = dy - line_h * 0.3;
        details = clamp(
            dx - line_h,
            top,
            right - (dx - line_h),
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
            rx - fw * 0.34,
            ry + line_h * 0.5,
            fw * 0.32,
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
        pick(RegionKind::Details, details, FALLBACK_DETAILS),
    ]
}

/// Whether recognised objectives text looks like a contract (used when no anchor was found).
pub fn looks_like_objectives(lines: &[OcrLine]) -> bool {
    lines.iter().any(|l| {
        let t = l.text.to_lowercase();
        t.contains("deliver") || t.contains("collect")
    })
}
