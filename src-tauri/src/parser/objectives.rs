//! The Primary Objectives grammar:
//!
//! ```text
//! Deliver <a>/<N> SCU of <Commodity> to <Location>.     → one requirement, total N
//!   Collect <Commodity> from <Location>.                → pickups of the Deliver above it
//! ```
//!
//! - Bullets and junk are dropped; a line not starting with a verb continues the previous clause.
//! - `a` is progress and ignored; N is the total and the delivery's SCU.
//! - One Collect: its pickup carries N. Several: each pickup's SCU is unknown (no invented split).
//! - Two Delivers of the same commodity stay two requirements (the game's structure).

use super::clean::{
    fix_digits, is_leading_junk, is_symbol_junk, join, min_conf, tokens_of, trim_clause_end, verb,
    Token, Verb,
};
use super::resolve::{Catalog, Resolution};
use super::{
    Field, Flag, NameField, ParsedRequirement, ParsedStop, References, LOW_CONFIDENCE,
    REASON_LOW_CONFIDENCE, REASON_NOT_IN_LIST,
};
use crate::ocr::OcrLine;

#[derive(Debug, Clone)]
struct Clause {
    verb: Verb,
    tokens: Vec<Token>,
}

fn is_header(tokens: &[Token]) -> bool {
    let upper: Vec<String> = tokens.iter().map(|t| t.text.to_uppercase()).collect();
    upper
        .windows(2)
        .any(|w| w[0].contains("PRIMARY") && w[1].starts_with("OBJ"))
}

fn is_secondary(tokens: &[Token]) -> bool {
    let upper: Vec<String> = tokens.iter().map(|t| t.text.to_uppercase()).collect();
    upper
        .windows(2)
        .any(|w| w[0].contains("SECONDARY") && w[1].starts_with("OBJ"))
}

/// Builds clauses from the region's lines; text before the first verb is reported.
fn clauses(lines: &[&OcrLine], problems: &mut Vec<String>) -> Vec<Clause> {
    let mut out: Vec<Clause> = Vec::new();
    for line in lines {
        let mut tokens = tokens_of(line);
        if is_header(&tokens) {
            continue;
        }
        if is_secondary(&tokens) {
            break;
        }
        while tokens.first().is_some_and(|t| is_leading_junk(&t.text)) {
            tokens.remove(0);
        }
        tokens.retain(|t| !is_symbol_junk(&t.text));
        let mut orphan: Vec<Token> = Vec::new();
        for token in tokens {
            if let Some(v) = verb(&token.text) {
                out.push(Clause {
                    verb: v,
                    tokens: Vec::new(),
                });
            } else if let Some(current) = out.last_mut() {
                current.tokens.push(token);
            } else {
                orphan.push(token);
            }
        }
        if !orphan.is_empty() {
            problems.push(format!("Couldn't read this line: \"{}\"", join(&orphan)));
        }
    }
    for clause in &mut out {
        trim_clause_end(&mut clause.tokens);
    }
    out
}

fn position(tokens: &[Token], word: &str) -> Option<usize> {
    tokens
        .iter()
        .position(|t| t.text.eq_ignore_ascii_case(word))
}

/// `0/50` (or `0/ 50`, `O/5O`, `0/50SCU`): the total N, its confidence and whether digits were fixed.
fn total(tokens: &[Token]) -> Option<(u32, f32, bool, usize)> {
    let slash = tokens.iter().take(3).position(|t| t.text.contains('/'))?;
    let after = tokens[slash]
        .text
        .split_once('/')
        .map(|(_, r)| r.to_string())
        .unwrap_or_default();
    let (text, conf, next) = if after.trim().is_empty() {
        let t = tokens.get(slash + 1)?;
        (t.text.clone(), t.conf, slash + 2)
    } else {
        (after, tokens[slash].conf, slash + 1)
    };
    let text = text.trim_end_matches([',', '.']);
    let text = text
        .strip_suffix("SCU")
        .or_else(|| text.strip_suffix("scu"))
        .unwrap_or(text);
    let (n, fixed) = fix_digits(text)?;
    Some((n, conf, fixed, next))
}

