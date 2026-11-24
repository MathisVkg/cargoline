// Tests for the catalog generators in tools/catalog (Task 14): the pure selection rules, the ship
// max-container derivation, and (when the API cache is present) byte-for-byte reproducibility.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import {
  LOCATION_TOKEN_KEY,
  enrichLocations,
  exclusionReason,
  gridMaxContainer,
  hauledCommodities,
  largestContainerFitting,
  placeNames,
  selectShips,
  shipMaxContainer,
} from "../../tools/catalog/rules.mjs";

const EXCLUSIONS = { genericSubAreas: ["Lobby", "Storehouse"], shipOrWreckNames: ["Caterpillar wreck site"] };

/* ----------------------------------------------------- ship max container */

test("largest container from a box's dimensions (unordered axes)", () => {
  assert.equal(largestContainerFitting({ x: 2.5, y: 10, z: 2.5 }), 32, "a 32-SCU footprint (Hull B / Hull C)");
  assert.equal(largestContainerFitting({ x: 10, y: 2.5, z: 2.5 }), 32, "rotated");
  assert.equal(largestContainerFitting({ x: 2.5, y: 2.5, z: 10 }), 32, "listed with the length on z (Polaris)");
  assert.equal(largestContainerFitting({ x: 2.5, y: 2.5, z: 1.25 }), 4, "600i");
  assert.equal(largestContainerFitting({ x: 5, y: 5, z: 2.5 }), 16, "5 m is too short for a 10 m container");
  assert.equal(largestContainerFitting({ x: 2.5, y: 7.5, z: 2.5 }), 24);
  assert.equal(largestContainerFitting({ x: 10, y: 10, z: 2.5 }), 32, "never above the largest standard size");
  assert.equal(largestContainerFitting({ x: 2.5, y: 1.25, z: 1.25 }), 2);
  assert.equal(largestContainerFitting({ x: 1.25, y: 1.25, z: 1.25 }), 1);
  assert.equal(largestContainerFitting({ x: 1.2, y: 1.2, z: 1.2 }), 0, "smaller than one SCU");
  assert.equal(largestContainerFitting(null), 0);
  assert.equal(largestContainerFitting({ x: 2.5, y: 10 }), 0, "missing a dimension");
});

test("per grid: placeholder and missing max_size are unknown, never a placeholder 1", () => {
  const hermesGrid = { width: 5, length: 22.5, height: 2.5, max_size: { x: 1.25, y: 1.25, z: 1.25 } };
  assert.equal(gridMaxContainer(hermesGrid), null, "the 1.25 m placeholder in a large grid");
  assert.equal(gridMaxContainer({ width: 2.5, length: 2.5, height: 2.5, max_size: { x: 1.25, y: 1.25, z: 1.25 } }), null, "even an 8-SCU grid");
  const tiny = { width: 1.25, length: 2.5, height: 1.25, max_size: { x: 1.25, y: 1.25, z: 1.25 } };
  assert.equal(gridMaxContainer(tiny), 1, "a genuinely tiny grid (no room for a 4-SCU box) keeps 1");
  // 1.25 × 3.75 × 2.5 m holds a 4-SCU box on its side (axes are unordered): the 1.25 m cube is a placeholder there.
  assert.equal(gridMaxContainer({ width: 1.25, length: 3.75, height: 2.5, max_size: { x: 1.25, y: 1.25, z: 1.25 } }), null);
  assert.equal(gridMaxContainer({ width: 10, length: 10, height: 7.5 }), null, "missing max_size");
  assert.equal(gridMaxContainer({ width: 10, length: 10, height: 7.5, max_size: { x: 2.5, y: 10, z: 2.5 } }), 32);
});

test("per ship: the maximum over known grids, null when none is known", () => {
  const placeholder = { width: 7.5, length: 25, height: 7.5, max_size: { x: 1.25, y: 1.25, z: 1.25 } };
  assert.equal(shipMaxContainer([placeholder, placeholder]), null, "Ironclad pattern");
  assert.equal(shipMaxContainer([{ max_size: { x: 5, y: 5, z: 2.5 } }, { max_size: { x: 2.5, y: 7.5, z: 2.5 } }]), 24, "Caterpillar pattern");
  assert.equal(shipMaxContainer([placeholder, { max_size: { x: 2.5, y: 2.5, z: 1.25 } }]), 4, "unknown grids are ignored");
  assert.equal(shipMaxContainer([]), null);
  assert.equal(shipMaxContainer(undefined), null);
});

