//! Text recognition of a stored capture with the bundled Tesseract engine.
//!
//! Recognition only: this module returns regions, lines, words and confidences. Turning text
//! into contract fields is a separate concern (Task 09), so a parser change never touches the
//! engine.
//!
//! Pipeline (configuration chosen by the Task 08 accuracy gate, `tessdata_fast`):
//! 1. Load the PNG.
//! 2. Anchor pass on a 50 % copy, `--psm 11` (sparse text): `PRIMARY OBJECTIVES`, `Reward`,
//!    `ABANDON` / `ACCEPT OFFER`.
//! 3. Regions from the anchors (fixed fractions when an anchor is missing).
//! 4. Title and reward raw, objectives grayscale ×2, all `--psm 6`, in parallel.

pub mod engine;
pub mod image;
pub mod layout;
pub mod service;
pub mod tsv;

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use engine::{CancelToken, Engine};
use image::Image;
use layout::{find_anchors, looks_like_objectives, plan_regions, screen_state, PlannedRegion};
use tsv::{parse_tsv, Mapping};

/// Page segmentation modes (see `tesseract --help-psm`).
const PSM_SPARSE: u8 = 11;
const PSM_BLOCK: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OcrBox {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl OcrBox {
    fn union(a: OcrBox, b: OcrBox) -> OcrBox {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        let r = (a.x + a.width).max(b.x + b.width);
        let bottom = (a.y + a.height).max(b.y + b.height);
        OcrBox {
            x,
            y,
            width: r - x,
            height: bottom - y,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcrWord {
    pub text: String,
    /// 0–100 as reported by Tesseract (-1 when unknown).
    pub confidence: f32,
    #[serde(rename = "box")]
    pub bbox: OcrBox,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcrLine {
    pub text: String,
    /// Mean word confidence.
    pub confidence: f32,
    #[serde(rename = "box")]
    pub bbox: OcrBox,
    pub words: Vec<OcrWord>,
}

impl OcrLine {
    pub fn from_words(words: Vec<OcrWord>) -> Self {
        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let known: Vec<f32> = words
            .iter()
            .map(|w| w.confidence)
            .filter(|c| *c >= 0.0)
            .collect();
        let confidence = if known.is_empty() {
            -1.0
        } else {
            known.iter().sum::<f32>() / known.len() as f32
        };
        let bbox = words
            .iter()
            .map(|w| w.bbox)
            .reduce(OcrBox::union)
            .unwrap_or(OcrBox {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            });
        Self {
            text,
            confidence: (confidence * 10.0).round() / 10.0,
            bbox,
            words,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegionKind {
    Title,
    Reward,
    Objectives,
    /// The Details text (only the max container size is read from it).
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegionSource {
    Anchor,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrRegion {
    pub kind: RegionKind,
    #[serde(rename = "box")]
    pub bbox: OcrBox,
    pub source: RegionSource,
    pub lines: Vec<OcrLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScreenState {
    /// The reward card reads `Contract Deadline`: the Accepted tab.
    Accepted,
    /// The reward card reads `Contract Availability`: the Offers tab (amounts may still change).
    Offer,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct AnchorsFound {
    pub header: bool,
    pub details: bool,
    pub reward: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrTimings {
    pub load_ms: u64,
    pub anchor_ms: u64,
    pub regions_ms: u64,
    pub total_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrResult {
    pub width: u32,
    pub height: u32,
    pub anchors: AnchorsFound,
    /// From the reward card (never from the bottom buttons).
    pub screen: ScreenState,
    pub regions: Vec<OcrRegion>,
    pub timings: OcrTimings,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OcrError {
    #[error("The OCR engine is not installed with this copy of Cargo Line ({0})")]
    EngineMissing(String),
    #[error("Reading the text took too long")]
    Timeout,
    #[error("No contract panel was found in the screenshot")]
    NoPanel,
    #[error("Text recognition was cancelled")]
    Cancelled,
    #[error("Text recognition failed: {0}")]
    Failed(String),
}

fn ms(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

/// Recognises a stored capture. Blocking; call from a worker thread.
pub fn recognize_file(
    engine: &Engine,
    png: &Path,
    cancel: &CancelToken,
) -> Result<OcrResult, OcrError> {
    let started = Instant::now();
    let file = std::fs::File::open(png)
        .map_err(|e| OcrError::Failed(format!("could not open the screenshot: {e}")))?;
    let frame = Image::decode_png(std::io::BufReader::new(file))
        .map_err(|e| OcrError::Failed(e.to_string()))?;
    let load_ms = ms(started);
    recognize_image(engine, &frame, cancel, load_ms, started)
}

pub fn recognize_image(
    engine: &Engine,
    frame: &Image,
    cancel: &CancelToken,
    load_ms: u64,
    started: Instant,
) -> Result<OcrResult, OcrError> {
    // 2. Anchor pass on a half-size copy (boxes mapped back to full size).
    let anchor_started = Instant::now();
    let small = frame.half();
    let anchor_tsv = engine.recognize_tsv(&small, PSM_SPARSE, cancel)?;
    let anchor_lines = parse_tsv(
        &anchor_tsv,
        Mapping {
            offset_x: 0.0,
            offset_y: 0.0,
            scale: 0.5,
        },
    );
    let anchors = find_anchors(&anchor_lines);
    let anchor_ms = ms(anchor_started);

    // 3–4. Regions, recognised in parallel.
    let regions_started = Instant::now();
    let planned = plan_regions(&anchors, frame.width, frame.height);
    let results: Vec<Result<OcrRegion, OcrError>> = std::thread::scope(|scope| {
        let handles: Vec<_> = planned
            .iter()
            .map(|region| scope.spawn(move || read_region(engine, frame, region, cancel)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    Err(OcrError::Failed("a recognition thread panicked".into()))
                })
            })
            .collect()
    });
    let mut regions = Vec::with_capacity(results.len());
    for result in results {
        regions.push(result?);
    }
    let regions_ms = ms(regions_started);

    if !anchors.any() {
        let objectives = regions.iter().find(|r| r.kind == RegionKind::Objectives);
        if !objectives.is_some_and(|r| looks_like_objectives(&r.lines)) {
            return Err(OcrError::NoPanel);
        }
    }
    Ok(OcrResult {
        width: frame.width,
        height: frame.height,
        anchors: anchors.found(),
        screen: screen_state(&anchors),
        regions,
        timings: OcrTimings {
            load_ms,
            anchor_ms,
            regions_ms,
            total_ms: ms(started),
        },
    })
}

fn read_region(
    engine: &Engine,
    frame: &Image,
    region: &PlannedRegion,
    cancel: &CancelToken,
) -> Result<OcrRegion, OcrError> {
    let crop = frame.crop(region.bbox);
    // Objectives: grayscale and 2× (56/56 fields in the gate); title and reward read best raw.
    let (image, scale) = match region.kind {
        RegionKind::Objectives => (crop.gray().double(), 2.0),
        RegionKind::Title | RegionKind::Reward => (crop, 1.0),
        // Details: grayscale is enough for "<n> SCU"; no upscale keeps the time budget.
        RegionKind::Details => (crop.gray(), 1.0),
    };
    let tsv = engine.recognize_tsv(&image, PSM_BLOCK, cancel)?;
    let mapping = Mapping {
        offset_x: f64::from(region.bbox.x),
        offset_y: f64::from(region.bbox.y),
        scale,
    };
    Ok(OcrRegion {
        kind: region.kind,
        bbox: region.bbox,
        source: region.source,
        lines: parse_tsv(&tsv, mapping),
    })
}
