#!/usr/bin/env node
// Cargo Line — reference catalog importer (maintenance tool, not shipped with the app).
//
// Fetches commodities and locations from the Star Citizen Wiki API
// (https://api.star-citizen.wiki, data © Star Citizen Wiki community, non-commercial use)
// and writes one deterministic, sorted snapshot:
//   src-tauri/resources/catalog/reference-catalog.json
//
// Usage:  node tools/catalog-import/import.mjs [--version <game build>] [--cache <dir>] [--out <file>]
// Requires Node 18+ (global fetch). No dependencies. Mission details are cached per mission in the
// cache directory, so re-runs only fetch what is missing. Requests are throttled.

import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const API = "https://api.star-citizen.wiki";
const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..", "..");
const SCHEMA_VERSION = 1;
const PAGE_SIZE = 200;
const DETAIL_DELAY_MS = 1000;
const ATTRIBUTION =
  "Commodity and location data from the Star Citizen Wiki API (api.star-citizen.wiki), maintained by the " +
  "Star Citizen Wiki community. Used for a free, non-commercial project; commercial use is not permitted.";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => (arg.startsWith("--") ? [...pairs, [arg.slice(2), all[i + 1]]] : pairs), []),
);
const CACHE = resolve(args.cache ?? join(HERE, ".cache"));
const OUT = resolve(args.out ?? join(ROOT, "src-tauri", "resources", "catalog", "reference-catalog.json"));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const normalizeDisplay = (s) => String(s ?? "").trim().replace(/\s+/g, " ");
const normalizeKey = (s) => normalizeDisplay(s).toLowerCase();
const isPlaceholder = (name) => !name || /<=|=>|UNINITIALI[SZ]ED|PLACEHOLDER/i.test(name) || /^WIP\b/i.test(name);

async function getJson(path, attempt = 1) {
  const url = path.startsWith("http") ? path : `${API}${path}`;
  const response = await fetch(url, { headers: { Accept: "application/json", "User-Agent": "cargo-line-catalog-import" } });
  if (response.status === 429 || response.status >= 500) {
    if (attempt > 5) throw new Error(`${response.status} after retries: ${url}`);
    await sleep(2000 * attempt);
    return getJson(path, attempt + 1);
  }
  if (!response.ok) throw new Error(`${response.status} ${response.statusText}: ${url}`);
  return response.json();
}

async function getAllPages(path, version, pageSize = PAGE_SIZE) {
  const sep = path.includes("?") ? "&" : "?";
  const items = [];
  for (let page = 1; ; page++) {
    const body = await getJson(`${path}${sep}version=${encodeURIComponent(version)}&page%5Bsize%5D=${pageSize}&page%5Bnumber%5D=${page}`);
    items.push(...body.data);
    process.stdout.write(`  ${path} page ${page}/${body.meta.last_page}\r`);
    if (page >= body.meta.last_page) break;
    await sleep(300);
  }
  process.stdout.write("\n");
  return items;
}

async function cached(name, produce) {
  const file = join(CACHE, name);
  try {
    return JSON.parse(await readFile(file, "utf8"));
  } catch {
    const value = await produce();
    await mkdir(dirname(file), { recursive: true });
    await writeFile(file, JSON.stringify(value));
    return value;
  }
}

/* ------------------------------------------------------------------------- */

