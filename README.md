# Cargo Line

Cargo Line is an offline-first hauling contract manager for Star Citizen, built as a Windows desktop app.
It needs no account and no server, and it does not require game log monitoring.

> Unofficial fan project. Not affiliated with or endorsed by Cloud Imperium Games.

## Current status

**Early development.** You can add contracts by hand. They are saved locally and shown on the Contracts
page, grouped by location or by contract. Everything else is still a placeholder.

| Area | Status |
| --- | --- |
| Native window (standard Windows controls, resizable) | Implemented |
| Top bar with ship selector | Placeholder, disabled. No ship catalog yet |
| Bottom navigation: Contracts, Cargo, History, Settings | Implemented |
| Contracts: manual Add Contract | Implemented |
| Contracts: By Location / By Contract views (choice is remembered) | Implemented |
| Contracts: searchable location and commodity fields (bundled lists + your own entries) | Implemented |
| Contracts: edit, complete and abandon active contracts (single or all) | Implemented |
| Contracts: optional Pick Up / Turn In reminder ticks (display only) | Implemented |
| Contracts: History page, OCR capture | Not implemented |
| Cargo summary and visualisation | Not implemented. Layout placeholder only |
| History, runs, statistics | Not implemented. Empty state only |
| Settings | Placeholder, controls disabled. Version is read from the app |
| SQLite database, schema migrations, persistent settings (Rust only) | Implemented. Not yet connected to the UI |
| OCR, screenshots, Game.log, ship catalog import, updater | Not started |

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

To build a release executable, run `npm run tauri build`. The executable is written to
`src-tauri/target/release/cargo-line.exe`. Installer bundling is turned off for now.

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

The database is `%APPDATA%\io.github.mathisvkg.sccargoapp\sc-cargo-app.sqlite3`. It is created on first launch
and is never stored in the repository.

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
