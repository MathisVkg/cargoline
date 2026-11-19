<#
.SYNOPSIS
  Builds the pinned Tesseract OCR engine and places it in src-tauri/resources/tesseract/.

.DESCRIPTION
  Cargo Line bundles tesseract.exe and English language data as app resources. They are not
  committed: run this script once per machine (about 5-10 minutes) before `npm run tauri build`.

  - Needs: Git, and Visual Studio 2022+ (or Build Tools) with the "Desktop development with C++"
    workload. No admin rights, no system-wide install, no PATH changes.
  - vcpkg is cloned at a pinned commit; its own tool is bootstrapped and checked by SHA-256.
  - Tesseract 5.5.3 / Leptonica 1.87.0 are built from source with vcpkg (static, release-only),
    using the overlay port in scripts/tesseract (curl and libarchive disabled). vcpkg verifies every
    source archive by SHA-512.
  - eng.traineddata comes from tesseract-ocr/tessdata_fast at a pinned commit, checked by SHA-256.
  - Everything is built under .tesseract-build/ (git-ignored). Binary caching is off, so nothing
    is written to %LOCALAPPDATA%\vcpkg.

.PARAMETER WorkDir
  Build folder (default: <repo>/.tesseract-build).
#>
param(
  [string]$WorkDir = (Join-Path $PSScriptRoot "..\.tesseract-build")
)

$ErrorActionPreference = "Stop"

# ---- Pins -------------------------------------------------------------------------------------
$VcpkgRepo       = "https://github.com/microsoft/vcpkg"
$VcpkgTag        = "2026.10.16"
$VcpkgCommit     = "9eec002df1cd109f8f104cd966a763daf9751489"
$VcpkgToolSha256 = "2A4AE1467B9E797964C60C919D057B9E5DC1EC03F4B9632C1150026DB25201F2"
$TessdataCommit  = "87416418657359cb625c412a48b6e1d6d41c29bd"   # tesseract-ocr/tessdata_fast
$TessdataSha256  = "7D4322BD2A7749724879683FC3912CB542F19906C83BCC1A52132556427170B2"
$ExpectTesseract = "tesseract 5.5.3"
$ExpectLeptonica = "leptonica-1.87.0"
$Triplet         = "x64-windows-static-release"

$Repo   = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Target = Join-Path $Repo "src-tauri\resources\tesseract"
New-Item -ItemType Directory -Force $WorkDir | Out-Null
$WorkDir = (Resolve-Path $WorkDir).Path
$Vcpkg  = Join-Path $WorkDir "vcpkg"

function Step($text) { Write-Host "==> $text" -ForegroundColor Cyan }
function Fail($text) { Write-Host "ERROR: $text" -ForegroundColor Red; exit 1 }
function Sha256($path) { (Get-FileHash $path -Algorithm SHA256).Hash.ToUpperInvariant() }

# ---- 1. vcpkg at the pinned commit ---------------------------------------------------------------
if (-not (Get-Command git -ErrorAction SilentlyContinue)) { Fail "Git is required." }
if (-not (Test-Path (Join-Path $Vcpkg ".git"))) {
  Step "Cloning vcpkg $VcpkgTag"
  git clone --quiet --depth 1 --branch $VcpkgTag $VcpkgRepo $Vcpkg
  if ($LASTEXITCODE -ne 0) { Fail "git clone of vcpkg failed." }
}
$head = (git -C $Vcpkg rev-parse HEAD).Trim()
if ($head -ne $VcpkgCommit) { Fail "vcpkg is at $head, expected $VcpkgCommit. Delete $Vcpkg and retry." }

$vcpkgExe = Join-Path $Vcpkg "vcpkg.exe"
if (-not (Test-Path $vcpkgExe)) {
  Step "Bootstrapping the vcpkg tool"
  & cmd /c "`"$(Join-Path $Vcpkg 'bootstrap-vcpkg.bat')`" -disableMetrics"
  if ($LASTEXITCODE -ne 0) { Fail "bootstrap-vcpkg failed." }
}
if ((Sha256 $vcpkgExe) -ne $VcpkgToolSha256) { Fail "vcpkg.exe hash mismatch: $(Sha256 $vcpkgExe)" }

# ---- 2. Build Tesseract (overlay port, static, release only) -------------------------------------
Step "Building Tesseract with vcpkg ($Triplet). This takes several minutes the first time."
$env:VCPKG_BINARY_SOURCES = "clear"
$env:VCPKG_DISABLE_METRICS = "1"
& $vcpkgExe install "tesseract:$Triplet" `
  --overlay-ports  (Join-Path $PSScriptRoot "tesseract\overlay-ports") `
  --overlay-triplets (Join-Path $PSScriptRoot "tesseract\triplets") `
  --downloads-root   (Join-Path $WorkDir "downloads") `
  --x-buildtrees-root (Join-Path $WorkDir "bt") `
  --x-packages-root  (Join-Path $WorkDir "pk") `
  --x-install-root   (Join-Path $WorkDir "installed") `
  --clean-after-build
if ($LASTEXITCODE -ne 0) { Fail "vcpkg install failed (is the Visual Studio C++ workload installed?)." }

$installed = Join-Path $WorkDir "installed\$Triplet"
$exe = Join-Path $installed "tools\tesseract\tesseract.exe"
if (-not (Test-Path $exe)) { Fail "tesseract.exe was not produced at $exe." }
$version = (& $exe --version 2>&1 | Out-String)
if ($version -notmatch [regex]::Escape($ExpectTesseract) -or $version -notmatch [regex]::Escape($ExpectLeptonica)) {
  Fail "Unexpected version output:`n$version"
}
if ($version -match "libcurl|libarchive") { Fail "The build still contains curl or libarchive." }

# ---- 3. English language data (tessdata_fast, pinned) -------------------------------------------
$traineddata = Join-Path $WorkDir "eng.traineddata"
if (-not (Test-Path $traineddata) -or (Sha256 $traineddata) -ne $TessdataSha256) {
  Step "Downloading eng.traineddata (tessdata_fast @ $TessdataCommit)"
  Invoke-WebRequest -UseBasicParsing -OutFile $traineddata `
    "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/$TessdataCommit/eng.traineddata"
}
if ((Sha256 $traineddata) -ne $TessdataSha256) { Fail "eng.traineddata hash mismatch: $(Sha256 $traineddata)" }

# ---- 4. Place the files --------------------------------------------------------------------------
Step "Copying into $Target"
New-Item -ItemType Directory -Force (Join-Path $Target "tessdata") | Out-Null
Copy-Item $exe (Join-Path $Target "tesseract.exe") -Force
Copy-Item $traineddata (Join-Path $Target "tessdata\eng.traineddata") -Force

$manifest = [ordered]@{
  tesseract        = "5.5.3"
  leptonica        = "1.87.0"
  vcpkg            = "$VcpkgTag ($VcpkgCommit)"
  triplet          = $Triplet
  tessdata         = "tessdata_fast @ $TessdataCommit"
  tesseract_sha256 = Sha256 (Join-Path $Target "tesseract.exe")
  eng_traineddata_sha256 = Sha256 (Join-Path $Target "tessdata\eng.traineddata")
  version_output   = ($version.Trim() -split "`r?`n")
}
$manifest | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $Target "manifest.json")

Step "Done."
Write-Host ($version.Trim())
Write-Host "tesseract.exe SHA-256: $($manifest.tesseract_sha256) (MSVC builds are not bit-reproducible; this is recorded, not pinned)"
