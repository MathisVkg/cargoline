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

Headless checks (these open no windows):

```bash
npm run typecheck                         # TypeScript
npm run build                             # production frontend build -> dist/
cargo check --manifest-path src-tauri/Cargo.toml
npm test                                          # frontend (node:test) + Rust tests
```

To build a release executable, run `npm run tauri build`. The executable is written to
`src-tauri/target/release/cargo-line.exe`. Installer bundling is turned off for now.

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
  tests/        Headless Rust tests (kept local, git-ignored)
```

- **Rust owns** business logic, local file IO, screenshot capture, OCR orchestration and SQLite. Only SQLite
  exists so far; the rest arrives in later tasks.
- **The frontend** only renders the UI and calls validated Rust commands. It never runs SQL directly.
- Stack: Tauri 2, React 19, TypeScript, Vite, Tailwind CSS 4, Lucide icons.

### Layout rules

- The top bar and the bottom navigation stay fixed. The main content area scrolls between them and is never
  covered by either bar.
- The Cargo page uses a CSS container query on the usable content width. Its two panels sit side by side at
  **990 px or more** and stack only below that, so a portrait window that is at least 990 px wide still shows
  two columns. The app does not set a minimum window size.

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
`src-tauri/tests/` (Rust). Project policy keeps test files local, so both folders are git-ignored;
`npm test` runs both suites. Tests only ever use temporary databases.

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
by the Star Citizen Wiki community. Unofficial; not affiliated with Cloud Imperium Games. The lists were
gathered once with a local script that is not part of this repository.
