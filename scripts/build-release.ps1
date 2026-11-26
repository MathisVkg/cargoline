<#
.SYNOPSIS
  Builds a Cargo Line release: the NSIS installer, the portable zip and SHA256SUMS.txt (Task 19).

.DESCRIPTION
  The same script runs on a developer PC and in .github/workflows/release.yml.

    1. Checks the tools (Node, Rust with the MSVC toolchain) and the OCR bundle; builds the bundle
       with prepare-tesseract.ps1 when it is missing.
    2. Unless -SkipChecks: npm ci, typecheck, frontend build and tests, cargo fmt --check,
       clippy -D warnings, cargo test (the same set as CI).
    3. npm run tauri build -- --bundles nsis (release profile from src-tauri/Cargo.toml).
    4. The portable zip: one folder Cargo-Line-<version>-portable/ with the same exe and OCR bundle.
    5. release-out/ (emptied first): the setup exe, the portable zip, SHA256SUMS.txt and
       release-notes.md (the CHANGELOG.md section of this version).
    6. A summary: version, files, sizes, SHA-256, total time.

  Needs no admin rights, reads no personal data and never touches the app's data folders.

.PARAMETER SkipChecks
  Skip step 2 (for a quick local rebuild).

.PARAMETER Tag
  A release tag such as v1.0.0-beta.1. The script fails unless it is "v" + the version in the files.
#>
param(
  [switch]$SkipChecks,
  [string]$Tag = ""
)

$ErrorActionPreference = "Stop"
$started = Get-Date
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Out = Join-Path $Repo "release-out"
Set-Location $Repo

function Step($text) { Write-Host "==> $text" -ForegroundColor Cyan }
function Fail($text) { Write-Host "ERROR: $text" -ForegroundColor Red; exit 1 }
function Run($what, [scriptblock]$cmd) {
  Step $what
  & $cmd
  if ($LASTEXITCODE -ne 0) { Fail "$what failed (exit code $LASTEXITCODE)." }
}

# ---- 1. Tools and version --------------------------------------------------------------------
Step "Checking the tools"
if (-not (Get-Command node -ErrorAction SilentlyContinue)) { Fail "Node.js is required (version in .nvmrc)." }
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) { Fail "npm is required." }
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Fail "Rust is required (https://rustup.rs, x86_64-pc-windows-msvc)." }
$wantNode = (Get-Content (Join-Path $Repo ".nvmrc") -Raw).Trim()
$haveNode = (node --version).Trim().TrimStart("v")
if ($haveNode.Split(".")[0] -ne $wantNode.Split(".")[0]) { Fail "Node $wantNode is required (.nvmrc); found $haveNode." }
$rustHost = (rustc -vV | Select-String "^host: ").ToString().Substring(6).Trim()
if ($rustHost -ne "x86_64-pc-windows-msvc") { Fail "Rust must use the x86_64-pc-windows-msvc toolchain; found $rustHost." }
Write-Host "    Node $haveNode, $((rustc --version).Trim()) ($rustHost)"

$versions = node -e "import('./scripts/set-version.mjs').then(m => console.log(JSON.stringify(m.readVersions('.'))))" | ConvertFrom-Json
$Version = $versions.packageJson
$mismatch = @()
foreach ($k in "packageLock", "packageLockRoot", "cargoToml", "cargoLock") { if ($versions.$k -ne $Version) { $mismatch += "$k=$($versions.$k)" } }
if ($versions.tauriConf -ne "../package.json" -and $versions.tauriConf -ne $Version) { $mismatch += "tauriConf=$($versions.tauriConf)" }
if ($mismatch) { Fail "The version files disagree with package.json ($Version): $($mismatch -join ', '). Run: node scripts/set-version.mjs $Version" }
if ($Tag -and $Tag -ne "v$Version") { Fail "The tag $Tag doesn't match the version in the files (v$Version)." }
Write-Host "    Version $Version$(if ($Tag) { ", tag $Tag" })"

$Bundle = Join-Path $Repo "src-tauri\resources\tesseract"
$bundleFiles = @("tesseract.exe", "tessdata\eng.traineddata")
if ($bundleFiles | Where-Object { -not (Test-Path (Join-Path $Bundle $_)) }) {
  Step "The OCR bundle is missing: building it (prepare-tesseract.ps1, about 5-10 minutes)"
  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Repo "scripts\prepare-tesseract.ps1")
  if ($LASTEXITCODE -ne 0) { Fail "prepare-tesseract.ps1 failed." }
}
foreach ($f in $bundleFiles) { if (-not (Test-Path (Join-Path $Bundle $f))) { Fail "OCR bundle file missing: src-tauri\resources\tesseract\$f" } }
$manifest = Get-Content (Join-Path $Bundle "manifest.json") -Raw | ConvertFrom-Json
$exeHash = (Get-FileHash (Join-Path $Bundle "tesseract.exe") -Algorithm SHA256).Hash
if ($manifest.tesseract_sha256 -and $exeHash -ne $manifest.tesseract_sha256) { Fail "tesseract.exe doesn't match its manifest.json hash. Rebuild it with prepare-tesseract.ps1." }
Write-Host "    OCR bundle: tesseract $($manifest.tesseract)"

