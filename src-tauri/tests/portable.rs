//! Tests: where the data goes (override, portable, default), which kind of copy is
//! running, the OCR bundle lookup order and the portable folder checks. Pure path logic, plus a
//! temporary folder for the write check.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use cargo_line_lib::layout::{
    is_within, ocr_bundle_candidates, portable_problem_message, portable_webview_dir,
    prepare_portable_dir, resolve_data_dir, AppMode, DataDir, DataDirInputs, PortableProblem,
};

fn exe_dir() -> PathBuf {
    PathBuf::from(r"C:\Games\Cargo-Line-1.0.0-portable")
}

fn inputs(data_dir_env: Option<&str>, marker_exists: bool) -> DataDirInputs {
    DataDirInputs {
        debug_build: false,
        data_dir_env: data_dir_env.map(OsString::from),
        dev_data_dir_env: None,
        exe_dir: Some(exe_dir()),
        marker_exists,
    }
}

#[test]
fn the_override_wins_over_the_portable_marker() {
    let dir = resolve_data_dir(&inputs(Some(r"D:\cl-test"), true));
    assert_eq!(dir, DataDir::Override(PathBuf::from(r"D:\cl-test")));
}

#[test]
fn the_marker_gives_the_data_folder_next_to_the_exe() {
    let dir = resolve_data_dir(&inputs(None, true));
    assert_eq!(dir, DataDir::Portable(exe_dir().join("data")));
    assert_eq!(dir.path(), Some(exe_dir().join("data").as_path()));
}

#[test]
fn no_marker_gives_the_default_app_data_folder() {
    let dir = resolve_data_dir(&inputs(None, false));
    assert_eq!(dir, DataDir::Default);
    assert_eq!(dir.path(), None);
}

#[test]
fn a_relative_or_empty_override_is_ignored() {
    assert_eq!(
        resolve_data_dir(&inputs(Some(r"relative\data"), true)),
        DataDir::Portable(exe_dir().join("data"))
    );
    assert_eq!(resolve_data_dir(&inputs(Some(""), false)), DataDir::Default);
}

#[test]
fn the_dev_override_counts_in_debug_builds_only() {
    let mut i = inputs(None, false);
    i.dev_data_dir_env = Some(OsString::from(r"D:\dev-data"));
    assert_eq!(resolve_data_dir(&i), DataDir::Default, "release ignores it");
    i.debug_build = true;
    assert_eq!(
        resolve_data_dir(&i),
        DataDir::Override(PathBuf::from(r"D:\dev-data"))
    );
}

#[test]
fn no_exe_folder_never_means_portable() {
    let mut i = inputs(None, true);
    i.exe_dir = None;
    assert_eq!(resolve_data_dir(&i), DataDir::Default);
}

#[test]
fn the_mode_comes_from_the_build_and_the_marker() {
    assert_eq!(AppMode::resolve(false, false), AppMode::Installed);
    assert_eq!(AppMode::resolve(false, true), AppMode::Portable);
    assert_eq!(AppMode::resolve(true, false), AppMode::Dev);
    assert_eq!(AppMode::resolve(true, true), AppMode::Dev);
    for (mode, text) in [
        (AppMode::Installed, "installed"),
        (AppMode::Portable, "portable"),
        (AppMode::Dev, "dev"),
    ] {
        assert_eq!(mode.as_str(), text);
        assert_eq!(serde_json::to_string(&mode).unwrap(), format!("\"{text}\""));
    }
}

#[test]
fn the_webview_profile_moves_only_for_a_portable_copy_and_only_if_unset() {
    let portable = DataDir::Portable(exe_dir().join("data"));
    assert_eq!(
        portable_webview_dir(&portable, false),
        Some(exe_dir().join("data").join("webview"))
    );
    assert_eq!(portable_webview_dir(&portable, true), None);
    assert_eq!(portable_webview_dir(&DataDir::Default, false), None);
    assert_eq!(
        portable_webview_dir(&DataDir::Override(PathBuf::from(r"D:\x")), false),
        None
    );
}

#[test]
fn the_ocr_bundle_is_looked_up_next_to_the_exe_first() {
    let exe = Path::new(r"C:\Games\Cargo Line");
    let res = Path::new(r"C:\Games\Cargo Line\resources");
    assert_eq!(
        ocr_bundle_candidates(Some(exe), Some(res), "tesseract"),
        vec![exe.join("tesseract"), res.join("tesseract")]
    );
    // On Windows the resource folder is usually the exe folder: listed once.
    let same = Path::new(r"c:/games/cargo line/");
    assert_eq!(
        ocr_bundle_candidates(Some(exe), Some(same), "tesseract"),
        vec![exe.join("tesseract")]
    );
    assert_eq!(
        ocr_bundle_candidates(None, Some(res), "tesseract"),
        vec![res.join("tesseract")]
    );
    assert!(ocr_bundle_candidates(None, None, "tesseract").is_empty());
}

#[test]
fn a_copy_inside_the_temp_folder_is_detected() {
    let temp = Path::new(r"C:\Users\someone\AppData\Local\Temp");
    assert!(is_within(
        Path::new(r"C:\Users\someone\AppData\Local\Temp\Temp1_Cargo-Line.zip\Cargo-Line"),
        temp
    ));
    assert!(is_within(
        Path::new(r"c:\users\someone\appdata\local\temp\x"),
        temp
    ));
    assert!(!is_within(
        Path::new(r"C:\Users\someone\AppData\Local\Temporary"),
        temp
    ));
    assert!(!is_within(Path::new(r"D:\Games\Cargo-Line"), temp));
}

#[test]
fn the_portable_folder_is_created_and_checked() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    prepare_portable_dir(&data).unwrap();
    assert!(data.is_dir());
    assert_eq!(
        std::fs::read_dir(&data).unwrap().count(),
        0,
        "the probe is removed"
    );

    // A file where the folder should be: it can't be created, and nothing else is tried.
    let blocked = root.path().join("blocked");
    std::fs::write(&blocked, b"x").unwrap();
    let err = prepare_portable_dir(&blocked.join("data")).unwrap_err();
    assert!(matches!(err, PortableProblem::NotWritable(_)));
}

#[test]
fn the_messages_say_to_extract_the_zip_first() {
    for problem in [
        PortableProblem::NotWritable("access denied".into()),
        PortableProblem::InTempFolder,
    ] {
        let text = portable_problem_message(&problem);
        assert!(text.contains("Extract"), "{text}");
        assert!(text.contains("cargo-line.exe"), "{text}");
    }
    assert!(
        portable_problem_message(&PortableProblem::NotWritable("x".into()))
            .contains("delete portable.txt")
    );
}
