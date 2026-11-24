//! Local fixture regression (fixtures stay in .agent-local/, not in the repository): capture PNG → OCR (bundled tesseract.exe) → parser,
//! on the 7 Task 06 screenshots, against `.agent-local/spikes/ground-truth.json`.
//! Skipped with a message when the fixtures or the bundle are missing.
//!
//! Pass bar (Task 09): structure, SCU, commodities and locations correct on all 7; title 7/7;
//! reward correct on the 3 Accepted screenshots; zero wrong values filled without a flag.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cargo_line_lib::ocr::engine::{CancelToken, Engine};
use cargo_line_lib::ocr::recognize_file;
use cargo_line_lib::ocr::ScreenState;
use cargo_line_lib::parser::{parse_contract, Field, Flag, NameField, References, Resolution};
use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[derive(Default)]
struct Tally {
    checked: usize,
    correct: usize,
    wrong_unflagged: Vec<String>,
    wrong_flagged: usize,
    empty_flagged: usize,
}

impl Tally {
    /// A filled value must be right, or flagged; an empty value must be flagged.
    fn field<T: PartialEq + std::fmt::Debug>(
        &mut self,
        what: String,
        field: &Field<T>,
        truth: Option<&T>,
    ) {
        self.checked += 1;
        match (&field.value, truth) {
            (Some(v), Some(t)) if v == t => self.correct += 1,
            (None, None) => self.correct += 1,
            (None, _) => {
                assert_ne!(field.flag, Flag::Ok, "{what}: empty but not flagged");
                self.empty_flagged += 1;
            }
            (Some(v), _) if field.flag == Flag::Ok => self
                .wrong_unflagged
                .push(format!("{what}: {v:?} vs {truth:?}")),
            (Some(_), _) => self.wrong_flagged += 1,
        }
    }

    fn name(&mut self, what: String, field: &NameField, truth: &str, stats: &mut [usize; 5]) {
        stats[field.resolution as usize] += 1;
        let as_field = Field {
            value: field.value.clone(),
            flag: field.flag,
            reason: field.reason.clone(),
        };
        self.field(what, &as_field, Some(&truth.to_string()));
    }
}

