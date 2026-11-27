//! Parser unit tests: OCR text samples, including the real junk seen in live captures.
//! No engine, no desktop, no database.

use cargo_line_lib::ocr::{
    AnchorsFound, OcrBox, OcrLine, OcrRegion, OcrResult, OcrTimings, OcrWord, RegionKind,
    RegionSource, ScreenState,
};
use cargo_line_lib::parser::clean::{fix_digits, fold_apostrophes, is_leading_junk, verb, Verb};
use cargo_line_lib::parser::resolve::strip_body;
use cargo_line_lib::parser::{
    parse_contract, Catalog, Flag, ParsedContract, References, Resolution,
};

const B: OcrBox = OcrBox {
    x: 0,
    y: 0,
    width: 1,
    height: 1,
};

/// A line from `"word word"`; `conf` applies to every word, `word@NN` overrides one word.
fn line(text: &str, conf: f32) -> OcrLine {
    let words = text
        .split_whitespace()
        .map(|w| match w.rsplit_once('@') {
            Some((t, c)) if !t.is_empty() && c.parse::<f32>().is_ok() => OcrWord {
                text: t.into(),
                confidence: c.parse().unwrap(),
                bbox: B,
            },
            _ => OcrWord {
                text: w.into(),
                confidence: conf,
                bbox: B,
            },
        })
        .collect();
    OcrLine::from_words(words)
}

fn region(kind: RegionKind, lines: &[(&str, f32)]) -> OcrRegion {
    OcrRegion {
        kind,
        bbox: B,
        source: RegionSource::Anchor,
        lines: lines.iter().map(|(t, c)| line(t, *c)).collect(),
    }
}

fn ocr(
    button: ScreenState,
    title: &[(&str, f32)],
    reward: &[(&str, f32)],
    objectives: &[&str],
) -> OcrResult {
    let obj: Vec<(&str, f32)> = objectives.iter().map(|t| (*t, 92.0)).collect();
    OcrResult {
        width: 3840,
        height: 2160,
        anchors: AnchorsFound {
            header: true,
            details: true,
            reward: true,
        },
        screen: button,
        regions: vec![
            region(RegionKind::Title, title),
            region(RegionKind::Reward, reward),
            region(RegionKind::Objectives, &obj),
        ],
        timings: OcrTimings::default(),
    }
}

fn refs() -> References {
    References {
        locations: Catalog::new([
            "Baijini Point",
            "ARC-L1 Wide Forest Station",
            "ARC-L2 Lively Pathway Station",
            "ARC-L5 Yellow Core Station",
            "Everus Harbor",
            "Port Tressler",
            "Seraphim Station",
            "Sacren's Plot",
            "HDMS-Thedus",
            "ArcCorp Mining Area 061",
        ]),
        commodities: Catalog::new([
            "Aluminum",
            "Tungsten",
            "Agricultural Supplies",
            "Souvenirs",
            "Distilled Spirits",
        ]),
    }
}

fn objectives(lines: &[&str]) -> ParsedContract {
    parse_contract(
        &ocr(
            ScreenState::Accepted,
            &[("Title", 95.0)],
            &[("¤ 1,000", 90.0)],
            lines,
        ),
        &refs(),
    )
}

/* ------------------------------------------------------------ cleaning */

