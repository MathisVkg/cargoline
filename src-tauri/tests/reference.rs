//! Headless tests for the bundled reference lists and used-commodity query.

use cargo_line_lib::contracts::repository::create_contract;
use cargo_line_lib::contracts::validation::validate_new_contract;
use cargo_line_lib::contracts::*;
use cargo_line_lib::database::models::ContractSource;
use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::locations::list_used_commodities;
use cargo_line_lib::reference::{
    commodities, locations, parse_list, ReferenceCommodity, ReferenceLocation, COMMODITIES_JSON,
    LOCATIONS_JSON,
};

fn assert_clean(names: &[&str]) {
    let mut keys: Vec<String> = names
        .iter()
        .map(|n| {
            n.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        })
        .collect();
    for n in names {
        assert!(!n.trim().is_empty(), "empty name");
        assert!(
            !n.contains("~mission(") && !n.contains("<EM4>"),
            "unresolved token in {n:?}"
        );
    }
    let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let mut sorted = lower.clone();
    sorted.sort();
    assert_eq!(lower, sorted, "items must be sorted case-insensitively");
    let total = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), total, "names must be unique case-insensitively");
}

#[test]
fn bundled_locations_file_is_valid() {
    let list = locations().expect("locations.json parses and validates");
    assert_eq!(list.game_version, "4.10.3-LIVE.13165946");
    assert!(list.source.contains("api.star-citizen.wiki"));
    assert!(list.attribution.contains("api.star-citizen.wiki"));
    assert!(!list.items.is_empty(), "locations list must not be empty");
    assert_clean(
        &list
            .items
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>(),
    );
    // Raw file contains no unresolved tokens anywhere.
    assert!(!LOCATIONS_JSON.contains("~mission(") && !LOCATIONS_JSON.contains("<EM4>"));
}

#[test]
fn bundled_commodities_file_is_valid() {
    let list = commodities().expect("commodities.json parses and validates");
    assert_eq!(list.game_version, "4.10.3-LIVE.13165946");
    assert!(list.attribution.contains("api.star-citizen.wiki"));
    assert!(!list.items.is_empty(), "commodities list must not be empty");
    assert_clean(
        &list
            .items
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>(),
    );
    assert!(!COMMODITIES_JSON.contains("~mission(") && !COMMODITIES_JSON.contains("<EM4>"));
    // `name`, not `display_name` ("Name (Group)").
    assert!(list.items.iter().all(|c| c
        .group
        .as_deref()
        .is_none_or(|g| !c.name.ends_with(&format!("({g})")))));
}

#[test]
fn invalid_lists_are_rejected_not_panicking() {
    let header = r#""source":"s","attribution":"a","game_version":"g","generated_at":"2026-11-19""#;
    let list = |items: &str| format!("{{{header},\"items\":[{items}]}}");
    assert!(parse_list::<ReferenceLocation>("not json").is_err());
    assert!(
        parse_list::<ReferenceLocation>(&list(r#"{"name":"","system":null,"body":null}"#)).is_err()
    );
    assert!(parse_list::<ReferenceLocation>(&list(
        r#"{"name":"Port Tressler","system":null,"body":null},{"name":"port  tressler","system":null,"body":null}"#
    ))
    .is_err());
    assert!(parse_list::<ReferenceLocation>(&list(
        r#"{"name":"Zeta","system":null,"body":null},{"name":"alpha","system":null,"body":null}"#
    ))
    .is_err());
    assert!(parse_list::<ReferenceLocation>(&list(
        r#"{"name":"~mission(Location|Address)","system":null,"body":null}"#
    ))
    .is_err());
    assert!(
        parse_list::<ReferenceCommodity>(&list(r#"{"name":"<EM4>Gold</EM4>","group":null}"#))
            .is_err()
    );
    let ok = parse_list::<ReferenceCommodity>(&list(
        r#"{"name":"aluminum","group":"Metal"},{"name":"Gold","group":null}"#,
    ))
    .unwrap();
    assert_eq!(ok.items.len(), 2);
}

#[test]
fn used_commodities_are_distinct_and_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    let mut conn = db.connection().unwrap();
    assert!(list_used_commodities(&conn).unwrap().is_empty());
    let at = AcceptanceTime {
        utc_ms: 1_000,
        local_date: "2026-11-19".into(),
        utc_offset_minutes: 60,
    };
    for commodity in ["Waste", "aluminum", "Aluminum ", "Stims"] {
        let input = NewContractInput {
            title: "T".into(),
            reward_auec: "1".into(),
            requirements: vec![NewRequirementInput {
                commodity_name: commodity.into(),
                quantity_scu: "1".into(),
                max_box_size_scu: None,
                pickups: vec![NewStopInput {
                    location_name: "A".into(),
                    quantity_scu: None,
                }],
                deliveries: vec![NewStopInput {
                    location_name: "B".into(),
                    quantity_scu: None,
                }],
            }],
        };
        create_contract(
            &mut conn,
            &validate_new_contract(&input).unwrap(),
            &at,
            ContractSource::Manual,
        )
        .unwrap();
    }
    assert_eq!(
        list_used_commodities(&conn).unwrap(),
        ["aluminum", "Stims", "Waste"]
    );
    // Saving is unchanged: requirements keep the typed name.
    let stored: Vec<String> = conn
        .prepare("SELECT commodity_name FROM cargo_requirements ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(stored, ["Waste", "aluminum", "Aluminum", "Stims"]);
}