async function main() {
  const version = args.version ?? (await getJson("/api/game-versions/default")).data.code;
  console.log(`Game version: ${version}`);
  const vdir = `v-${version.replace(/[^\w.-]/g, "_")}`;

  const commodities = await cached(`${vdir}/commodities.json`, () => getAllPages("/api/commodities", version));
  const locations = await cached(`${vdir}/locations.json`, () => getAllPages("/api/locations", version));
  const hauling = await cached(`${vdir}/hauling-missions.json`, () =>
    // Mission pages are heavy; the API errors on 200-item pages, so use 50.
    getAllPages("/api/missions?filter%5Breward_scope%5D=Hauling", version, 50),
  );

  // Mission details: hauling orders (commodity uuids) and token pickup/destination names.
  const details = [];
  for (const [i, mission] of hauling.entries()) {
    const detail = await cached(`${vdir}/missions/${mission.uuid}.json`, async () => {
      await sleep(DETAIL_DELAY_MS);
      return (await getJson(`/api/missions/${mission.uuid}?version=${encodeURIComponent(version)}`)).data;
    });
    details.push(detail);
    if (i % 25 === 0 || i === hauling.length - 1) process.stdout.write(`  mission details ${i + 1}/${hauling.length}\r`);
  }
  process.stdout.write("\n");

  const hauledCommodityIds = new Set();
  const tokenNames = new Set();
  const TOKEN_KEY = /^(Location|Pickup|Dropoff|DropOff|Destination)\d*(\|Address)?$/;
  for (const d of details) {
    for (const order of d?.hauling_orders ?? []) if (order.kind === "Resource" && order.uuid) hauledCommodityIds.add(order.uuid);
    for (const [key, values] of Object.entries(d?.mission_tokens ?? {})) {
      if (!TOKEN_KEY.test(key)) continue;
      for (const v of [].concat(values)) if (typeof v === "string" && !/[<>[\]]/.test(v)) tokenNames.add(normalizeKey(v));
    }
  }

  /* ---------- commodities ---------- */
  const humanize = (g) => g.replace(/_/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2");
  const commodityRows = [];
  const commodityStats = { api: commodities.length, excludedNoCargoContainers: 0, excludedEmptyName: 0, duplicatesDropped: 0 };
  for (const c of commodities) {
    const name = normalizeDisplay(c.name);
    if (!name || isPlaceholder(name)) { commodityStats.excludedEmptyName++; continue; }
    // The API marks non-cargo entries (placeholders, gases, samples) with has_default_cargo_containers=false.
    if (c.has_default_cargo_containers !== true) { commodityStats.excludedNoCargoContainers++; continue; }
    commodityRows.push({
      source_id: c.uuid,
      name,
      key: normalizeKey(name),
      kind: c.commodity_groups?.[0] ? humanize(c.commodity_groups[0]) : null,
      seen_in_hauling: hauledCommodityIds.has(c.uuid),
    });
  }

  /* ---------- locations ---------- */
  const byId = new Map(locations.map((l) => [l.uuid, l]));
  const bodyOf = (loc) => {
    let current = loc.parent ? byId.get(loc.parent.uuid) : undefined;
    for (let depth = 0; current && depth < 8; depth++) {
      if (/^(Planet|Moon)$/.test(current.type?.name ?? "")) return normalizeDisplay(current.name);
      current = current.parent ? byId.get(current.parent.uuid) : undefined;
    }
    return null;
  };
  const systemOf = (loc) => {
    const s = normalizeDisplay(loc.system).replace(/\s+System$/i, "");
    return s && !isPlaceholder(s) ? s : null;
  };
  const CELESTIAL = /^(Star|Planet|Moon|SolarSystem|Asteroid|Asteroid_ValidQT|JumpPoint|NavPoint|Anomaly)$/;
  const DEPOT = /Distribution Cent(re|er)|Logistics Depot/i;
  const KIND_RANK = { city: 0, station: 1, distribution_centre: 2, outpost: 3, other: 4 };

  const locationStats = { api: locations.length, rules: {}, perSystem: {}, duplicatesDropped: [], seenInHauling: 0 };
  const candidates = [];
  for (const loc of locations) {
    const name = normalizeDisplay(loc.name);
    if (isPlaceholder(name)) continue;
    const type = loc.type?.name ?? "";
    const seen = tokenNames.has(normalizeKey(name));
    let kind = null;
    let rule = null;
    if (type === "LandingZone") [kind, rule] = ["city", "landing zone"];
    else if (/^Manmade/.test(type)) [kind, rule] = ["station", "station (manmade)"];
    else if (/^Outpost/.test(type) && DEPOT.test(name)) [kind, rule] = ["distribution_centre", "depot by name"];
    else if (/^Outpost/.test(type) && seen) [kind, rule] = ["outpost", "outpost named by hauling missions"];
    else if (!CELESTIAL.test(type) && seen) [kind, rule] = ["other", "other place named by hauling missions"];
    if (!kind) continue;
    candidates.push({
      row: { source_id: loc.uuid, name, key: normalizeKey(name), kind, body: bodyOf(loc), system: systemOf(loc), seen_in_hauling: seen },
      rule,
    });
  }

  // One entry per normalized name: prefer seen_in_hauling, then kind rank, then source id.
  candidates.sort(
    (a, b) =>
      a.row.key.localeCompare(b.row.key) ||
      Number(b.row.seen_in_hauling) - Number(a.row.seen_in_hauling) ||
      KIND_RANK[a.row.kind] - KIND_RANK[b.row.kind] ||
      a.row.source_id.localeCompare(b.row.source_id),
  );
  const locationRows = [];
  for (const c of candidates) {
    if (locationRows.length && locationRows.at(-1).key === c.row.key) {
      locationStats.duplicatesDropped.push(`${c.row.name} (${c.row.source_id})`);
      continue;
    }
    locationRows.push(c.row);
    locationStats.rules[c.rule] = (locationStats.rules[c.rule] ?? 0) + 1;
    const sys = c.row.system ?? "unknown";
    locationStats.perSystem[sys] ??= {};
    locationStats.perSystem[sys][c.rule] = (locationStats.perSystem[sys][c.rule] ?? 0) + 1;
    if (c.row.seen_in_hauling) locationStats.seenInHauling++;
  }

  const bySourceKey = (a, b) => a.key.localeCompare(b.key) || a.source_id.localeCompare(b.source_id);
  commodityRows.sort(bySourceKey);
  locationRows.sort(bySourceKey);
  // Unique keys per table (commodity duplicates, if any, keep the first source id).
  const uniqueCommodities = commodityRows.filter((c, i) => i === 0 || commodityRows[i - 1].key !== c.key);
  commodityStats.duplicatesDropped = commodityRows.length - uniqueCommodities.length;

  const content = { commodities: uniqueCommodities, locations: locationRows };
  const hash = createHash("sha256").update(JSON.stringify(content)).digest("hex").slice(0, 12);
  const snapshot = {
    schema_version: SCHEMA_VERSION,
    catalog_version: `${version}+${hash}`,
    source: { name: "Star Citizen Wiki API", url: API, docs: `${API}/developers` },
    game_version: version,
    fetched_at: new Date().toISOString().slice(0, 10),
    attribution: ATTRIBUTION,
    license: "Non-commercial use only. Credit api.star-citizen.wiki.",
    ...content,
  };
  await mkdir(dirname(OUT), { recursive: true });
  await writeFile(OUT, `${JSON.stringify(snapshot, null, 2)}\n`);

  const stats = {
    version,
    catalog_version: snapshot.catalog_version,
    haulingMissions: hauling.length,
    detailsFetched: details.filter(Boolean).length,
    hauledCommodityIds: hauledCommodityIds.size,
    tokenNames: tokenNames.size,
    commodities: { ...commodityStats, written: uniqueCommodities.length, seenInHauling: uniqueCommodities.filter((c) => c.seen_in_hauling).length },
    locations: { ...locationStats, written: locationRows.length },
  };
  await writeFile(join(CACHE, `${vdir}/stats.json`), JSON.stringify(stats, null, 2));
  console.log(JSON.stringify({ ...stats, locations: { ...stats.locations, duplicatesDropped: stats.locations.duplicatesDropped.length } }, null, 2));
  console.log(`Wrote ${OUT}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
