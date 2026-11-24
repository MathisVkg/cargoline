// Pure selection rules for the bundled catalogs (no I/O), shared by the generators and the tests.
import { byName, sortKey } from "./lib.mjs";

/* ---------------------------------------------------------------- locations */

/**
 * Mission token keys that hold pickup / destination names, e.g. `Pickup1|Address`,
 * `DropOff1|Address`, `Destination|Target|Address`, `Location`. Values are lists of possible
 * place names for that mission template.
 */
export const LOCATION_TOKEN_KEY = /^(location|pickup|dropoff|destination|dropofflocation)\d*(\|.*)?$/i;

/** Why a token value is not a place name, or null when it is kept. */
export function exclusionReason(name, exclusions) {
  if (name.trim() === "") return "empty";
  if (/~mission\(|[<>[\]{}|]/.test(name)) return "unresolved token";
  if (/^(a|an|the) /.test(name)) return "generic phrase"; // lowercase article: "a Salvage Yard on Daymar"
  if (/\.$/.test(name)) return "description sentence"; // "The cave can only be accessed on foot."
  if (exclusions.genericSubAreas.includes(name)) return "generic sub-area";
  if (exclusions.shipOrWreckNames.includes(name)) return "ship or wreck name";
  return null;
}

/** A long address whose short form is also a kept name: "Checkmate at the L4 Lagrange of Pyro II". */
export const LONG_ADDRESS = /^(.+?) (?:at the L\d Lagrange (?:point )?of .+|above .+)$/;

/**
 * Place names from hauling mission details.
 * - every value of every location token key, counted;
 * - spelling variants (same name ignoring case/whitespace) → the most frequent spelling;
 * - exclusions (see exclusionReason);
 * - long addresses dropped when their short form is in the list.
 */
export function placeNames(missions, exclusions) {
  const counts = new Map();
  for (const m of missions) {
    for (const [key, values] of Object.entries(m.mission_tokens ?? {})) {
      if (!LOCATION_TOKEN_KEY.test(key) || !Array.isArray(values)) continue;
      for (const v of values) if (typeof v === "string") counts.set(v, (counts.get(v) ?? 0) + 1);
    }
  }
  const excluded = [];
  const variants = new Map();
  for (const [name, count] of counts) {
    const reason = exclusionReason(name, exclusions);
    if (reason) {
      excluded.push({ name, count, reason });
      continue;
    }
    const k = sortKey(name);
    const best = variants.get(k);
    if (!best || count > best.count || (count === best.count && name < best.name)) variants.set(k, { name, count });
  }
  const kept = new Set([...variants.values()].map((v) => v.name));
  const dropped = [];
  for (const name of [...kept]) {
    const m = LONG_ADDRESS.exec(name);
    if (m && kept.has(m[1])) {
      kept.delete(name);
      dropped.push({ name, short: m[1] });
    }
  }
  return { names: [...kept], excluded, dropped, distinct: counts.size };
}

/**
 * System and body from `/api/locations`, for exact name matches with a single record only.
 * System: "Stanton System" → "Stanton". Body: the nearest Planet or Moon above the record.
 */
export function enrichLocations(names, records) {
  const byUuid = new Map(records.map((r) => [r.uuid, r]));
  const byExactName = new Map();
  for (const r of records) {
    const list = byExactName.get(r.name) ?? [];
    list.push(r);
    byExactName.set(r.name, list);
  }
  const ambiguous = [];
  const items = names.map((name) => {
    const found = byExactName.get(name) ?? [];
    if (found.length !== 1) {
      if (found.length > 1) ambiguous.push({ name, records: found.length });
      return { name, system: null, body: null };
    }
    const r = found[0];
    const system = typeof r.system === "string" && r.system.trim() ? r.system.replace(/ System$/, "") : null;
    let body = null;
    let p = r.parent;
    const seen = new Set();
    while (p && !seen.has(p.uuid)) {
      seen.add(p.uuid);
      if (p.type_name === "Planet" || p.type_name === "Moon") {
        body = p.name;
        break;
      }
      p = byUuid.get(p.uuid)?.parent ?? null;
    }
    return { name, system, body };
  });
  items.sort(byName);
  return { items, ambiguous };
}

/* -------------------------------------------------------------- commodities */

/**
 * Commodities hauled by `Resource` orders in hauling missions, excluding placeholder groups
 * (HeatPlaceholder, …). Item: { name, group } with the first commodity group.
 */
export function hauledCommodities(missions, commodities) {
  const uuids = new Set();
  for (const m of missions) for (const o of m.hauling_orders ?? []) if (o.kind === "Resource" && o.uuid) uuids.add(o.uuid);
  const byUuid = new Map(commodities.map((c) => [c.uuid, c]));
  const unresolved = [...uuids].filter((u) => !byUuid.has(u));
  const groupName = (g) => (typeof g === "string" ? g : (g?.name ?? g?.key ?? null));
  const items = [];
  const placeholders = [];
  for (const u of uuids) {
    const c = byUuid.get(u);
    if (!c) continue;
    const groups = (c.commodity_groups ?? []).map(groupName).filter(Boolean);
    if (groups.some((g) => /Placeholder$/.test(g))) {
      placeholders.push(c.name);
      continue;
    }
    items.push({ name: c.name, group: groups[0] ?? null });
  }
  items.sort(byName);
  return { items, unresolved, placeholders, orders: uuids.size };
}

/* -------------------------------------------------------------------- ships */

/** Container dimensions in 1.25 m cells: width × length × height (Task 13.1 table). */
export const CONTAINER_CELLS = {
  1: [1, 1, 1],
  2: [1, 2, 1],
  4: [2, 2, 1],
  8: [2, 2, 2],
  16: [2, 4, 2],
  24: [2, 6, 2],
  32: [2, 8, 2],
};
export const CELL_M = 1.25;
const cells = (metres) => Math.floor(metres / CELL_M + 1e-6);

const valid = (...v) => v.every((x) => typeof x === "number" && x > 0);
const ascending = (a, b) => a - b;

/**
 * Largest standard container (SCU) fitting a box of `{x, y, z}` metres, 0 if none.
 *
 * The box is treated as **unordered**: a container fits when its sorted dimensions fit the box's
 * sorted dimensions. The API's axis order is not consistent between ships, for `max_size` and
 * for the grid itself:
 * - Polaris: `max_size` 2.5 × 2.5 × 10 m in a grid 5 m "high" → its 10 m is a length;
 * - 600i: grid 5 × 1.25 × 2.5 m (width × length × height) with `max_size` 2.5 × 2.5 × 1.25 m,
 *   which only fits if that grid's "length" and "height" are swapped.
 * A fixed vertical axis (horizontal rotation only) would wrongly reject 32 on the Polaris or 4 on
 * the 600i, so no axis is assumed to be vertical.
 */
export function largestContainerFitting(size) {
  if (!size || !valid(size.x, size.y, size.z)) return 0;
  const m = [cells(size.x), cells(size.y), cells(size.z)].sort(ascending);
  let best = 0;
  for (const [scu, dims] of Object.entries(CONTAINER_CELLS)) {
    const s = [...dims].sort(ascending);
    if (s[0] <= m[0] && s[1] <= m[1] && s[2] <= m[2]) best = Math.max(best, Number(scu));
  }
  return best;
}

const isPlaceholderCube = (s) => s && [s.x, s.y, s.z].every((v) => Math.abs(v - CELL_M) < 1e-6);

/**
 * The largest container one grid accepts, from its `max_size`, or null when unknown:
 * - `max_size` missing;
 * - the 1.25 m placeholder cube while the grid itself would hold at least a 4-SCU container
 *   (the Hermes / Ironclad pattern).
 * Grid dimensions: width → x, length → y, height → z.
 */
export function gridMaxContainer(grid) {
  const max = grid?.max_size;
  if (!max) return null;
  if (isPlaceholderCube(max)) {
    // The grid itself, as a box: does it hold more than the placeholder says?
    const room = largestContainerFitting({ x: grid.width, y: grid.length, z: grid.height });
    if (room >= 4) return null;
  }
  const best = largestContainerFitting(max);
  return best > 0 ? best : null;
}

/** The ship's largest container: the maximum over its known grids; null if none is known. */
export function shipMaxContainer(grids) {
  const known = (grids ?? []).map(gridMaxContainer).filter((v) => v !== null);
  return known.length ? Math.max(...known) : null;
}

const en = (o) => (o && typeof o === "object" ? o.en_EN : o);
const kind = (v) => (v.is_spaceship ? "spaceship" : v.is_gravlev ? "gravlev" : v.is_vehicle ? "ground" : "other");
/** Suffixes of internal copies: a class name carrying one is less canonical. */
const COPY = /_(Display|BIS\d+|FW_\d+|TSG|Boarded.*|Collector.*|GS|PIR)$|_(Display|Boarded|Collector)_/i;
const plainness = (v) => (COPY.test(v.class_name) ? 1 : 0) * 1000 + v.class_name.length;

/** Modular ships: the configuration used when one name has several capacities. */
export const MODULAR_CHOICE = { Galaxy: "RSI_Galaxy_TEMP_Cargo" };

/**
 * Ships for the selector (Task 11 rules):
 * - include flight-ready spaceships with cargo_capacity > 0;
 * - exclude 0 SCU, ground vehicles / gravlev, concepts, and entries without a production status;
 * - same name + same capacity → one entry, the plainest class name wins;
 * - same name + different capacities → modular, resolved by MODULAR_CHOICE (else an error).
 */
export function selectShips(vehicles) {
  const excluded = { zeroCargo: [], ground: [], concept: [], noStatus: [], duplicate: [] };
  const candidates = [];
  for (const v of vehicles) {
    const status = en(v.production_status);
    if (kind(v) !== "spaceship") excluded.ground.push(v);
    else if (!status) excluded.noStatus.push(v);
    else if (status !== "flight-ready") excluded.concept.push(v);
    else if (!(v.cargo_capacity > 0)) excluded.zeroCargo.push(v);
    else candidates.push(v);
  }
  const byShipName = new Map();
  for (const v of candidates) {
    const list = byShipName.get(v.name) ?? [];
    list.push(v);
    byShipName.set(v.name, list);
  }
  const items = [];
  const modular = [];
  for (const [name, list] of byShipName) {
    let chosen;
    if (new Set(list.map((v) => v.cargo_capacity)).size > 1) {
      chosen = list.find((v) => v.class_name === MODULAR_CHOICE[name]);
      if (!chosen) throw new Error(`modular ship without a rule: ${name}`);
      modular.push({ name, chosen: chosen.class_name, raw: list.map((v) => [v.class_name, v.cargo_capacity]) });
    } else {
      chosen = [...list].sort((a, b) => plainness(a) - plainness(b))[0];
    }
    for (const v of list) if (v !== chosen) excluded.duplicate.push(v);
    items.push({
      id: chosen.class_name,
      name: chosen.name,
      manufacturer: chosen.manufacturer?.name ?? "",
      manufacturer_code: chosen.manufacturer?.code ?? "",
      cargo_scu: chosen.cargo_capacity,
      max_container_scu: shipMaxContainer(chosen.cargo_grids),
    });
  }
  items.sort(byName);
  return { items, excluded, modular };
}