#[test]
fn verbs_digits_and_bullets() {
    for v in ["Deliver", "Dellver", "Deiiver", "DELIVER"] {
        assert_eq!(verb(v), Some(Verb::Deliver), "{v}");
    }
    for v in ["Collect", "Coliect", "CoIlect"] {
        assert_eq!(verb(v), Some(Verb::Collect), "{v}");
    }
    assert_eq!(verb("Delivery"), None);
    assert_eq!(fix_digits("84"), Some((84, false)));
    assert_eq!(fix_digits("5O"), Some((50, true)));
    assert_eq!(fix_digits("9l"), Some((91, true)));
    assert_eq!(fix_digits("1,000"), Some((1000, false)));
    assert_eq!(
        fix_digits("SOS"),
        None,
        "words without a real digit are never converted"
    );
    assert_eq!(fix_digits("4x"), None);
    for junk in ["©", "~", "<~", "|", "o", "O", "0", "co)", "«", "@"] {
        assert!(is_leading_junk(junk), "{junk}");
    }
    for word in ["IV.", "61.", "Hurston.", "Deliver"] {
        assert!(!is_leading_junk(word), "{word}");
    }
    assert_eq!(fold_apostrophes("Sacren’'s Plot"), "Sacren's Plot");
    assert_eq!(strip_body("Port Tressler above microTech"), "Port Tressler");
    assert_eq!(strip_body("Sacren's Plot on pyro IV"), "Sacren's Plot");
    assert_eq!(strip_body("Baijini Point"), "Baijini Point");
}

/* ---------------------------------------------------------- objectives */

#[test]
fn real_junk_from_tasks_06_and_08() {
    // A live capture, line for line (bullets as `<~`, `~`, `|`, a wrapped body).
    let parsed = objectives(&[
        "PRIMARY OBJECTIVES",
        "<~ Deliver 0/50 SCU of Aluminum to Port Tressler above |",
        "microTech.",
        "Collect Aluminum from ARC-L1 Wide Forest Station.",
        "~ Deliver 0/47 SCU of Aluminum to Everus Harbor above",
        "Hurston.",
        "© Collect Aluminum from ARC-L1 Wide Forest Station. <",
    ]);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    assert_eq!(
        parsed.requirements.len(),
        2,
        "two Delivers of the same commodity stay two requirements"
    );
    let r = &parsed.requirements[0];
    assert_eq!(r.commodity.value.as_deref(), Some("Aluminum"));
    assert_eq!(r.total_scu.value, Some(50));
    assert_eq!(
        r.deliveries[0].location.value.as_deref(),
        Some("Port Tressler")
    );
    assert_eq!(r.deliveries[0].location.resolution, Resolution::Stripped);
    assert_eq!(r.deliveries[0].scu.value, Some(50));
    assert_eq!(r.pickups.len(), 1);
    assert_eq!(
        r.pickups[0].location.value.as_deref(),
        Some("ARC-L1 Wide Forest Station")
    );
    assert_eq!(
        r.pickups[0].scu.value,
        Some(50),
        "one Collect carries the whole amount"
    );
    assert_eq!(
        parsed.requirements[1].deliveries[0]
            .location
            .value
            .as_deref(),
        Some("Everus Harbor")
    );
    assert_eq!(
        parsed.requirements[1].pickups[0].location.value.as_deref(),
        Some("ARC-L1 Wide Forest Station")
    );
}

#[test]
fn several_collects_leave_the_pickup_split_unknown() {
    let parsed = objectives(&[
        "o Deliver 0/11 SCU of Agricultural Supplies to Everus Harbor above Hurston.",
        "C Collect Agricultural Supplies from Port Tressler.",
        "co) Collect Agricultural Supplies from Seraphim Station. co)",
    ]);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    let r = &parsed.requirements[0];
    assert_eq!(r.pickups.len(), 2);
    assert!(
        r.pickups
            .iter()
            .all(|p| p.scu.value.is_none() && p.scu.flag == Flag::Ok),
        "no invented split"
    );
    assert_eq!(
        r.pickups[1].location.value.as_deref(),
        Some("Seraphim Station")
    );
    assert_eq!(r.deliveries[0].scu.value, Some(11));
}

