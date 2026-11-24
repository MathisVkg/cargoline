// Hauling missions (list + one detail request each), shared by locations.mjs and commodities.mjs.
// About 1,900 requests at 1 request/s on an empty cache (roughly 32 minutes); seconds when cached.
//   node tools/catalog/missions.mjs [--version <code>] [--cache-only]   fills the cache only
import { pathToFileURL } from "node:url";
import { apiUrl, getAllPages, getJson, parseArgs, stats } from "./lib.mjs";

/** Unique hauling missions with their details, in a stable order (by uuid). */
export async function loadHaulingMissions(opts) {
  // Mission list pages use size 50: the API answers HTTP 500 on larger mission pages.
  const list = await getAllPages("/missions", { "filter[reward_scope]": "Hauling" }, opts, 50);
  const uuids = [...new Set(list.map((m) => m.uuid))].sort();
  const details = [];
  let done = 0;
  for (const uuid of uuids) {
    const json = await getJson(apiUrl(`/missions/${uuid}`, {}, opts.version), opts);
    details.push(json.data);
    done++;
    if (done % 100 === 0) console.error(`  missions: ${done}/${uuids.length}`);
  }
  return { listed: list.length, details };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const opts = parseArgs();
  const { listed, details } = await loadHaulingMissions(opts);
  console.log(`hauling missions: ${listed} listed, ${details.length} unique`, stats());
}
