//! Local fixture regression (fixtures stay in .agent-local/, not in the repository): runs the app's OCR pipeline with the real bundled
//! tesseract.exe on the 7 Task 06 screenshots and checks the Task 08 pass bar with the Task 06
//! scorers (`.agent-local/spikes/score.mjs`, `score-locations.mjs`).
//!
//! Skipped, with a message, when the fixtures, the bundle or Node.js are missing.
//! Timing is only meaningful in release: `cargo test --release --test ocr_fixtures -- --nocapture`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use cargo_line_lib::ocr::engine::{CancelToken, Engine};
use cargo_line_lib::ocr::{recognize_file, RegionKind, ScreenState};
use serde_json::{json, Value};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn skip(reason: &str) {
    eprintln!("SKIPPED ocr_fixtures: {reason}");
}

/// Last "x/y" pair of a scorer row for the `app` variant.
fn row<'a>(output: &'a str, prefix: &str) -> Vec<&'a str> {
    output
        .lines()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no `{prefix}` row in:\n{output}"))
        .split_whitespace()
        .collect()
}

#[test]
fn bundled_tesseract_meets_the_task_08_pass_bar_on_the_fixtures() {
    let shots = repo().join(".agent-local/screenshot");
    let spikes = repo().join(".agent-local/spikes");
    let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/tesseract");
    if !shots.is_dir() || !spikes.join("ground-truth.json").is_file() {
        return skip("fixtures not found in .agent-local/");
    }
    if !bundle.join("tesseract.exe").is_file() {
        return skip("bundle missing (run scripts/prepare-tesseract.ps1)");
    }
    if Command::new("node").arg("--version").output().is_err() {
        return skip("Node.js not found (needed for the Task 06 scorers)");
    }
    let truth: Value =
        serde_json::from_str(&std::fs::read_to_string(spikes.join("ground-truth.json")).unwrap())
            .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::in_dir(&bundle, tmp.path().join("ocr-tmp"), Duration::from_secs(30));
    eprintln!("engine: {}", engine.version().unwrap());
    let out_dir = tmp.path().join("scored");
    std::fs::create_dir_all(&out_dir).unwrap();

    let mut accepted_rewards = (0, 0);
    let mut slowest = 0;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&shots)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 7, "expected the 7 Task 06 screenshots");
    for png in &files {
        let name = png.file_name().unwrap().to_str().unwrap();
        let gt = &truth[name];
        let result = recognize_file(&engine, png, &CancelToken::default()).unwrap();
        let t = result.timings;
        slowest = slowest.max(t.total_ms);
        eprintln!(
            "{name}: anchors {:?}, screen {:?}, load {} + anchor {} + regions {} = {} ms",
            result.anchors, result.screen, t.load_ms, t.anchor_ms, t.regions_ms, t.total_ms
        );
        assert!(
            result.anchors.header && result.anchors.reward,
            "{name}: anchors"
        );

        // Button: the app's own classification.
        let expected = if gt["button"] == "ABANDON" {
            ScreenState::Accepted
        } else {
            ScreenState::Offer
        };
        assert_eq!(result.screen, expected, "{name}: button");

        // Reward on the Accepted tab (Offers misses are the in-game cursor, reported separately).
        let region = |kind| result.regions.iter().find(|r| r.kind == kind).unwrap();
        let reward_text: String = region(RegionKind::Reward)
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let want = gt["reward"].to_string();
        let digits_match = reward_text
            .split_whitespace()
            .any(|w| w.chars().filter(char::is_ascii_digit).collect::<String>() == want);
        if gt["tab"] == "accepted" {
            accepted_rewards.1 += 1;
            accepted_rewards.0 += usize::from(digits_match);
        } else {
            eprintln!(
                "  offers reward read as {reward_text:?} (want {}) -> {}",
                gt["reward"],
                if digits_match { "ok" } else { "miss" }
            );
        }

        // Title, reward and objectives text in the scorers' format, plus the button line.
        let mut lines: Vec<Value> = [
            RegionKind::Title,
            RegionKind::Reward,
            RegionKind::Objectives,
        ]
        .iter()
        .flat_map(|k| {
            region(*k)
                .lines
                .iter()
                .map(|l| json!({ "text": l.text, "conf": l.confidence }))
        })
        .collect();
        lines.push(json!({ "text": if expected == ScreenState::Accepted { "ABANDON" } else { "ACCEPT OFFER" } }));
        let scored = json!({ "file": name, "variants": { "app": { "lines": lines, "ocr_ms": t.total_ms } } });
        std::fs::write(
            out_dir.join(name.replace(".png", ".json")),
            scored.to_string(),
        )
        .unwrap();
    }

    let run = |script: &str| {
        let out = Command::new("node")
            .current_dir(&spikes)
            .arg(script)
            .arg(&out_dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{script}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let scores = run("score.mjs");
    let locations = run("score-locations.mjs");
    eprintln!("{scores}\n{locations}");
    let s = row(&scores, "app ");
    assert_eq!(
        &s[1..6],
        ["7/7", s[2], "7/7", "56/56", "7/7"],
        "title, button, objective fields, structure"
    );
    assert_eq!(
        row(&locations, "app ")[1],
        "29/29",
        "locations after catalog resolution"
    );
    assert_eq!(
        accepted_rewards,
        (3, 3),
        "reward on the Accepted screenshots"
    );
    if cfg!(debug_assertions) {
        eprintln!("slowest capture {slowest} ms (debug build: not checked)");
    } else {
        assert!(slowest <= 3_000, "slowest capture took {slowest} ms");
    }
}

/// Optional extra frame (for example a live capture kept locally):
/// `OCR_EXTRA_PNG=<path> OCR_EXTRA_BUTTON=accepted|offer cargo test --test ocr_fixtures -- --nocapture`
#[test]
fn optional_extra_frame() {
    let (Ok(png), Ok(button)) = (
        std::env::var("OCR_EXTRA_PNG"),
        std::env::var("OCR_EXTRA_BUTTON"),
    ) else {
        return skip("OCR_EXTRA_PNG / OCR_EXTRA_BUTTON not set");
    };
    let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/tesseract");
    if !bundle.join("tesseract.exe").is_file() {
        return skip("bundle missing");
    }
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::in_dir(&bundle, tmp.path().to_path_buf(), Duration::from_secs(30));
    let result = recognize_file(&engine, Path::new(&png), &CancelToken::default()).unwrap();
    eprintln!(
        "anchors {:?}, button {:?}, {} ms",
        result.anchors, result.screen, result.timings.total_ms
    );
    for region in &result.regions {
        for line in &region.lines {
            eprintln!("  {:?} {:>3.0} {}", region.kind, line.confidence, line.text);
        }
    }
    let expected = if button == "accepted" {
        ScreenState::Accepted
    } else {
        ScreenState::Offer
    };
    assert_eq!(result.screen, expected);
    assert!(result.anchors.header && result.anchors.reward);
}
