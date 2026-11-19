//! Parses Tesseract's TSV output into lines of words with boxes and confidences.
//!
//! Columns: `level page_num block_num par_num line_num word_num left top width height conf text`.
//! Only word rows (level 5) with non-blank text are kept; words are grouped by
//! (page, block, paragraph, line) in output order.

use super::{OcrBox, OcrLine, OcrWord};

/// Maps a box from the recognised image back to the source image: `source = offset + box / scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mapping {
    pub offset_x: f64,
    pub offset_y: f64,
    pub scale: f64,
}

impl Mapping {
    pub const IDENTITY: Mapping = Mapping {
        offset_x: 0.0,
        offset_y: 0.0,
        scale: 1.0,
    };

    fn apply(&self, left: f64, top: f64, width: f64, height: f64) -> OcrBox {
        OcrBox {
            x: (self.offset_x + left / self.scale).round() as i32,
            y: (self.offset_y + top / self.scale).round() as i32,
            width: (width / self.scale).round() as i32,
            height: (height / self.scale).round() as i32,
        }
    }
}

/// (page, block, paragraph, line) of a TSV row.
type LineKey = (u32, u32, u32, u32);

pub fn parse_tsv(tsv: &str, mapping: Mapping) -> Vec<OcrLine> {
    let mut lines: Vec<(LineKey, Vec<OcrWord>)> = Vec::new();
    for row in tsv.lines() {
        let cols: Vec<&str> = row.trim_end_matches('\r').split('\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue; // header, non-word levels, malformed rows
        }
        let text = cols[11..].join("\t").trim().to_string();
        if text.is_empty() {
            continue;
        }
        let num = |i: usize| cols[i].trim().parse::<f64>().ok();
        let id = |i: usize| cols[i].trim().parse::<u32>().unwrap_or(0);
        let (Some(left), Some(top), Some(width), Some(height)) = (num(6), num(7), num(8), num(9))
        else {
            continue;
        };
        let word = OcrWord {
            text,
            confidence: num(10).unwrap_or(-1.0).clamp(-1.0, 100.0) as f32,
            bbox: mapping.apply(left, top, width, height),
        };
        let key = (id(1), id(2), id(3), id(4));
        match lines.last_mut() {
            Some((k, words)) if *k == key => words.push(word),
            _ => lines.push((key, vec![word])),
        }
    }
    lines
        .into_iter()
        .map(|(_, words)| OcrLine::from_words(words))
        .collect()
}
