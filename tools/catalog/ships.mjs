// Generates src-tauri/resources/catalog/ships.json: flight-ready spaceships with cargo, their
// capacity and the largest standard container their grids accept (derived from grid sizes).
//   node tools/catalog/ships.mjs [--version <code>] [--cache-only] [--check] [--out <path>]
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CATALOG_DIR, gameVersion, getAllPages, parseArgs, stats, writeCatalog } from "./lib.mjs";
import { selectShips } from "./rules.mjs";

const opts = parseArgs();
opts.version = await gameVersion(opts);

// The vehicles endpoint silently caps page[size] at 50.
const vehicles = await getAllPages("/vehicles", {}, opts, 50);
const { items, excluded, modular } = selectShips(vehicles);

// Changes of the largest container compared with the current file (for review).
const previous = existsSync(join(CATALOG_DIR, "ships.json")) ? JSON.parse(readFileSync(join(CATALOG_DIR, "ships.json"), "utf8")).items : [];
const before = new Map(previous.map((s) => [s.id, s.max_container_scu]));
const changed = items
  .filter((s) => before.has(s.id) && before.get(s.id) !== s.max_container_scu)
  .map((s) => `${s.name}: ${before.get(s.id)} → ${s.max_container_scu}`);

writeCatalog("ships.json", opts.version, items, opts);
const label = (v) => `${v.name} (${v.class_name}, ${v.cargo_capacity} SCU)`;
console.log(
  JSON.stringify(
    {
      game_version: opts.version,
      api_vehicles: vehicles.length,
      included: items.length,
      excluded: Object.fromEntries(Object.entries(excluded).map(([k, v]) => [k, v.length])),
      no_status_with_cargo: excluded.noStatus.filter((v) => v.cargo_capacity > 0).map(label),
      modular,
      max_container_unknown: items.filter((s) => s.max_container_scu === null).map((s) => s.name),
      max_container_changed: changed,
      ...stats(),
    },
    null,
    1,
  ),
);
