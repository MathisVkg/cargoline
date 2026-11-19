use std::path::Path;

/// Files produced by `scripts/prepare-tesseract.ps1` (git-ignored, bundled as resources).
const OCR_BUNDLE: [&str; 2] = [
    "resources/tesseract/tesseract.exe",
    "resources/tesseract/tessdata/eng.traineddata",
];

fn main() {
    for file in OCR_BUNDLE {
        println!("cargo:rerun-if-changed={file}");
    }
    let missing: Vec<&str> = OCR_BUNDLE
        .iter()
        .copied()
        .filter(|f| !Path::new(f).is_file())
        .collect();
    if !missing.is_empty() {
        let message = format!(
            "the bundled OCR engine is missing ({}). Run: powershell -ExecutionPolicy Bypass -File scripts/prepare-tesseract.ps1",
            missing.join(", ")
        );
        // Release builds ship the engine, so they must have it. Debug builds and tests still work
        // without it (text recognition then reports `engineMissing`).
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            panic!("{message}");
        }
        println!("cargo:warning={message}");
        // Tauri would also fail on the missing resource files: leave them out of this build.
        // SAFETY: build scripts are single-threaded; tauri_build reads this right below.
        unsafe { std::env::set_var("TAURI_CONFIG", r#"{"bundle":{"resources":[]}}"#) };
    }
    tauri_build::build()
}