#[test]
fn partial_progress_and_confusable_numbers() {
    let parsed = objectives(&[
        "Deliver 12/51 SCU of Aluminum to Baijini Point above ArcCorp.",
        "Collect Aluminum from ARC-L2 Lively Pathway Station.",
        "Deliver O/9l SCU of Tungsten to Baijini Point.",
        "Collect Tungsten from ARC-L2 Lively Pathway Station.",
        "Deliver 0/ 33 SCU of Tungsten to Baijini Point.",
        "Collect Tungsten from ARC-L2 Lively Pathway Station.",
    ]);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    assert_eq!(
        parsed.requirements[0].total_scu.value,
        Some(51),
        "the right-hand number is the total"
    );
    assert_eq!(parsed.requirements[0].total_scu.flag, Flag::Ok);
    assert_eq!(parsed.requirements[1].total_scu.value, Some(91));
    assert_eq!(
        parsed.requirements[1].total_scu.flag,
        Flag::Check,
        "fixed digits are flagged"
    );
    assert_eq!(parsed.requirements[1].pickups[0].scu.flag, Flag::Check);
    assert_eq!(parsed.requirements[2].total_scu.value, Some(33));
}

#[test]
fn low_confidence_numbers_are_flagged() {
    let parsed = objectives(&[
        "Deliver 0/45@41 SCU of Aluminum to Everus Harbor.",
        "Collect Aluminum from ARC-L5 Yellow Core Station.",
    ]);
    assert_eq!(parsed.requirements[0].total_scu.value, Some(45));
    assert_eq!(parsed.requirements[0].total_scu.flag, Flag::Check);
}

#[test]
fn folded_ambiguous_and_unresolved_names() {
    let parsed = objectives(&[
        "Deliver 0/45 SCU of Aluminum to Sacren•s Plot on pyro lv.",
        "Collect Aluminum from ARC-LS Yellow Core Station.",
        "Deliver 0/20 SCU of Alumlnum to Nowhere Outpost above Hurston.",
        "Collect Alumlnum from ARC-LI Wide Forest Station.",
    ]);
    let r = &parsed.requirements;
    assert_eq!(
        r[0].deliveries[0].location.value.as_deref(),
        Some("Sacren's Plot")
    );
    assert_eq!(r[0].deliveries[0].location.resolution, Resolution::Stripped);
    assert_eq!(
        r[0].pickups[0].location.value.as_deref(),
        Some("ARC-L5 Yellow Core Station")
    );
    assert_eq!(r[0].pickups[0].location.resolution, Resolution::Folded);
    assert_eq!(
        r[1].commodity.value.as_deref(),
        Some("Aluminum"),
        "Alumlnum folds to one commodity"
    );
    assert_eq!(r[1].commodity.resolution, Resolution::Folded);
    // Unresolved: the cleaned text without the body, flagged, never the nearest name.
    let unknown = &r[1].deliveries[0].location;
    assert_eq!(unknown.value.as_deref(), Some("Nowhere Outpost"));
    assert_eq!(unknown.resolution, Resolution::Unresolved);
    assert_eq!(
        (unknown.flag, unknown.reason.as_deref()),
        (Flag::Check, Some("Not in list"))
    );
    assert_eq!(
        r[1].pickups[0].location.value.as_deref(),
        Some("ARC-L1 Wide Forest Station")
    );

    // Two names that fold together: no match.
    let ambiguous = Catalog::new(["Station SO1", "Station 501"]);
    assert_eq!(
        ambiguous.resolve("Station S0l", false).1,
        Resolution::Unresolved
    );
    assert_eq!(
        ambiguous.resolve("station so1", false),
        ("Station SO1".into(), Resolution::Exact)
    );
    // The user's names count too; the bundled spelling wins on duplicates.
    let mixed = Catalog::new(["Port Tressler", "port tressler", "My Outpost"]);
    assert_eq!(mixed.resolve("PORT  TRESSLER", true).0, "Port Tressler");
    assert_eq!(mixed.resolve("My Outpost", true).1, Resolution::Exact);
}

