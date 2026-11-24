// Generates src-tauri/resources/catalog/locations.json: place names used as pickups and
// destinations by hauling missions, with system and body where an exact location record exists.
//   node tools/catalog/locations.mjs [--version <code>] [--cache-only] [--check] [--out <path>]
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { TOOL_DIR, gameVersion, getAllPages, parseArgs, stats, writeCatalog } from "./lib.mjs";
import { loadHaulingMissions } from "./missions.mjs";
import { enrichLocations, placeNames } from "./rules.mjs";

const opts = parseArgs();
opts.version = await gameVersion(opts); // every request carries the version (pinned or default)
const exclusions = JSON.parse(readFileSync(join(TOOL_DIR, "exclusions.json"), "utf8"));

const { listed, details } = await loadHaulingMissions(opts);
const { names, excluded, dropped, distinct } = placeNames(details, exclusions);
const records = await getAllPages("/locations", {}, opts, 200);
const { items, ambiguous } = enrichLocations(names, records);
writeCatalog("locations.json", opts.version, items, opts);

const byReason = {};
for (const e of excluded) byReason[e.reason] = (byReason[e.reason] ?? 0) + 1;
console.log(
  JSON.stringify(
    {
      game_version: opts.version,
      hauling_missions: { listed, unique: details.length },
      distinct_token_values: distinct,
      excluded: byReason,
      long_address_dropped: dropped.map((d) => `${d.name} → ${d.short}`),
      kept: items.length,
      with_system_or_body: items.filter((i) => i.system || i.body).length,
      ambiguous_matches: ambiguous.map((a) => `${a.name} (${a.records})`),
      ...stats(),
    },
    null,
    1,
  ),
);