fn name_field(catalog: &Catalog, tokens: &[Token], strip_body: bool) -> NameField {
    let read = join(tokens);
    if read.trim().is_empty() {
        return NameField {
            value: None,
            flag: Flag::Missing,
            reason: Some("Couldn't read this name".into()),
            resolution: Resolution::Unresolved,
            read,
        };
    }
    let (value, resolution) = catalog.resolve(&read, strip_body);
    let (flag, reason) = if resolution == Resolution::Unresolved {
        (Flag::Check, Some(REASON_NOT_IN_LIST.to_string()))
    } else if min_conf(tokens) < LOW_CONFIDENCE {
        (Flag::Check, Some(REASON_LOW_CONFIDENCE.to_string()))
    } else {
        (Flag::Ok, None)
    };
    NameField {
        value: Some(value),
        flag,
        reason,
        resolution,
        read,
    }
}

struct Deliver {
    commodity: NameField,
    total: Field<u32>,
    location: NameField,
}

fn parse_deliver(tokens: &[Token], refs: &References) -> Result<Deliver, String> {
    let (n, conf, fixed, mut i) = total(tokens).ok_or("no SCU amount")?;
    if tokens.get(i).is_some_and(|t| {
        let up = t.text.to_uppercase().replace('5', "S");
        up.trim_end_matches([',', '.']) == "SCU"
    }) {
        i += 1;
    }
    if tokens
        .get(i)
        .is_some_and(|t| t.text.eq_ignore_ascii_case("of") || t.text.eq_ignore_ascii_case("0f"))
    {
        i += 1;
    }
    let rest = &tokens[i.min(tokens.len())..];
    let to = position(rest, "to").ok_or("no \"to <location>\"")?;
    if n == 0 {
        return Err("the SCU amount is 0".into());
    }
    let total = if fixed || conf < LOW_CONFIDENCE {
        Field::check(n, REASON_LOW_CONFIDENCE)
    } else {
        Field::ok(n)
    };
    Ok(Deliver {
        commodity: name_field(&refs.commodities, &rest[..to], false),
        total,
        location: name_field(&refs.locations, &rest[to + 1..], true),
    })
}

fn parse_collect(tokens: &[Token], refs: &References) -> Result<(NameField, NameField), String> {
    let from = position(tokens, "from").ok_or("no \"from <location>\"")?;
    Ok((
        name_field(&refs.commodities, &tokens[..from], false),
        name_field(&refs.locations, &tokens[from + 1..], true),
    ))
}

fn same_commodity(a: &NameField, b: &NameField) -> bool {
    match (&a.value, &b.value) {
        (Some(x), Some(y)) => super::resolve::fold_key(x) == super::resolve::fold_key(y),
        _ => false,
    }
}

pub fn parse_objectives(
    lines: &[&OcrLine],
    refs: &References,
    problems: &mut Vec<String>,
) -> Vec<ParsedRequirement> {
    let mut requirements: Vec<ParsedRequirement> = Vec::new();
    for clause in clauses(lines, problems) {
        let text = join(&clause.tokens);
        match clause.verb {
            Verb::Deliver => match parse_deliver(&clause.tokens, refs) {
                Ok(d) => {
                    let scu = d.total.clone();
                    requirements.push(ParsedRequirement {
                        commodity: d.commodity,
                        total_scu: d.total,
                        pickups: Vec::new(),
                        deliveries: vec![ParsedStop {
                            location: d.location,
                            scu,
                        }],
                    });
                }
                Err(why) => problems.push(format!("Couldn't read \"Deliver {text}\" ({why})")),
            },
            Verb::Collect => match (parse_collect(&clause.tokens, refs), requirements.last_mut()) {
                (Err(why), _) => problems.push(format!("Couldn't read \"Collect {text}\" ({why})")),
                (Ok(_), None) => {
                    problems.push(format!("\"Collect {text}\" has no Deliver line above it"))
                }
                (Ok((commodity, location)), Some(current)) => {
                    if !same_commodity(&commodity, &current.commodity) {
                        problems.push(format!(
                            "\"Collect {text}\" names a different commodity than its Deliver line ({})",
                            current.commodity.value.as_deref().unwrap_or("?")
                        ));
                    }
                    current.pickups.push(ParsedStop {
                        location,
                        scu: Field::unknown(),
                    });
                }
            },
        }
    }
    for (i, requirement) in requirements.iter_mut().enumerate() {
        match requirement.pickups.len() {
            0 => problems.push(format!(
                "Objective {}: no Collect (pickup) line was read",
                i + 1
            )),
            // One pickup carries the whole amount; several stay unknown (never an invented split).
            1 => requirement.pickups[0].scu = requirement.total_scu.clone(),
            _ => {}
        }
    }
    if requirements.is_empty() {
        problems.push("No objectives were read".into());
    }
    requirements
}