#[test]
fn problems_are_reported_not_guessed() {
    let parsed = objectives(&[
        "Collect Aluminum from Port Tressler.",
        "Deliver 0/10 SCU of Aluminum to Baijini Point.",
        "Collect Tungsten from Port Tressler.",
        "Deliver 0/5 SCU of Tungsten to Everus Harbor.",
        "Deliver the goods somewhere",
    ]);
    let problems = parsed.problems.join("\n");
    assert!(
        problems.contains("has no Deliver line above it"),
        "{problems}"
    );
    assert!(problems.contains("different commodity"), "{problems}");
    assert!(problems.contains("Objective 2: no Collect"), "{problems}");
    assert!(
        problems.contains("Couldn't read \"Deliver the goods somewhere\""),
        "{problems}"
    );
    assert_eq!(parsed.requirements.len(), 2);
    assert!(parsed.requirements[1].pickups.is_empty());

    let junk_first = objectives(&[
        "Vv Veliver U/Si oUU",
        "Deliver 0/91 SCU of Aluminum to Baijini Point.",
        "Collect Aluminum from ARC-L2 Lively Pathway Station.",
    ]);
    assert!(junk_first
        .problems
        .iter()
        .any(|p| p.starts_with("Couldn't read this line")));
    assert_eq!(junk_first.requirements.len(), 1);

    let empty = objectives(&["PRIMARY OBJECTIVES"]);
    assert_eq!(empty.problems, ["No objectives were read"]);
    assert!(empty.requirements.is_empty());
}

#[test]
fn secondary_objectives_are_not_read() {
    let parsed = objectives(&[
        "Deliver 0/10 SCU of Aluminum to Baijini Point.",
        "Collect Aluminum from Port Tressler.",
        "SECONDARY OBJECTIVES",
        "Deliver 0/99 SCU of Tungsten to Everus Harbor.",
    ]);
    assert_eq!(parsed.requirements.len(), 1);
}

/* ------------------------------------------------------ title, reward */

#[test]
fn title_rules() {
    let p = |lines: &[(&str, f32)]| {
        parse_contract(&ocr(ScreenState::Accepted, lines, &[], &[]), &refs()).title
    };
    let t = p(&[
        ("Junior Rank - Direct Medium Cargo Haul", 95.0),
        ("S", 30.0),
    ]);
    assert_eq!(
        (t.value.as_deref(), t.flag),
        (Some("Junior Rank - Direct Medium Cargo Haul"), Flag::Ok)
    );
    let low = p(&[("Junlor Rank Cargo", 52.0)]);
    assert_eq!(
        (low.flag, low.reason.as_deref()),
        (Flag::Check, Some("Check — read with low confidence"))
    );
    let empty = p(&[("|", 20.0)]);
    assert_eq!((empty.value, empty.flag), (None, Flag::Missing));
}

#[test]
fn reward_rules() {
    let r = |button, lines: &[(&str, f32)]| {
        parse_contract(&ocr(button, &[], lines, &[]), &refs()).reward
    };
    use ScreenState::{Accepted, Offer};
    // Currency-glyph junk around the number.
    let ok = r(Accepted, &[("| © 59,500", 80.0)]);
    assert_eq!((ok.value, ok.flag), (Some(59_500), Flag::Ok));
    assert_eq!(r(Accepted, &[("om 64,250", 70.0)]).value, Some(64_250));
    assert_eq!(
        r(Accepted, &[("2” 1,234,567", 90.0)]).value,
        Some(1_234_567)
    );
    // 40–60: filled and flagged.
    let low = r(Accepted, &[("& 57,750", 47.6)]);
    assert_eq!((low.value, low.flag), (Some(57_750), Flag::Check));
    // Below 40: empty and flagged.
    let bad = r(Accepted, &[("_ 357,000", 36.8)]);
    assert_eq!(
        (bad.value, bad.flag, bad.reason.as_deref()),
        (None, Flag::Missing, Some("Couldn't read reward"))
    );
    // Offers: below 60 is empty (the cursor can hide a digit).
    assert_eq!(r(Offer, &[("| 89,500", 50.7)]).value, None);
    assert_eq!(r(Offer, &[("476,500", 75.0)]).value, Some(476_500));
    // The word looks fine but its line is damaged: the line's confidence counts.
    assert_eq!(r(Offer, &[("| lipg@30 250@90", 0.0)]).value, None);
    // No thousands separator: a fragment, flagged even when confident.
    let bare = r(Accepted, &[("¤ 500", 95.0)]);
    assert_eq!((bare.value, bare.flag), (Some(500), Flag::Check));
    // Letters glued to the digits never match.
    assert_eq!(r(Accepted, &[("©H476,500", 90.0)]).flag, Flag::Missing);
    assert_eq!(r(Accepted, &[("Reward", 90.0)]).flag, Flag::Missing);
    // Bad grouping is not a reward.
    assert_eq!(r(Accepted, &[("59,50", 90.0)]).flag, Flag::Missing);
}

