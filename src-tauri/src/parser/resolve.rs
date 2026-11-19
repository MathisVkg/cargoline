//! Resolves OCR'd names against a catalog (bundled list plus the user's own names).
//!
//! Order: exact (ignoring case and whitespace) → for locations, without a trailing
//! `above <X>` / `on <X>` → for locations, the Lagrange long form
//! (`<Name> at <Body>'s L<n> Lagrange point` → `<CODE>-L<n> <Name>`) → a confusable fold, only
//! when it gives exactly one name.
//! Never the "nearest" name: anything else stays unresolved, with the cleaned OCR text.

use std::collections::HashMap;

use serde::Serialize;

use super::clean::fold_apostrophes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Resolution {
    Exact,
    Stripped,
    /// `<Name> at <Body>'s L<n> Lagrange point` matched as `<CODE>-L<n> <Name>`.
    Lagrange,
    Folded,
    Unresolved,
}

#[derive(Debug, Default, Clone)]
pub struct Catalog {
    exact: HashMap<String, String>,
    /// Folded key → canonical name, or None when two different names fold together.
    folded: HashMap<String, Option<String>>,
}

fn exact_key(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `I l 1 | !` → 1, `S 5` → 5, `O 0` → 0, apostrophe variants → `'`; case and spaces ignored.
pub fn fold_key(text: &str) -> String {
    fold_apostrophes(&exact_key(text))
        .chars()
        .map(|c| match c {
            'i' | 'l' | '1' | '|' | '!' => '1',
            's' | '5' => '5',
            'o' | '0' => '0',
            c => c,
        })
        .collect()
}

/// Lagrange station codes, as used by the bundled `locations.json` (checked: ARC-L1…L5,
/// CRU-L1/L4/L5, HUR-L1…L5, MIC-L1…L5).
pub const LAGRANGE_CODES: [(&str, &str); 4] = [
    ("arccorp", "ARC"),
    ("crusader", "CRU"),
    ("hurston", "HUR"),
    ("microtech", "MIC"),
];

/// `Long Forest Station at microTech's L2 Lagrange point` → `MIC-L2 Long Forest Station`.
/// None when the text is not in that form or the body has no known code.
pub fn lagrange_candidate(text: &str) -> Option<String> {
    let text = fold_apostrophes(text);
    let words: Vec<&str> = text.split_whitespace().collect();
    let at = words.iter().rposition(|w| w.eq_ignore_ascii_case("at"))?;
    let name = &words[..at];
    let rest = &words[at + 1..];
    if name.is_empty()
        || rest.len() < 3
        || !rest[2]
            .trim_end_matches('.')
            .eq_ignore_ascii_case("lagrange")
    {
        return None;
    }
    if rest.len() > 3 && !rest[3].trim_end_matches('.').eq_ignore_ascii_case("point") {
        return None;
    }
    let body = rest[0].to_lowercase();
    let body = body.strip_suffix("'s").unwrap_or(&body);
    let code = LAGRANGE_CODES
        .iter()
        .find(|(b, _)| *b == body || fold_key(b) == fold_key(body))
        .map(|(_, code)| *code)?;
    // `L2`, and OCR's `LI` / `Ll` for L1.
    let point = rest[1].trim_end_matches(['.', ',']);
    let digit = point.strip_prefix(['L', 'l'])?;
    let n = match digit {
        "I" | "l" | "|" | "i" => 1,
        d => d.parse::<u8>().ok().filter(|n| (1..=5).contains(n))?,
    };
    Some(format!("{code}-L{n} {}", name.join(" ")))
}

/// Removes a trailing `above <X>` / `on <X>` (case-insensitive), keeping the place itself.
pub fn strip_body(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words
        .iter()
        .rposition(|w| w.eq_ignore_ascii_case("above") || w.eq_ignore_ascii_case("on"))
    {
        Some(i) if i > 0 && i + 1 < words.len() => words[..i].join(" "),
        _ => words.join(" "),
    }
}

impl Catalog {
    /// Earlier names win on exact duplicates (pass the bundled list first).
    pub fn new<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut catalog = Catalog::default();
        for name in names {
            let name = name.as_ref().trim();
            if name.is_empty() {
                continue;
            }
            let key = exact_key(name);
            if catalog.exact.contains_key(&key) {
                continue;
            }
            catalog.exact.insert(key, name.to_string());
            let folded = fold_key(name);
            match catalog.folded.get(&folded) {
                Some(Some(existing)) if exact_key(existing) != exact_key(name) => {
                    catalog.folded.insert(folded, None); // ambiguous
                }
                Some(_) => {}
                None => {
                    catalog.folded.insert(folded, Some(name.to_string()));
                }
            }
        }
        catalog
    }

    /// Resolves `text`. Returns the canonical name or the cleaned text, and how.
    pub fn resolve(&self, text: &str, strip_location_body: bool) -> (String, Resolution) {
        let cleaned = fold_apostrophes(&text.split_whitespace().collect::<Vec<_>>().join(" "));
        if let Some(name) = self.exact.get(&exact_key(&cleaned)) {
            return (name.clone(), Resolution::Exact);
        }
        let base = if strip_location_body {
            strip_body(&cleaned)
        } else {
            cleaned.clone()
        };
        if strip_location_body && base != cleaned {
            if let Some(name) = self.exact.get(&exact_key(&base)) {
                return (name.clone(), Resolution::Stripped);
            }
        }
        if strip_location_body {
            if let Some(candidate) = lagrange_candidate(&cleaned) {
                if let Some(name) = self.exact.get(&exact_key(&candidate)) {
                    return (name.clone(), Resolution::Lagrange);
                }
                if let Some(Some(name)) = self.folded.get(&fold_key(&candidate)) {
                    return (name.clone(), Resolution::Lagrange);
                }
            }
        }
        if let Some(Some(name)) = self.folded.get(&fold_key(&base)) {
            return (name.clone(), Resolution::Folded);
        }
        (base, Resolution::Unresolved)
    }
}
