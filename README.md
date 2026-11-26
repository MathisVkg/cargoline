# Cargo Line

Cargo Line is an offline-first hauling contract manager for Star Citizen, built as a Windows desktop app.
It needs no account and no server, and it does not require game log monitoring.

> Unofficial fan project. Not affiliated with or endorsed by Cloud Imperium Games.

## Download and install

Get the latest version from the [Releases page](https://github.com/MathisVkg/cargoline/releases).
Each release has two downloads. Pick one:

| File | Choose it if |
|---|---|
| `Cargo-Line_<version>_x64-setup.exe` (**recommended**) | You want a normal install: a Start menu entry, no admin rights needed (it installs for your user only, in `%LOCALAPPDATA%\Cargo Line`). It gets the update notice, and later the in-app update. |
| `Cargo-Line_<version>_x64-portable.zip` | You don't want to install anything. Extract the zip and run `cargo-line.exe`. Everything stays in its own folder, so you can keep it on a USB stick. Updates are done by hand. |

**"Windows protected your PC".** Cargo Line isn't code-signed yet, so Windows SmartScreen doesn't know it and
shows this blue warning the first time. Click **More info**, then **Run anyway**.

**Checking the download (optional).** Each release has a `SHA256SUMS.txt`. In PowerShell, run
`Get-FileHash <file> -Algorithm SHA256` and compare the result with the line for that file (case doesn't matter).

**Requirements**

- Windows 10 or 11, x64.
- Microsoft Edge WebView2 Runtime. Windows 11 already has it. If it's missing, the installer downloads and
  installs it for you. For the portable version, get it from
  [Microsoft's WebView2 page](https://developer.microsoft.com/microsoft-edge/webview2/) ("Evergreen Bootstrapper").
- Star Citizen in **borderless or windowed** mode, 16:9 or wider (16:10 works too), for reading contracts from a
  screenshot.

**Where your data lives**

- **Installed:** your contracts, history, screenshots and settings are in
  `%APPDATA%\io.github.mathisvkg.sccargoapp`. The WebView2 profile and temporary OCR files are in
  `%LOCALAPPDATA%\io.github.mathisvkg.sccargoapp`. Uninstalling keeps your data, unless you tick
  **Delete the application data** in the uninstaller. That box deletes those two folders (and the installer's own
registry entries) and nothing else. A portable copy is never touched by the uninstaller.
- **Portable:** everything is in the `data\` folder next to `cargo-line.exe`. Nothing is written to `%APPDATA%`
  or `%LOCALAPPDATA%`. To move to another PC, close the app and copy the whole folder. To switch the portable
  copy to the normal app data folder, delete `portable.txt`.
- **Both:** Settings → Data → Export backup makes a single backup file that either version can restore.

Settings → About shows the version and which one you're running (`Installed` or `Portable`).

## Updates

- **What is checked:** the list of releases on this repository's GitHub Releases page. A beta build is offered
  newer betas and stable releases; a stable build only newer stable releases.
- **How often:** about 10 seconds after Cargo Line starts, at most once every 24 hours, plus whenever you click
  **Check now** in Settings → Updates. A failed automatic check is silent; it never slows the start.
- **Turning it off:** Settings → Updates → **Check for updates automatically**. Check now still works.
- **When there is a new version:** a quiet **Update available** line appears in the top bar (never a pop-up).
  **Skip this version** hides it until an even newer version comes out.
  - **Installed version:** **Download and install** downloads the installer and `SHA256SUMS.txt`, checks the
    installer's size and SHA-256, then asks before it closes Cargo Line, installs over the existing copy (no admin
    prompt, your shortcuts stay as they are) and opens it again. Your data folder isn't touched.
  - **Portable version** (and development builds): **Open download page**. Download the new zip and copy your
    `data` folder into it.
- **What the check protects against:** a damaged or incomplete download (the file is deleted and nothing is
  installed). It doesn't protect against a compromised GitHub account; code signing would cover that later.

**Network:** the update check is the only network access Cargo Line makes. It contacts GitHub only
(`api.github.com`, `github.com` release downloads and GitHub's download servers), sends nothing but standard
request headers (`User-Agent: Cargo-Line/<version>`, no account, no token, no data of yours), and can be turned off.

## Current status

**Beta.** What the app does:

| Area | What it does |
| --- | --- |
| Contracts | Add by hand or read from a screenshot (built-in OCR). Grouped by location or by contract. Edit, complete or abandon contracts. Pick Up / Turn In ticks. |
| Game.log | Optional. Detects the log, notices contracts accepted and completed in the game. |
| Ships | Catalog from the Star Citizen Wiki, favourites, custom ships, selector in the top bar. |
| Cargo | Pickup and destination summary, isometric box view. |
| History | Runs, earnings, aUEC per hour. Restore or clear. |
| Settings | Capture, Game.log, data folder, backup and restore, About. |
| Updates | Checks GitHub for a newer version. The installed version updates itself in one click. |

See [`CHANGELOG.md`](CHANGELOG.md) for each version's changes.

## Prerequisites (Windows x64)

- [Rust](https://rustup.rs/) stable with the `x86_64-pc-windows-msvc` toolchain
- Microsoft C++ Build Tools ("Desktop development with C++" / MSVC + Windows SDK)
- Microsoft Edge WebView2 Runtime (preinstalled on Windows 10/11)
- Node.js 20+ and npm

See the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/) for details.

## Development

```bash
npm install
npm run tauri dev      # launches the desktop app with hot reload
```

Headless checks (these open no windows). CI runs the same commands on every push and pull request
to `main` (`.github/workflows/ci.yml`, Windows):

```bash
npm ci
npm run typecheck                                              # TypeScript
npm run build                                                  # frontend -> dist/ (needed before cargo)
npm run test:frontend                                          # frontend tests (node:test)
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml                # Rust tests
npm test                                                       # both test suites
```

Node is pinned in `.nvmrc` (Node 24: the frontend tests use its built-in TypeScript support).

To build a release (installer, portable zip and checksums), run the same script the release workflow uses:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1              # with all the CI checks
powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1 -SkipChecks  # quick rebuild
```

The output goes to `release-out/` (git-ignored). For the exe alone, run `npm run tauri build -- --no-bundle`
(output: `src-tauri/target/release/cargo-line.exe`). Making a release is described in
[`docs/RELEASING.md`](docs/RELEASING.md).

### Measuring startup

Two environment variables, both unset by default, help measure a release build without touching your data:

- **`CARGO_LINE_DATA_DIR=<absolute folder>`:** the database, screenshots and OCR temp files go to that folder
  instead of the app data folders. It works in any build; a relative path is ignored.
  (`CARGO_LINE_DEV_DATA_DIR` does the same in debug builds only.)
- **`CARGO_LINE_STARTUP_TRACE=1`:** writes timing marks, one per line, to stderr and to `startup-trace.log` in
  the data folder. Offsets are in ms from the process start, and the file is replaced at each launch. Marks are
  names and counts only, never paths or contract data. When the variable is unset, nothing is recorded.

```powershell
cargo run --release --manifest-path src-tauri/Cargo.toml --example perf_seed -- C:\temp\cl-perf    # synthetic data
cargo run --release --manifest-path src-tauri/Cargo.toml --example perf_bench -- C:\temp\cl-perf   # backend steps, no window
npm run tauri build -- --no-bundle
$env:CARGO_LINE_DATA_DIR = "C:\temp\cl-perf"; $env:CARGO_LINE_STARTUP_TRACE = "1"
$env:WEBVIEW2_USER_DATA_FOLDER = "C:\temp\cl-perf-webview"   # optional: a separate WebView2 profile
src-tauri\target\release\cargo-line.exe
Get-Content C:\temp\cl-perf\startup-trace.log
```

## Architecture

```
src/            React + TypeScript UI (presentation only)
  components/   Shell: top bar, ship selector, bottom navigation, UI primitives
  pages/        Contracts, Cargo, History, Settings pages
  features/contracts/  Add Contract modal and form, By Location / By Contract views, typed IPC client,
                       searchable location picker, pure grouping/search functions and client-side checks
src-tauri/      Rust backend (Tauri 2)
  src/lib.rs    Application builder, starts database initialization
  src/main.rs   Minimal entry point
  src/database/ Connection setup, migrations (SQL files), typed enum values
  src/settings/ Typed, validated persistent settings
  src/contracts/ Contract validation (authoritative) and transactional persistence
  src/commands.rs Tauri IPC commands: create_contract, list_active_contracts
  tests/        Headless Rust tests
tests/frontend/ Frontend unit tests (node:test)
tools/catalog/  Generators for the bundled catalogs (see tools/catalog/README.md)
```

- **Rust owns** business logic, local file IO, screenshot capture, OCR orchestration and SQLite. Only SQLite
  exists so far; the rest arrives in later tasks.
- **The frontend** only renders the UI and calls validated Rust commands. It never runs SQL directly.
- Stack: Tauri 2, React 19, TypeScript, Vite, Tailwind CSS 4, Lucide icons.

### Layout rules

- The top bar and the bottom navigation stay fixed. The main content area scrolls between them and is never
  covered by either bar.
- The Cargo page's two panels sit side by side when the app is **990 px or wider** and stack only below
  that (a width rule, never orientation), so a portrait window that is at least 990 px wide still shows two
  columns. The app does not set a minimum window size.

## Local data

The database is `%APPDATA%\io.github.mathisvkg.sccargoapp\sc-cargo-app.sqlite3` (portable version:
`data\sc-cargo-app.sqlite3` next to the exe). It is created on first launch and is never stored in the repository.
The data folder is decided once at startup, in this order: `CARGO_LINE_DATA_DIR`, then `portable.txt` next to
the exe (→ `data\`), then `%APPDATA%` (`src-tauri/src/layout.rs`).

- **Engine:** SQLite, bundled through `rusqlite`. Foreign keys are enforced, the database uses WAL journaling,
  and a connection waits up to 5 s for a lock before failing.
- **Startup:** initialization runs on a background thread, so it never blocks the UI thread.
- **Migrations:** forward-only, managed by `rusqlite_migration`. SQLite's `user_version` field records the
  schema version, and all pending migrations are applied in a single transaction. Never edit an applied file
  in `src-tauri/src/database/migrations/`; add a new numbered one instead.
- **No silent resets.** Cargo Line marks its databases with a SQLite `application_id`. It refuses to open, and
  never modifies, a non-empty file that lacks that mark, or a database created by a newer app version. The
  error message explains why.
- **Units:** timestamps are UTC milliseconds, run dates are local `YYYY-MM-DD`, and SCU quantities are integer
  thousandths of an SCU (milli-SCU).
- **Deletion:** active contracts cannot be deleted. Deleting an archived contract also deletes its cargo rows,
  but screenshot metadata is kept, detached from the contract, so the image file can still be cleaned up.

## Tests

Automated tests live in `tests/frontend/` (Node's built-in `node:test`, no extra dependencies) and
`src-tauri/tests/` (Rust); `npm test` runs both suites. Tests only ever use temporary databases and
temporary folders.

The whole suite passes on a clean clone. A few tests need local material that is never committed;
they **skip with a message** when it is missing:

| Test | Needs |
|---|---|
| `src-tauri/tests/ocr_fixtures.rs` | screenshots and `ground-truth.json` in `.agent-local/`, the OCR bundle, Node |
| `src-tauri/tests/parser_fixtures.rs` | the same screenshots and ground truth, the OCR bundle |
| `gamelog.rs` · `real_game_log_fixture` | a real `Game.log` in `.agent-local/screenshot/` |
| `gamelog.rs` · `detect_on_this_machine` | an installed RSI Launcher (its log) |
| `tests/frontend/catalog-tools.test.ts` · reproducibility | the API cache in `tools/catalog/.cache/` |

The OCR bundle is built by `scripts/prepare-tesseract.ps1`; debug builds and tests work without it.

### What is tracked

- **Tracked:** the source, the automated tests, the catalog generators (`tools/catalog/`) and the
  generated catalogs (`src-tauri/resources/catalog/`).
- **Never tracked:** `.agent-local/` (fixtures, screenshots, ground truth, desktop-session data), task
  files and reports, caches (`tools/catalog/.cache/`), build output, databases, logs, and anything with
  personal data. Test data in the repository is synthetic.

## Location catalog and bundled suggestion lists

The Add Contract location and commodity fields are "type or pick" fields: search a list, or type any
value. Free text is always accepted, and validation and saving are unchanged.

- **User location catalog (`locations` table, migration 0002).** Location names typed in saved contracts.
  Names are compared ignoring case and extra spaces, so "port  tressler" reuses "Port Tressler". New names
  are added in the same transaction as the contract, so cancelling or a failed save leaves nothing behind.
- **Bundled reference lists:** `src-tauri/resources/catalog/locations.json` and `commodities.json`. They
  are static data, embedded at compile time, served by the Rust commands `list_reference_locations` and
  `list_reference_commodities`, and never written to the database. They list the locations named in
  hauling-mission text and the commodities used in hauling orders, for one pinned game build. Location
  names keep the in-game mission spelling (e.g. "Covalex Distribution Center S4DC05").
- **Dropdown merge:** bundled entries come first and their spelling wins in the dropdown. Then come the
  user's catalog (locations) or the commodities already used in saved contracts, then names typed in the
  current form. Duplicates are removed ignoring case and spacing.
- **Search order:** names that start with the typed text, then names where a word starts with it, then
  names that contain it. At most 50 options are shown.

### Data source and attribution

Data from the [Star Citizen Wiki API](https://api.star-citizen.wiki) (api.star-citizen.wiki), maintained
by the Star Citizen Wiki community. Unofficial; not affiliated with Cloud Imperium Games. The location,
commodity and ship lists are generated by the scripts in [`tools/catalog/`](tools/catalog/README.md),
which document every inclusion and exclusion rule and how to regenerate the lists after a game patch.

## Licence

Cargo Line is free software under the GNU GPL v3.0 (`GPL-3.0-only`); see [`LICENSE`](LICENSE).

- **Bundled third-party components** keep their own licences; see
  [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md). The OCR engine, Tesseract, is under the Apache
  License 2.0, which is compatible with GPL-3.0.
- **Star Citizen Wiki data** (the bundled location, commodity and ship lists) is used under its own terms:
  non-commercial use, with attribution.
- Cargo Line is an unofficial fan project. It is not affiliated with or endorsed by Cloud Imperium Games.
  Star Citizen® is a registered trademark of Cloud Imperium Rights LLC.