# ---- 2. Checks (the CI set) ------------------------------------------------------------------
if ($SkipChecks) {
  Step "Checks skipped (-SkipChecks)"
} else {
  Run "npm ci" { npm ci }
  Run "Typecheck" { npm run typecheck }
  Run "Frontend build" { npm run build }
  Run "Frontend tests" { npm run test:frontend }
  Run "cargo fmt --check" { cargo fmt --manifest-path src-tauri/Cargo.toml --check }
  Run "cargo clippy" { cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings }
  Run "cargo test" { cargo test --manifest-path src-tauri/Cargo.toml }
}

# ---- 3. Release build and installer ----------------------------------------------------------
$Target = Join-Path $Repo "src-tauri\target\release"
$NsisDir = Join-Path $Target "bundle\nsis"
if (Test-Path $NsisDir) { Remove-Item $NsisDir -Recurse -Force }
$buildStarted = Get-Date
Run "Release build and NSIS installer" { npm run tauri build -- --bundles nsis }
$buildSeconds = [int]((Get-Date) - $buildStarted).TotalSeconds
$setup = @(Get-ChildItem $NsisDir -Filter "*_x64-setup.exe")
if ($setup.Count -ne 1) { Fail "Expected one *_x64-setup.exe in $NsisDir, found $($setup.Count)." }
$Exe = Join-Path $Target "cargo-line.exe"
if (-not (Test-Path $Exe)) { Fail "The release exe is missing: $Exe" }

# ---- 4. Portable zip, 5. release-out ---------------------------------------------------------
Step "Collecting release-out\"
if (Test-Path $Out) { Remove-Item $Out -Recurse -Force }
New-Item -ItemType Directory $Out | Out-Null
$SetupName = "Cargo-Line_${Version}_x64-setup.exe"
$ZipName = "Cargo-Line_${Version}_x64-portable.zip"
Copy-Item $setup[0].FullName (Join-Path $Out $SetupName)

$staging = Join-Path $Out ".portable"
$folder = Join-Path $staging "Cargo-Line-$Version-portable"
New-Item -ItemType Directory (Join-Path $folder "tesseract\tessdata") -Force | Out-Null
Copy-Item $Exe (Join-Path $folder "cargo-line.exe")
Copy-Item (Join-Path $Bundle "tesseract.exe") (Join-Path $folder "tesseract\tesseract.exe")
Copy-Item (Join-Path $Bundle "tessdata\eng.traineddata") (Join-Path $folder "tesseract\tessdata\eng.traineddata")
Copy-Item (Join-Path $Repo "scripts\portable\portable.txt") $folder
Copy-Item (Join-Path $Repo "scripts\portable\README-portable.txt") $folder
Copy-Item (Join-Path $Repo "LICENSE") $folder
Copy-Item (Join-Path $Repo "THIRD_PARTY_NOTICES.md") $folder
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
# One top-level folder. Entries are written one by one so their names always use "/" (the zip
# standard); ZipFile.CreateFromDirectory writes "\" under Windows PowerShell 5.1.
$zip = [IO.Compression.ZipFile]::Open((Join-Path $Out $ZipName), [IO.Compression.ZipArchiveMode]::Create)
try {
  foreach ($file in Get-ChildItem $folder -Recurse -File | Sort-Object FullName) {
    $entry = $file.FullName.Substring($staging.Length + 1).Replace("\", "/")
    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $file.FullName, $entry, [IO.Compression.CompressionLevel]::Optimal)
  }
} finally {
  $zip.Dispose()
}
Remove-Item $staging -Recurse -Force

node scripts/release-notes.mjs $Version (Join-Path $Out "release-notes.md")
if ($LASTEXITCODE -ne 0) { Fail "No release notes for $Version in CHANGELOG.md." }

$sums = foreach ($name in $SetupName, $ZipName) {
  "{0}  {1}" -f (Get-FileHash (Join-Path $Out $name) -Algorithm SHA256).Hash.ToLowerInvariant(), $name
}
# LF line endings, no BOM: the usual sha256sum format, readable by any tool.
[IO.File]::WriteAllText((Join-Path $Out "SHA256SUMS.txt"), (($sums -join "`n") + "`n"))

# ---- 6. Summary ------------------------------------------------------------------------------
$total = [int]((Get-Date) - $started).TotalSeconds
Write-Host ""
Write-Host "Cargo Line $Version" -ForegroundColor Green
foreach ($name in $SetupName, $ZipName, "SHA256SUMS.txt", "release-notes.md") {
  $item = Get-Item (Join-Path $Out $name)
  Write-Host ("  {0,-44} {1,12:N0} bytes" -f $name, $item.Length)
}
Write-Host ""
$sums | ForEach-Object { Write-Host "  $_" }
Write-Host ""
Write-Host "  tauri build: $buildSeconds s, total: $total s"
Write-Host "  Output: $Out"