#[test]
fn screen_is_passed_through() {
    let unknown = parse_contract(&ocr(ScreenState::Unknown, &[], &[], &[]), &refs());
    assert_eq!(unknown.screen, ScreenState::Unknown);
}

/* ------------------------------------------------- Lagrange */

#[test]
fn lagrange_long_names_resolve_to_the_short_station_names() {
    use cargo_line_lib::parser::resolve::lagrange_candidate;
    let refs = References::build(&[], &[]);
    let resolve = |text: &str| refs.locations.resolve(text, true);
    // The two live examples.
    assert_eq!(
        resolve("Long Forest Station at microTech's L2 Lagrange point"),
        ("MIC-L2 Long Forest Station".into(), Resolution::Lagrange)
    );
    assert_eq!(
        resolve("Red Crossroads Station at microTech’s L4 Lagrange point"),
        ("MIC-L4 Red Crossroads Station".into(), Resolution::Lagrange)
    );
    // OCR variants: apostrophes, L1 read as LI, a trailing full stop, a confusable in the name.
    assert_eq!(
        resolve("Wide Forest Station at ArcCorp´s LI Lagrange point.").0,
        "ARC-L1 Wide Forest Station"
    );
    assert_eq!(
        resolve("Green GIade Station at Hurston's L1 Lagrange point").0,
        "HUR-L1 Green Glade Station"
    );
    // A body without a code, and a station missing from the list: unresolved, never the nearest.
    let unknown_body = resolve("Long Forest Station at Pyro's L2 Lagrange point");
    assert_eq!(unknown_body.1, Resolution::Unresolved);
    assert_eq!(
        unknown_body.0,
        "Long Forest Station at Pyro's L2 Lagrange point"
    );
    assert_eq!(
        resolve("Quiet Harbor Station at Crusader's L2 Lagrange point").1,
        Resolution::Unresolved
    );
    assert_eq!(
        lagrange_candidate("Quiet Harbor Station at Crusader's L2 Lagrange point").as_deref(),
        Some("CRU-L2 Quiet Harbor Station")
    );
    // Not the long form.
    assert_eq!(lagrange_candidate("Baijini Point above ArcCorp"), None);
    assert_eq!(
        lagrange_candidate("Station at ArcCorp's L9 Lagrange point"),
        None
    );
    assert_eq!(lagrange_candidate("at ArcCorp's L1 Lagrange point"), None);
    // Short names still resolve exactly.
    assert_eq!(resolve("MIC-L2 Long Forest Station").1, Resolution::Exact);
}

#[test]
fn every_bundled_lagrange_station_resolves_from_its_long_form() {
    let refs = References::build(&[], &[]);
    let bodies = [
        ("ARC", "ArcCorp"),
        ("CRU", "Crusader"),
        ("HUR", "Hurston"),
        ("MIC", "microTech"),
    ];
    let list = cargo_line_lib::reference::locations().unwrap();
    let mut count = 0;
    for item in &list.items {
        let Some((code, rest)) = item.name.split_once("-L") else {
            continue;
        };
        let Some(body) = bodies.iter().find(|(c, _)| *c == code).map(|(_, b)| *b) else {
            continue;
        };
        let (n, name) = rest.split_once(' ').unwrap();
        let long = format!("{name} at {body}'s L{n} Lagrange point");
        assert_eq!(
            refs.locations.resolve(&long, true),
            (item.name.clone(), Resolution::Lagrange),
            "{long}"
        );
        count += 1;
    }
    assert_eq!(count, 18, "ARC-L1..5, CRU-L1/L4/L5, HUR-L1..5, MIC-L1..5");
}

