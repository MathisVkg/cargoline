# Tesseract OCR bundle (generated, not committed)

This folder holds the OCR engine bundled with Cargo Line:

- `tesseract.exe` (Tesseract 5.5.3 / Leptonica 1.87.0, static, built from source with vcpkg)
- `tessdata/eng.traineddata` (tesseract-ocr/tessdata_fast, pinned commit)
- `manifest.json` (versions and SHA-256 hashes)

Generate it with:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/prepare-tesseract.ps1
```

Only this README is tracked. Without the bundle the app still works (manual entry); text
recognition reports that the OCR engine is missing, and release builds refuse to build.
Licences: see `THIRD_PARTY_NOTICES.md` at the repository root.