test("ship selection rules (Task 11), with null max containers", () => {
  const v = (class_name: string, name: string, cargo: number, extra: Record<string, unknown> = {}) => ({
    class_name,
    name,
    cargo_capacity: cargo,
    is_spaceship: true,
    production_status: { en_EN: "flight-ready" },
    manufacturer: { name: "Maker", code: "MK" },
    cargo_grids: [{ width: 5, length: 20, height: 2.5, max_size: { x: 1.25, y: 1.25, z: 1.25 } }],
    ...extra,
  });
  const { items, excluded, modular } = selectShips([
    v("MK_Alpha", "Alpha", 10),
    v("MK_Alpha_Display", "Alpha", 10),
    v("MK_Beta", "beta", 0),
    v("MK_Gamma", "Gamma", 5, { production_status: { en_EN: "in-concept" } }),
    v("MK_Rover", "Rover", 4, { is_spaceship: false, is_vehicle: true }),
    v("MK_Delta", "Delta", 8, { production_status: null }),
    v("RSI_Galaxy", "Galaxy", 64),
    v("RSI_Galaxy_TEMP_Cargo", "Galaxy", 720, { cargo_grids: [{ max_size: { x: 2.5, y: 10, z: 2.5 } }] }),
  ]);
  assert.deepEqual(items.map((i) => [i.id, i.cargo_scu, i.max_container_scu]), [["MK_Alpha", 10, null], ["RSI_Galaxy_TEMP_Cargo", 720, 32]]);
  assert.deepEqual(Object.fromEntries(Object.entries(excluded).map(([k, l]) => [k, (l as unknown[]).length])), { zeroCargo: 1, ground: 1, concept: 1, noStatus: 1, duplicate: 2 });
  assert.equal(modular[0].chosen, "RSI_Galaxy_TEMP_Cargo");
  assert.throws(() => selectShips([v("X_A", "Odd", 1), v("X_B", "Odd", 2)]), /modular ship without a rule/);
});

/* -------------------------------------------------- locations, commodities */

test("location token values: exclusion rules", () => {
  assert.equal(exclusionReason("", EXCLUSIONS), "empty");
  assert.equal(exclusionReason("Remote Outpost near [NearbyLocation]", EXCLUSIONS), "unresolved token");
  assert.equal(exclusionReason("~mission(Location|Address)", EXCLUSIONS), "unresolved token");
  assert.equal(exclusionReason("a Salvage Yard on Daymar", EXCLUSIONS), "generic phrase");
  assert.equal(exclusionReason("the habs inside Checkmate", EXCLUSIONS), "generic phrase");
  assert.equal(exclusionReason("The cave can only be accessed on foot.", EXCLUSIONS), "description sentence");
  assert.equal(exclusionReason("Lobby", EXCLUSIONS), "generic sub-area");
  assert.equal(exclusionReason("Caterpillar wreck site", EXCLUSIONS), "ship or wreck name");
  assert.equal(exclusionReason('"The Orphanage"', EXCLUSIONS), null, "quoted names are kept");
  assert.equal(exclusionReason("Covalex Distribution Center S4DC05", EXCLUSIONS), null);
  assert.ok(LOCATION_TOKEN_KEY.test("Pickup1|Address") && LOCATION_TOKEN_KEY.test("DropOff1|Address") && LOCATION_TOKEN_KEY.test("Destination|Target|Address") && LOCATION_TOKEN_KEY.test("Location"));
  assert.ok(!LOCATION_TOKEN_KEY.test("Title") && !LOCATION_TOKEN_KEY.test("Itinerary") && !LOCATION_TOKEN_KEY.test("Contractor"));
});

