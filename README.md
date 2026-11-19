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
| Contracts: searchable location picker backed by a local location catalog | Implemented |
| Contracts: pickup/turn-in tracking, editing, completion, abandonment, OCR | Not implemented |
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

The database is `%APPDATA%\io.github.mathisvkg.cargoline\sc-cargo-app.sqlite3`. It is created on first launch
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

## Reference catalogs (locations and commodities)

The Add Contract location and commodity fields are "type or pick" pickers. Suggestions come from two
local catalog tables, `locations` (migration 0002) and `commodities` (migration 0003). The catalogs only
speed up entry and keep spellings consistent; any value can still be typed and saved.

- **Bundled entries** come from an offline snapshot of community data,
  `src-tauri/resources/catalog/reference-catalog.json`, compiled into the app. The app never calls the
  network for catalog data.
- **User entries** are names typed when saving a contract. They are added in the same transaction as the
  contract, so cancelling or a failed save leaves no unused entries.
- **Matching:** names are compared ignoring case and extra spaces. A typed name that matches an entry
  apart from case or spacing uses the catalog spelling.
- **Startup sync:** when the embedded snapshot's `catalog_version` differs from the one recorded in
  `catalog_metadata`, the app applies the snapshot in one transaction. Rows are matched by source id,
  then by name. A matching user entry becomes bundled. Bundled entries missing from a newer snapshot are
  retired, never deleted, and retired entries are hidden from suggestions. If the sync fails, the
  previous catalog is kept and contracts keep working.
- **Contracts still store plain names.** There is no foreign key from contracts to catalog rows.

### Refreshing the snapshot

```bash
node tools/catalog-import/import.mjs                       # current default LIVE build
node tools/catalog-import/import.mjs --version <build>     # pin a specific build
```

The importer needs Node 18+ and has no dependencies. It caches raw responses in
`tools/catalog-import/.cache/` (git-ignored) and throttles mission requests to one per second.

### Data source and attribution

Commodity and location data comes from the [Star Citizen Wiki API](https://api.star-citizen.wiki),
maintained by the Star Citizen Wiki community. It is used here for a free, non-commercial project;
commercial use is not permitted.

- **Commodities:** every API commodity with default cargo containers.
- **Locations:** landing zones (cities), manmade locations (stations), distribution centres and logistics
  depots, and outposts or other places named exactly in hauling-mission text.
- **`seen_in_hauling`:** a commodity is used by a hauling-mission order (matched by id), or a location is
  named exactly in hauling-mission pickup or destination text.
