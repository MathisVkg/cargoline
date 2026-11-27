//! The max container size from the Details text. Nothing else is read from it.
//!
//! Seen in mission texts:
//! - "At most the containers will be 8 SCU in size"
//! - "(max container size of 4 SCU)"
//! - "into containers 8 SCU or smaller"
//! - "(4 SCU or smaller)"
//!
//! Rule: an `<n> SCU` in a sentence that mentions `container` or `or smaller`, and that is
//! - followed by `or smaller` / `in size`, or
//! - preceded (within 5 words) by `container(s)` or `size`,
//!
//! but **not** one describing a container type: `<n> SCU cargo containers` /
//! `<n> SCU containers` (e.g. "a ship that can fit 2 SCU cargo containers").

use std::collections::BTreeSet;

use super::{Field, Flag};

pub const REASON_SEVERAL_SIZES: &str = "Check — several container sizes in the text";

fn words(sentence: &str) -> Vec<String> {
    sentence
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// `8` + `scu`, or a glued `8scu`; returns the number and the index after it.
fn scu_amount(words: &[String], i: usize) -> Option<(u32, usize)> {
    let w = &words[i];
    if let Some(n) = w
        .strip_suffix("scu")
        .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    {
        return Some((n.parse().ok()?, i + 1));
    }
    if w.chars().all(|c| c.is_ascii_digit()) && words.get(i + 1).is_some_and(|n| n == "scu") {
        return Some((w.parse().ok()?, i + 2));
    }
    None
}

/// All container-size values stated in `text` (distinct, sorted).
pub fn container_sizes(text: &str) -> BTreeSet<u32> {
    let mut sizes = BTreeSet::new();
    for sentence in text.split(['.', '(', ')', ';', '!', '?', '\n']) {
        let low = sentence.to_lowercase();
        if !(low.contains("container") || low.contains("or smaller")) {
            continue;
        }
        let w = words(sentence);
        for i in 0..w.len() {
            let Some((n, after)) = scu_amount(&w, i) else {
                continue;
            };
            if n == 0 {
                continue;
            }
            let next = |k: usize| w.get(after + k).map(String::as_str).unwrap_or("");
            // "<n> SCU cargo containers" / "<n> SCU containers": a container type, not a limit.
            if next(0) == "cargo" || next(0).starts_with("container") {
                continue;
            }
            let follows =
                (next(0) == "or" && next(1) == "smaller") || (next(0) == "in" && next(1) == "size");
            let precedes = w[i.saturating_sub(5)..i]
                .iter()
                .any(|p| p.starts_with("container") || p == "size");
            if follows || precedes {
                sizes.insert(n);
            }
        }
    }
    sizes
}

/// One value → filled (`ok`); none → empty, no flag; several → empty and flagged.
pub fn parse_max_box(text: &str) -> Field<u32> {
    let sizes = container_sizes(text);
    match sizes.len() {
        0 => Field::unknown(),
        1 => Field::ok(*sizes.iter().next().expect("one value")),
        _ => Field {
            value: None,
            flag: Flag::Check,
            reason: Some(REASON_SEVERAL_SIZES.into()),
        },
    }
}