test("place names: counting, spelling variants, long addresses", () => {
  const missions = [
    { mission_tokens: { "Pickup1|Address": ["Port Tressler", "Lobby", "Checkmate"], Title: ["Port Olisar"] } },
    { mission_tokens: { "DropOff1|Address": ["port  tressler", "Checkmate at the L4 Lagrange of Pyro II", "Orbituary above Pyro III"] } },
    { mission_tokens: { "Destination|Address": ["Port Tressler"] } },
  ];
  const r = placeNames(missions, EXCLUSIONS);
  assert.deepEqual(r.names.sort(), ["Checkmate", "Orbituary above Pyro III", "Port Tressler"], "short form exists only for Checkmate; Title ignored");
  assert.deepEqual(r.dropped, [{ name: "Checkmate at the L4 Lagrange of Pyro II", short: "Checkmate" }]);
  assert.deepEqual(r.excluded.map((e) => [e.name, e.reason]), [["Lobby", "generic sub-area"]]);
});

test("enrichment: exact single match only; system and the nearest planet or moon", () => {
  const records = [
    { uuid: "p", name: "microTech", system: "Stanton System", parent: { uuid: "s", name: "Stanton", type_name: "Star" } },
    { uuid: "st", name: "Port Tressler", system: "Stanton System", parent: { uuid: "p", name: "microTech", type_name: "Planet" } },
    { uuid: "c", name: "Port Tressler Clinic", system: "Stanton System", parent: { uuid: "st", name: "Port Tressler", type_name: "Manmade" } },
    { uuid: "m", name: "Adir", system: "Pyro System", parent: { uuid: "p5", name: "Pyro V", type_name: "Planet" } },
    { uuid: "d1", name: "QV Logistics Station", system: "Nyx System", parent: null },
    { uuid: "d2", name: "QV Logistics Station", system: "Nyx System", parent: null },
  ];
  const { items, ambiguous } = enrichLocations(["Port Tressler Clinic", "Adir", "QV Logistics Station", "Teasa Spaceport", "Port Tressler"], records);
  assert.deepEqual(items, [
    { name: "Adir", system: "Pyro", body: "Pyro V" },
    { name: "Port Tressler", system: "Stanton", body: "microTech" },
    { name: "Port Tressler Clinic", system: "Stanton", body: "microTech" },
    { name: "QV Logistics Station", system: null, body: null },
    { name: "Teasa Spaceport", system: null, body: null },
  ]);
  assert.deepEqual(ambiguous, [{ name: "QV Logistics Station", records: 2 }]);
});

test("commodities: Resource orders only, placeholders excluded, sorted", () => {
  const missions = [
    { hauling_orders: [{ kind: "Resource", uuid: "u1" }, { kind: "MissionItem", uuid: "x" }, { kind: "Resource", uuid: "u3" }] },
    { hauling_orders: [{ kind: "Resource", uuid: "u2" }, { kind: "Resource", uuid: "missing" }] },
  ];
  const commodities = [
    { uuid: "u1", name: "Titanium", commodity_groups: ["Metal"] },
    { uuid: "u2", name: "Agricultural Supplies", commodity_groups: [{ name: "Organic" }] },
    { uuid: "u3", name: "Heat", commodity_groups: ["HeatPlaceholder"] },
  ];
  const r = hauledCommodities(missions, commodities);
  assert.deepEqual(r.items, [{ name: "Agricultural Supplies", group: "Organic" }, { name: "Titanium", group: "Metal" }]);
  assert.deepEqual(r.placeholders, ["Heat"]);
  assert.deepEqual(r.unresolved, ["missing"]);
});

/* --------------------------------------------------------- reproducibility */

const cacheDir = join(import.meta.dirname, "..", "..", "tools", "catalog", ".cache");
const cached = (prefix: string) => existsSync(cacheDir) && readdirSync(cacheDir).some((f) => f.startsWith(prefix));
const VERSION = "4.10.3-LIVE.13165946";

for (const [name, needs] of [
  ["ships", ["vehicles"]],
  ["commodities", ["missions", "commodities"]],
  ["locations", ["missions", "locations"]],
] as const) {
  const ready = needs.every(cached);
  test(`${name}.json regenerates byte for byte from the cache`, { skip: ready ? false : `no API cache in tools/catalog/.cache (run node tools/catalog/${name}.mjs once)` }, () => {
    const out = execFileSync(process.execPath, [join(cacheDir, "..", `${name}.mjs`), "--version", VERSION, "--cache-only", "--check"], { encoding: "utf8" });
    assert.match(out, new RegExp(`${name}\\.json: identical`));
  });
}