#[test]
fn lagrange_deliveries_parse_in_context() {
    let parsed = parse_contract(
        &ocr(
            ScreenState::Accepted,
            &[("Title", 95.0)],
            &[("¤ 12,000", 90.0)],
            &[
                "Deliver 0/5 SCU of Pressurized Ice to Long Forest Station at microTech's L2 Lagrange",
                "point.",
                "Collect Pressurized Ice from Red Crossroads Station at microTech's L4 Lagrange point.",
            ],
        ),
        &References::build(&[], &[]),
    );
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    let r = &parsed.requirements[0];
    assert_eq!(
        r.deliveries[0].location.value.as_deref(),
        Some("MIC-L2 Long Forest Station")
    );
    assert_eq!(
        (
            r.deliveries[0].location.flag,
            r.deliveries[0].location.resolution
        ),
        (Flag::Ok, Resolution::Lagrange)
    );
    assert_eq!(
        r.pickups[0].location.value.as_deref(),
        Some("MIC-L4 Red Crossroads Station")
    );
}

/* ------------------------------------------- max box size */

#[test]
fn max_container_size_from_the_details_text() {
    use cargo_line_lib::parser::details::{container_sizes, parse_max_box, REASON_SEVERAL_SIZES};
    // Each sentence pattern seen in mission texts.
    for (text, size) in [
        ("Also, we need this done today. At most the containers will be 8 SCU in size.", 8),
        ("Pick up the crates (max container size of 4 SCU) and bring them over.", 4),
        ("They've processed a bunch of Aluminum into containers 8 SCU or smaller, and are looking", 8),
        ("Bring it all over (4 SCU or smaller).", 4),
        ("containers will be 16SCU in size", 16),
    ] {
        let field = parse_max_box(text);
        assert_eq!((field.value, field.flag), (Some(size), Flag::Ok), "{text}");
    }
    // The Ling Family text: a container type, not a limit.
    let ling = "Remember to equip your handheld tractor beam, and a ship that can fit 2 SCU cargo containers.";
    assert!(container_sizes(ling).is_empty());
    let none = parse_max_box(ling);
    assert_eq!(
        (none.value, none.flag, none.reason),
        (None, Flag::Ok, None),
        "empty, no flag"
    );
    // Other SCU mentions outside a container sentence are ignored.
    assert!(
        container_sizes("Deliver 0/84 SCU of Aluminum. We pay well for 12 SCU loads.").is_empty()
    );
    assert!(container_sizes("You will need 32 SCU of space.").is_empty());
    // The same value twice is one value; two different values are flagged, never chosen.
    assert_eq!(
        parse_max_box("containers 8 SCU or smaller. At most the containers will be 8 SCU in size.")
            .value,
        Some(8)
    );
    let conflict = parse_max_box("containers 8 SCU or smaller. Some are (4 SCU or smaller).");
    assert_eq!(
        (conflict.value, conflict.flag, conflict.reason.as_deref()),
        (None, Flag::Check, Some(REASON_SEVERAL_SIZES))
    );
    assert_eq!(parse_max_box("").flag, Flag::Ok);
}

#[test]
fn parsed_contract_carries_the_max_box_size() {
    let mut result = ocr(
        ScreenState::Accepted,
        &[("Title", 95.0)],
        &[("¤ 1,000", 90.0)],
        &[
            "Deliver 0/10 SCU of Aluminum to Baijini Point.",
            "Collect Aluminum from Port Tressler.",
        ],
    );
    result.regions.push(region(
        RegionKind::Details,
        &[
            ("At most the containers will be", 90.0),
            ("8 SCU in size.", 90.0),
        ],
    ));
    let parsed = parse_contract(&result, &refs());
    assert_eq!(
        parsed.max_box_scu.value,
        Some(8),
        "wrapped over two OCR lines"
    );
    let without = parse_contract(&ocr(ScreenState::Accepted, &[], &[], &[]), &refs());
    assert_eq!(without.max_box_scu.value, None);
}
