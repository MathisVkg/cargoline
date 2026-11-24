// Generates src-tauri/resources/catalog/commodities.json: commodities hauled by `Resource` orders
// in hauling missions, without placeholder groups.
//   node tools/catalog/commodities.mjs [--version <code>] [--cache-only] [--check] [--out <path>]
import { gameVersion, getAllPages, parseArgs, stats, writeCatalog } from "./lib.mjs";
import { loadHaulingMissions } from "./missions.mjs";
import { hauledCommodities } from "./rules.mjs";

const opts = parseArgs();
opts.version = await gameVersion(opts);

const { details } = await loadHaulingMissions(opts);
const commodities = await getAllPages("/commodities", {}, opts, 200);
const { items, unresolved, placeholders, orders } = hauledCommodities(details, commodities);
writeCatalog("commodities.json", opts.version, items, opts);

const groups = {};
for (const i of items) groups[i.group] = (groups[i.group] ?? 0) + 1;
console.log(
  JSON.stringify(
    { game_version: opts.version, resource_uuids: orders, unresolved: unresolved.length, placeholder_excluded: placeholders, kept: items.length, groups, ...stats() },
    null,
    1,
  ),
);