#[test]
fn full_pipeline_meets_the_task_09_pass_bar() {
    let shots = repo().join(".agent-local/screenshot");
    let truth_file = repo().join(".agent-local/spikes/ground-truth.json");
    let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/tesseract");
    if !shots.is_dir() || !truth_file.is_file() || !bundle.join("tesseract.exe").is_file() {
        eprintln!("SKIPPED parser_fixtures: fixtures or OCR bundle missing");
        return;
    }
    let truth: Value = serde_json::from_str(&std::fs::read_to_string(truth_file).unwrap()).unwrap();
    let refs = References::build(&[], &[]);
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::in_dir(&bundle, tmp.path().to_path_buf(), Duration::from_secs(30));

    let mut tally = Tally::default();
    let mut resolutions = [0usize; 5]; // exact, stripped, lagrange, folded, unresolved
    let (mut titles, mut accepted_rewards, mut structures, mut screens, mut boxes) =
        (0, 0, 0, 0, 0);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&shots)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    files.sort();
    for png in &files {
        let name = png.file_name().unwrap().to_str().unwrap().to_string();
        let gt = &truth[&name];
        let ocr = recognize_file(&engine, png, &CancelToken::default()).unwrap();
        let parsed = parse_contract(&ocr, &refs);
        let short = &name[..13];
        eprintln!(
            "{short} [{:?}] title {:?} ({:?}) · reward {:?} ({:?}{}) · {} requirement(s) · problems {:?}",
            parsed.screen,
            parsed.title.value,
            parsed.title.flag,
            parsed.reward.value,
            parsed.reward.flag,
            parsed.reward.reason.as_deref().map(|r| format!(": {r}")).unwrap_or_default(),
            parsed.requirements.len(),
            parsed.problems
        );

        let title = gt["title"].as_str().unwrap().to_string();
        titles += usize::from(parsed.title.value.as_ref() == Some(&title));
        tally.field(format!("{short} title"), &parsed.title, Some(&title));
        // Screen state from the reward card (Task 10.1), never from the buttons.
        let screen = if gt["tab"] == "accepted" {
            ScreenState::Accepted
        } else {
            ScreenState::Offer
        };
        screens += usize::from(parsed.screen == screen);
        // Max box size: correct, or empty and flagged; none expected means empty.
        let want_box = gt["maxBox"].as_u64().map(|b| b as u32);
        let box_ok = parsed.max_box_scu.value == want_box
            || (parsed.max_box_scu.value.is_none() && parsed.max_box_scu.flag != Flag::Ok);
        boxes += usize::from(box_ok);
        eprintln!(
            "{short} screen {:?} · max box {:?} ({:?}), expected {want_box:?}",
            parsed.screen, parsed.max_box_scu.value, parsed.max_box_scu.flag
        );
        tally.field(
            format!("{short} max box"),
            &parsed.max_box_scu,
            want_box.as_ref(),
        );
        let reward = gt["reward"].as_u64().unwrap();
        if gt["tab"] == "accepted" {
            accepted_rewards += usize::from(parsed.reward.value == Some(reward));
        }
        tally.field(format!("{short} reward"), &parsed.reward, Some(&reward));

        let reqs = gt["requirements"].as_array().unwrap();
        let mut structure_ok =
            parsed.requirements.len() == reqs.len() && parsed.problems.is_empty();
        for (i, (r, p)) in reqs.iter().zip(&parsed.requirements).enumerate() {
            let what = |f: &str| format!("{short} req {i} {f}");
            let scu = r["scu"].as_u64().unwrap() as u32;
            tally.name(
                what("commodity"),
                &p.commodity,
                r["commodity"].as_str().unwrap(),
                &mut resolutions,
            );
            tally.field(what("total"), &p.total_scu, Some(&scu));
            let to = refs.locations.resolve(r["to"].as_str().unwrap(), true).0;
            structure_ok &= p.deliveries.len() == 1;
            for d in &p.deliveries {
                tally.name(what("delivery"), &d.location, &to, &mut resolutions);
                tally.field(what("delivery scu"), &d.scu, Some(&scu));
            }
            let from: Vec<String> = r["from"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| refs.locations.resolve(f.as_str().unwrap(), true).0)
                .collect();
            structure_ok &= p.pickups.len() == from.len();
            for (j, (want, got)) in from.iter().zip(&p.pickups).enumerate() {
                tally.name(
                    what(&format!("pickup {j}")),
                    &got.location,
                    want,
                    &mut resolutions,
                );
                let expected_scu = (from.len() == 1).then_some(scu);
                tally.field(
                    what(&format!("pickup {j} scu")),
                    &got.scu,
                    expected_scu.as_ref(),
                );
            }
            structure_ok &= p.commodity.value.as_deref() == r["commodity"].as_str()
                && p.total_scu.value == Some(scu)
                && p.deliveries
                    .iter()
                    .all(|d| d.location.value.as_deref() == Some(to.as_str()))
                && p.pickups
                    .iter()
                    .zip(&from)
                    .all(|(g, w)| g.location.value.as_deref() == Some(w.as_str()));
        }
        structures += usize::from(structure_ok);
    }

    eprintln!(
        "\ntitles {titles}/7 · accepted rewards {accepted_rewards}/3 · structures {structures}/7 · screens {screens}/7 · max box {boxes}/7\n\
         fields {}: correct {}, wrong+flagged {}, empty+flagged {}, WRONG UNFLAGGED {}\n\
         name resolution: exact {}, stripped {}, lagrange {}, folded {}, unresolved {}",
        tally.checked,
        tally.correct,
        tally.wrong_flagged,
        tally.empty_flagged,
        tally.wrong_unflagged.len(),
        resolutions[Resolution::Exact as usize],
        resolutions[Resolution::Stripped as usize],
        resolutions[Resolution::Lagrange as usize],
        resolutions[Resolution::Folded as usize],
        resolutions[Resolution::Unresolved as usize],
    );
    assert!(
        tally.wrong_unflagged.is_empty(),
        "wrong values filled without a flag: {:#?}",
        tally.wrong_unflagged
    );
    assert_eq!(titles, 7, "titles");
    assert_eq!(accepted_rewards, 3, "rewards on the Accepted screenshots");
    assert_eq!(structures, 7, "structure, SCU, commodities and locations");
    assert_eq!(screens, 7, "screen state from the reward card");
    assert_eq!(boxes, 7, "max box size correct or empty-and-flagged");
}
