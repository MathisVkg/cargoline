# Catalog generators

These scripts produce the bundled reference lists in `src-tauri/resources/catalog/`:

| Script | Writes | What it lists |
|---|---|---|
| `locations.mjs` | `locations.json` | Place names that hauling missions use as pickups and destinations, with system and body where known |
| `commodities.mjs` | `commodities.json` | Commodities hauled by hauling missions, with their group |
| `ships.mjs` | `ships.json` | Flight-ready spaceships with cargo: capacity (SCU) and the largest standard container their grids accept |

`missions.mjs` is shared by the first two (it can also be run on its own to fill the cache). The selection
rules live in `rules.mjs` as pure functions, tested by `tests/frontend/catalog-tools.test.ts`.
`exclusions.json` holds the hand-reviewed lists used by `locations.mjs`.

Plain Node (the version in `.nvmrc`); no npm dependencies.

## Source, attribution and use

All data comes from the [Star Citizen Wiki API](https://api.star-citizen.wiki), maintained by the Star
Citizen Wiki community. Every generated file carries this header:

```json
"source": "Star Citizen Wiki API (api.star-citizen.wiki)",
"attribution": "Data from api.star-citizen.wiki. Unofficial; not affiliated with Cloud Imperium Games.",
"game_version": "<the game build the data describes>",
"generated_at": "<YYYY-MM-DD>"
```

**Non-commercial use only.** Cargo Line is an unofficial fan project. The Wiki data describes Star Citizen,
whose content belongs to Cloud Imperium Games. Use it for non-commercial purposes only, always keep the
attribution, and check the Star Citizen Wiki's terms before any other use.

## Running

```bash
node tools/catalog/ships.mjs                                   # the API's default game version
node tools/catalog/locations.mjs --version 4.10.3-LIVE.13165946
node tools/catalog/commodities.mjs --cache-only --check        # offline: would the file change?
```

| Option | Effect |
|---|---|
| `--version <code>` | Pin the game version (sent as `?version=` on every request). Default: the API's default version. |
| `--cache-only` | Never use the network; fail if a response is not cached. |
| `--check` | Write nothing; print whether the output equals the committed file; exit 1 if it differs. |
| `--out <path>` | Write the output somewhere else (e.g. to compare by hand). |

- **Requests:** User-Agent `cargo-line-catalog-gather`, at most **one request per second**, up to 4 attempts
  each.
- **Cache:** every response is stored in `tools/catalog/.cache/` (git-ignored). The cache key is the full URL,
  including the version, so different versions never mix.
- **Output:** sorted (case- and whitespace-insensitive), pretty-printed with 2 spaces and a final newline.
  `generated_at` keeps the committed file's date when the version and the items are unchanged, so a rerun
  without data changes leaves the file byte-identical. `.gitattributes` keeps these files LF on every
  checkout.

### Duration (empty cache)

| Script | Requests | Time |
|---|---|---|
| `ships.mjs` | about 8 | about 10 s |
| `commodities.mjs` | about 1,900 (hauling missions) + 2 | about 32 min |
| `locations.mjs` | the same missions + 11 | about 32 min (seconds after `commodities.mjs`) |

With a full cache, every script runs in a few seconds.

## After a game patch

1. Check the API's default version: `https://api.star-citizen.wiki/api/game-versions/default`.
2. Run all three scripts with `--version <new code>`. The new version has its own cache entries.
3. Review the diff. `git diff --stat src-tauri/resources/catalog/` gives the size of the change, then
   `git diff` the files. The scripts also print a summary with counts, exclusions, dropped long addresses,
   ambiguous names, unknown or changed ship containers, and ships without a production status.
   - **Locations:** look for new values that are sub-areas, ship or wreck names, or phrases. Add them to
     `exclusions.json` (exact spelling) and run again.
   - **Ships:** a new modular ship (one name, several capacities) stops the script with "modular ship
     without a rule". Add it to `MODULAR_CHOICE` in `rules.mjs`.
4. Run the tests: `npm test`. The Rust tests check that each file parses and follows the list rules.

## Rules

### locations.json (Task 04.2)

- **Missions:** `/api/missions?filter[reward_scope]=Hauling`, list pages of 50 (larger mission pages answer
  HTTP 500), duplicates removed by uuid, then one detail request per mission.
- **Names:** every value of every location token key in `mission_tokens`: `Location…`, `Pickup…`,
  `Dropoff…` / `DropOff…`, `Destination…` and `DropOffLocation…`, with or without `|Address` (see
  `LOCATION_TOKEN_KEY`). The in-game spelling is kept exactly.
- **Excluded:**
  - **empty** values;
  - **unresolved tokens:** containing `~mission(`, `<`, `>`, `[`, `]`, `{`, `}` or `|`;
  - **generic phrases:** starting with a lowercase article (`a `, `an `, `the `), e.g. "a Salvage Yard on
    Daymar";
  - **description sentences:** ending with a full stop, e.g. "The cave can only be accessed on foot.";
  - **generic sub-areas** and **ship or wreck names:** the exact lists in `exclusions.json`.
- **Spelling variants** (equal ignoring case and whitespace): the most frequent spelling is kept.
- **Long addresses** such as "Checkmate at the L4 Lagrange of Pyro II" or "Orbituary above Pyro III" are
  dropped when their short form ("Checkmate", "Orbituary") is also in the list (Task 04.2 follow-up). They
  are kept when no short form exists.
- **System and body:** from `/api/locations`, only for an **exact name match with a single record**.
  - System: "Stanton System" → "Stanton".
  - Body: the nearest Planet or Moon going up the parent chain.
  - Several records with the same name (e.g. "QV Logistics Station") or no record: both null.

### commodities.json (Task 04.2)

- Commodities referenced by `Resource` hauling orders of the same missions, matched by uuid in
  `/api/commodities`.
- Excluded: commodities in a `…Placeholder` group ("Cooler" and "Heat" in 4.10.3).
- `group` is the commodity's first group.

### ships.json (Tasks 11 and 14)

- **Included:** spaceships (`is_spaceship`), `production_status` "flight-ready", `cargo_capacity > 0`.
- **Excluded:** 0 SCU, ground vehicles and gravlev, concepts, and entries without a production status.
  The last group includes Wikelo / PYAM / Alliance specials, Hyun and the 600i Executive Edition; the
  script lists the ones with cargo for review.
- **Duplicates** (same name, same capacity: paints, display or test copies): one entry. The plainest class
  name wins; `_Display`, `_BIS…`, `_FW_…`, `_TSG`, `_Boarded…`, `_Collector…`, `_GS` and `_PIR` count
  as copies.
- **Modular ships** (same name, different capacities) follow `MODULAR_CHOICE`. Galaxy uses
  `RSI_Galaxy_TEMP_Cargo` (720 SCU, cargo module).
- **`max_container_scu`** (Task 14) is derived from the grids' `max_size`, **not** from the API's
  `max_scu_box`. That field is unreliable: the Hermes and Ironclad say 1, and the Hull B and Hull C say 16
  for grids that hold 32-SCU containers.
  - Standard containers in 1.25 m cells: 1 SCU 1×1×1, 2 SCU 1×2×1, 4 SCU 2×2×1, 8 SCU 2×2×2, 16 SCU
    2×4×2, 24 SCU 2×6×2, 32 SCU 2×8×2.
  - **A grid** accepts the largest container whose sorted dimensions fit the sorted `max_size`
    dimensions. Axes are treated as **unordered**, because the API's axis order is not consistent: the
    Polaris lists 2.5 × 2.5 × **10** m for a grid that is 5 m "high", and a 600i grid only fits its
    `max_size` with "length" and "height" swapped.
  - **Unknown grids:** `max_size` missing, or the 1.25 m placeholder cube in a grid large enough for a
    4-SCU container (the Hermes / Ironclad pattern).
  - **The ship:** the maximum over its known grids, or **null** when every grid is unknown. A placeholder 1
    is never written.
  - The app no longer uses this value for box sizes (Task 13.2). It is informational.
