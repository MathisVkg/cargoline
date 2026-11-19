//! Token-level cleaning of OCR text (junk tolerances measured in Tasks 06 and 08).

use crate::ocr::OcrLine;

/// One recognised word with its confidence (0–100, -1 unknown).
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub text: String,
    pub conf: f32,
}

impl Token {
    pub fn new(text: impl Into<String>, conf: f32) -> Self {
        Self {
            text: text.into(),
            conf,
        }
    }
}

/// Characters the ◇ bullets and the panel edge come out as.
const JUNK_SYMBOLS: &[char] = &[
    '◇', '◆', '•', '¤', '?', '©', '®', '~', '«', '»', '*', '@', '|', '<', '>', '-', '_', '—', '–',
    '\'', '"', '‘', '’', '“', '”', '(', ')', '[', ']', '{', '}', '.', ',', ':', ';', '=', '+', '^',
    '`', '´', '/', '\\',
];

pub fn tokens_of(line: &OcrLine) -> Vec<Token> {
    if line.words.is_empty() {
        return line
            .text
            .split_whitespace()
            .map(|w| Token::new(w, line.confidence))
            .collect();
    }
    line.words
        .iter()
        .flat_map(|w| {
            w.text
                .split_whitespace()
                .map(move |t| Token::new(t, w.confidence))
        })
        .collect()
}

/// A token made only of junk symbols (`|`, `~`, `<~`, `©`, …), anywhere in a line.
pub fn is_symbol_junk(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| JUNK_SYMBOLS.contains(&c))
}

/// A bullet at the start of a line: symbol junk, a lone `o` / `O` / `0`, or up to 3 characters
/// mixing letters and symbols (`co)`, `0)`).
pub fn is_leading_junk(text: &str) -> bool {
    if is_symbol_junk(text) || matches!(text, "o" | "O" | "0" | "e" | "©)") {
        return true;
    }
    // A wrapped line can start with a real short word such as "IV." or "61.": only symbols other
    // than a full stop or comma make it junk.
    let n = text.chars().count();
    n <= 3
        && text
            .chars()
            .any(|c| JUNK_SYMBOLS.contains(&c) && c != '.' && c != ',')
        && !text.chars().any(|c| c.is_ascii_digit())
}

/// The clause verb, tolerating `l`/`i`/`I`/`1`/`|` confusions (`Dellver`, `Coliect`, `CoIlect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Deliver,
    Collect,
}

pub fn verb(text: &str) -> Option<Verb> {
    let folded: String = text
        .trim_matches(|c: char| !c.is_alphanumeric())
        .chars()
        .map(|c| match c {
            'i' | 'I' | '1' | '|' | '!' | 'L' => 'l',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    match folded.as_str() {
        "dellver" => Some(Verb::Deliver),
        "collect" | "co11ect" => Some(Verb::Collect),
        _ => None,
    }
}

/// Fixes digit look-alikes inside a numeric token only (`O/o/Q/D→0`, `l/I/i/|/!→1`, `S/s→5`,
/// `B→8`, `Z/z→2`). The token must already contain a real digit; words are never converted.
/// Returns the digits and whether any character was converted.
pub fn fix_digits(text: &str) -> Option<(u32, bool)> {
    if !text.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut fixed = false;
    let mut digits = String::new();
    for c in text.chars() {
        let d = match c {
            '0'..='9' => c,
            'O' | 'o' | 'Q' | 'D' => '0',
            'l' | 'I' | 'i' | '|' | '!' => '1',
            'S' | 's' => '5',
            'B' => '8',
            'Z' | 'z' => '2',
            ',' => continue, // thousands separator
            _ => return None,
        };
        fixed |= c != d;
        digits.push(d);
    }
    if digits.is_empty() || digits.len() > 6 {
        return None;
    }
    digits.parse().ok().map(|n| (n, fixed))
}

/// Folds `’ ' ´ \` •` (and runs of them) to one `'`.
pub fn fold_apostrophes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_apostrophe = false;
    for c in text.chars() {
        if matches!(c, '’' | '\'' | '´' | '`' | '•' | '‘') {
            if !last_apostrophe {
                out.push('\'');
            }
            last_apostrophe = true;
        } else {
            out.push(c);
            last_apostrophe = false;
        }
    }
    out
}

/// Trims the end of a clause: up to 3 stray characters after its final full stop, then the stop
/// itself and trailing punctuation.
pub fn trim_clause_end(tokens: &mut Vec<Token>) {
    if let Some(stop) = tokens.iter().rposition(|t| t.text.ends_with('.')) {
        let after: usize = tokens[stop + 1..]
            .iter()
            .map(|t| t.text.chars().count())
            .sum();
        if after <= 3 {
            tokens.truncate(stop + 1);
        }
    }
    while let Some(last) = tokens.last_mut() {
        let trimmed = last.text.trim_end_matches(['.', ',', ';', ':']).to_string();
        if trimmed.is_empty() || is_symbol_junk(&trimmed) {
            tokens.pop();
        } else {
            last.text = trimmed;
            break;
        }
    }
}

pub fn join(tokens: &[Token]) -> String {
    fold_apostrophes(
        &tokens
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

pub fn min_conf(tokens: &[Token]) -> f32 {
    tokens
        .iter()
        .map(|t| t.conf)
        .filter(|c| *c >= 0.0)
        .fold(f32::INFINITY, f32::min)
        .min(100.0)
}
