// Frontend unit tests. Run: npm run test:frontend
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  boxInfo,
  contractRef,
  groupByContract,
  groupByLocation,
  isExpanded,
  setAllExpanded,
  toggleExpanded,
} from "../../src/features/contracts/grouping.ts";
import { emptyDraft, emptyRequirement, emptyStop, toInput, validateDraft } from "../../src/features/contracts/draft.ts";
import { formatScu } from "../../src/features/contracts/scu.ts";
import type { ContractDetail, RequirementDetail, StopDetail } from "../../src/features/contracts/types.ts";

/* ---------- fixture builders (synthetic, not seed data) ---------- */

let nextId = 1000;
const stop = (locationName: string, scu: number | null = null): StopDetail => ({
  id: ++nextId,
  locationName,
  quantityScuMilli: scu === null ? null : scu * 1000,
  reminderDoneAt: null,
});
const req = (
  commodityName: string,
  scu: number,
  pickups: StopDetail[],
  deliveries: StopDetail[],
  extra: Partial<RequirementDetail> = {},
): RequirementDetail => ({
  id: ++nextId,
  commodityName,
  quantityScuMilli: scu * 1000,
  maxBoxSizeScuMilli: null,
  boxGroups: [],
  pickups,
  deliveries,
  ...extra,
});
const contract = (id: number, title: string, acceptedAt: number, requirements: RequirementDetail[]): ContractDetail => ({
  id,
  run: { id: 1, runNumber: 1, runDate: "2026-11-19" },
  title,
  rewardAuec: 10_000,
  status: "active",
  source: "manual",
  acceptedAt,
  finishedAt: null,
  totalScuMilli: requirements.reduce((s, r) => s + r.quantityScuMilli, 0),
  requirements,
});

/** Example A: one pickup -> one destination. */
const exampleA = () =>
  contract(4, "Direct Small Cargo Haul", 4_000, [req("Stims", 12, [stop("Port Tressler")], [stop("Devlin Scrap & Salvage")])]);
/** Example B: two commodities, two pickups each (unknown split), one destination. */
const exampleB = () =>
  contract(2, "Small Cargo Haul", 2_000, [
    req("Waste", 4, [stop("microTech Logistics Depot S4LD01"), stop("Sakura Sun Goldenrod Workcenter")], [stop("Port Tressler")]),
    req("Scrap", 6, [stop("microTech Logistics Depot S4LD01"), stop("Sakura Sun Goldenrod Workcenter")], [stop("Port Tressler")]),
  ]);
/** Example C: one pickup -> two destinations with known split. */
const exampleC = () =>
  contract(3, "Extra Small Cargo Haul", 3_000, [
    req("Aluminum", 9, [stop("Port Tressler")], [
      stop("Sakura Sun Goldenrod Workcenter", 5),
      stop("Greycat Stanton IV Production Complex-A", 4),
    ]),
  ]);

const all = () => [exampleA(), exampleB(), exampleC()];
const group = (groups: ReturnType<typeof groupByLocation>, name: string) => {
  const g = groups.find((x) => x.locationName === name);
  assert.ok(g, `missing location ${name}`);
  return g;
};

/* ---------- By Location ---------- */

test("several contracts at one location share a single group", () => {
  const groups = groupByLocation(all());
  const tressler = group(groups, "Port Tressler");
  assert.equal(tressler.contractCount, 3);
  assert.deepEqual(
    tressler.rows.map((r) => [r.operation, r.contractRef, r.commodityName]),
    [
      ["pickup", "C03", "Aluminum"],
      ["pickup", "C04", "Stims"],
      ["delivery", "C02", "Waste"],
      ["delivery", "C02", "Scrap"],
    ],
  );
});

test("a location with pickups and deliveries is one group with separate totals", () => {
  const tressler = group(groupByLocation(all()), "Port Tressler");
  assert.equal(tressler.operationCount, 4);
  // Single-location sides are derived without ambiguity; never pickup + delivery summed.
  assert.deepEqual(tressler.pickup, { scuMilli: 21_000, complete: true, rows: 2 });
  assert.deepEqual(tressler.delivery, { scuMilli: 10_000, complete: true, rows: 2 });
});

test("single pickup with multiple destinations", () => {
  const groups = groupByLocation([exampleC()]);
  const pickup = group(groups, "Port Tressler").rows[0];
  assert.deepEqual(pickup.quantity, { kind: "derived", scuMilli: 9_000 });
  assert.deepEqual(pickup.oppositeLocations, ["Sakura Sun Goldenrod Workcenter", "Greycat Stanton IV Production Complex-A"]);
  assert.deepEqual(group(groups, "Sakura Sun Goldenrod Workcenter").rows[0].quantity, { kind: "known", scuMilli: 5_000 });
  assert.deepEqual(group(groups, "Greycat Stanton IV Production Complex-A").rows[0].quantity, { kind: "known", scuMilli: 4_000 });
});

test("multiple pickups with one destination keep unknown quantities unknown", () => {
  const groups = groupByLocation([exampleB()]);
  const depot = group(groups, "microTech Logistics Depot S4LD01");
  assert.deepEqual(depot.rows.map((r) => r.quantity.kind), ["unknown", "unknown"]);
  // The requirement total stays available for row details, but is not assigned.
  assert.deepEqual(depot.rows.map((r) => r.requirementScuMilli), [4_000, 6_000]);
  assert.deepEqual(depot.pickup, { scuMilli: 0, complete: false, rows: 2 });
  const tressler = group(groups, "Port Tressler");
  assert.deepEqual(tressler.rows.map((r) => r.quantity), [
    { kind: "derived", scuMilli: 4_000 },
    { kind: "derived", scuMilli: 6_000 },
  ]);
});

test("partially known allocations are a lower bound, not invented", () => {
  const c = contract(7, "Partial", 7_000, [req("Gold", 10, [stop("A", 4), stop("B")], [stop("Z")])]);
  const groups = groupByLocation([c]);
  assert.deepEqual(group(groups, "A").pickup, { scuMilli: 4_000, complete: true, rows: 1 });
  assert.deepEqual(group(groups, "B").rows[0].quantity, { kind: "unknown" });
  assert.deepEqual(group(groups, "B").pickup, { scuMilli: 0, complete: false, rows: 1 });
});

test("distinct requirements with identical commodity and quantity are never merged", () => {
  const c = contract(8, "Twins", 8_000, [
    req("Gold", 5, [stop("Hub")], [stop("Dest 1")]),
    req("Gold", 5, [stop("Hub")], [stop("Dest 2")]),
  ]);
  const hub = group(groupByLocation([c]), "Hub");
  assert.equal(hub.rows.length, 2);
  assert.notEqual(hub.rows[0].requirementId, hub.rows[1].requirementId);
  assert.notEqual(hub.rows[0].key, hub.rows[1].key);
});

test("multiple commodities per contract each produce their own rows", () => {
  const groups = groupByLocation([exampleB()]);
  const goldenrod = group(groups, "Sakura Sun Goldenrod Workcenter");
  assert.deepEqual(goldenrod.rows.map((r) => r.commodityName), ["Waste", "Scrap"]);
  assert.equal(goldenrod.contractCount, 1);
});

test("location names are grouped case- and whitespace-insensitively", () => {
  const a = contract(1, "One", 1, [req("X", 1, [stop("Port  Tressler")], [stop("B")])]);
  const b = contract(2, "Two", 2, [req("Y", 1, [stop("port tressler ")], [stop("C")])]);
  const groups = groupByLocation([b, a]);
  assert.equal(groups.length, 3);
  const tressler = groups.find((g) => g.key === "location:port tressler");
  assert.ok(tressler);
  assert.equal(tressler.locationName, "Port  Tressler"); // spelling of the oldest contract
  assert.equal(tressler.rows.length, 2);
});

test("pickup locations come first, each part alphabetical, rows deterministic regardless of input order", () => {
  const forward = groupByLocation(all());
  const reversed = groupByLocation([...all()].reverse());
  const alpha = (xs: string[]) => [...xs].sort((x, y) => x.localeCompare(y, "en", { sensitivity: "base" }));
  const pickupFirst = forward.filter((g) => g.hasPickup).map((g) => g.locationName);
  const deliveryOnly = forward.filter((g) => !g.hasPickup).map((g) => g.locationName);
  assert.deepEqual(forward.map((g) => g.locationName), [...pickupFirst, ...deliveryOnly]);
  assert.deepEqual(pickupFirst, alpha(pickupFirst));
  assert.deepEqual(deliveryOnly, alpha(deliveryOnly));
  assert.deepEqual(pickupFirst, ["microTech Logistics Depot S4LD01", "Port Tressler", "Sakura Sun Goldenrod Workcenter"]);
  // Ignore stop ids (fresh fixtures); compare structure.
  const shape = (gs: typeof forward) => gs.map((g) => [g.locationName, g.rows.map((r) => `${r.operation}:${r.contractRef}:${r.commodityName}`)]);
  assert.deepEqual(shape(forward), shape(reversed));
  for (const g of forward) {
    const ops = g.rows.map((r) => r.operation);
    assert.deepEqual(ops, [...ops].sort((x, y) => (x === y ? 0 : x === "pickup" ? -1 : 1)), "pickups before deliveries");
  }
});

/* ---------- By Contract ---------- */

test("contract groups are newest first with requirement-only totals", () => {
  const groups = groupByContract(all());
  assert.deepEqual(groups.map((g) => g.contractRef), ["C04", "C03", "C02"]);
  const b = groups[2];
  assert.equal(b.totalScuMilli, 10_000);
  assert.deepEqual(b.objectives.map((o) => [o.commodityName, o.totalScuMilli, o.pickups.length, o.deliveries.length]), [
    ["Waste", 4_000, 2, 1],
    ["Scrap", 6_000, 2, 1],
  ]);
  assert.deepEqual(b.objectives[0].pickups.map((p) => p.quantity.kind), ["unknown", "unknown"]);
  assert.deepEqual(b.objectives[0].deliveries[0].quantity, { kind: "derived", scuMilli: 4_000 });
});

test("switching modes shows the same stops and never mutates records", () => {
  const input = all();
  const snapshot = structuredClone(input);
  const fromLocations = groupByLocation(input).flatMap((g) => g.rows.map((r) => r.stopId)).sort((a, b) => a - b);
  const fromContracts = groupByContract(input)
    .flatMap((g) => g.objectives.flatMap((o) => [...o.pickups, ...o.deliveries].map((s) => s.stopId)))
    .sort((a, b) => a - b);
  assert.deepEqual(fromLocations, fromContracts);
  assert.equal(new Set(fromLocations).size, fromLocations.length, "each stop appears exactly once");
  assert.deepEqual(input, snapshot);
});

test("empty input produces empty groups", () => {
  assert.deepEqual(groupByLocation([]), []);
  assert.deepEqual(groupByContract([]), []);
});

test("contract references derive from stable ids", () => {
  assert.equal(contractRef(1), "C01");
  assert.equal(contractRef(12), "C12");
  assert.equal(contractRef(123), "C123");
});

/* ---------- Box information ---------- */

test("box information uses stored data only", () => {
  assert.deepEqual(boxInfo([], null), { kind: "none" });
  assert.deepEqual(boxInfo([], 8_000), { kind: "max", maxBoxSizeScuMilli: 8_000 });
  const groups = [
    { boxSizeScuMilli: 1_000, boxCount: 2, sourceQuality: "exact" as const },
    { boxSizeScuMilli: 4_000, boxCount: 1, sourceQuality: "exact" as const },
  ];
  assert.deepEqual(boxInfo(groups, 4_000), { kind: "breakdown", groups: [groups[1], groups[0]], estimated: false });
  assert.equal((boxInfo([{ ...groups[0], sourceQuality: "estimated" }], null) as { estimated: boolean }).estimated, true);
});

/* ---------- Expansion ---------- */

test("expansion state toggles per group and falls back to the default", () => {
  let state = new Map<string, boolean>();
  assert.equal(isExpanded(state, "a", true), true);
  state = new Map(toggleExpanded(state, "a", true));
  assert.equal(isExpanded(state, "a", true), false);
  assert.equal(isExpanded(state, "b", true), true, "other groups unaffected");
  state = new Map(toggleExpanded(state, "a", true));
  assert.equal(isExpanded(state, "a", true), true);
  const collapsed = setAllExpanded(["a", "b"], false);
  assert.equal(isExpanded(collapsed, "a", true), false);
  assert.equal(isExpanded(collapsed, "b", true), false);
});

/* ---------- Contract creation form (regression) ---------- */

test("Add Contract draft still validates and converts to the IPC payload", () => {
  assert.ok(validateDraft(emptyDraft()).length > 0);
  const draft = emptyDraft();
  draft.title = "Extra Small Cargo Haul";
  draft.rewardAuec = "9,000";
  const r = draft.requirements[0];
  r.commodityName = "Aluminum";
  r.quantityScu = "9";
  r.pickups[0].locationName = "Port Tressler";
  r.deliveries = [
    { ...emptyStop(), locationName: "Sakura Sun Goldenrod Workcenter", quantityScu: "5" },
    { ...emptyStop(), locationName: "Greycat Stanton IV Production Complex-A", quantityScu: "4" },
  ];
  assert.deepEqual(validateDraft(draft), []);
  const input = toInput(draft);
  assert.equal(input.requirements[0].pickups[0].quantityScu, null);
  assert.equal(input.requirements[0].deliveries[1].quantityScu, "4");

  r.deliveries[1].quantityScu = "3";
  assert.deepEqual(validateDraft(draft).map((i) => i.path), ["requirements.0.deliveries"]);

  draft.requirements.push(emptyRequirement());
  assert.ok(validateDraft(draft).some((i) => i.path === "requirements.1.commodityName"));
  assert.equal(formatScu(9_000), "9");
});

/* ---------- Task 04.1: manifest aggregation ---------- */

import { aggregateRows, isCombinedKey } from "../../src/features/contracts/grouping.ts";
import {
  buildOptions,
  fromNames,
  fromReferenceCommodity,
  fromReferenceLocation,
  fromUserLocations,
  humanizeGroup,
  matchTier,
  MAX_OPTIONS,
  mergeSuggestions,
  moveActive,
  normalizeName,
  rankSuggestions,
} from "../../src/features/contracts/catalogSearch.ts";

/** C02: Aluminum 13 Tressler -> NB, Aluminum 15 Tressler -> Sakura. */
const c02 = () =>
  contract(2, "Aluminum split", 2_000, [
    req("Aluminum", 13, [stop("Port Tressler")], [stop("New Babbage")]),
    req("Aluminum", 15, [stop("Port Tressler")], [stop("Sakura Sun Goldenrod Workcenter")]),
  ]);

test("same-contract duplicate pickups combine into one manifest line with original parts", () => {
  const tressler = group(groupByLocation([c02()]), "Port Tressler");
  assert.equal(tressler.rows.length, 2, "underlying stops are preserved");
  assert.equal(tressler.entries.length, 1);
  const [line] = tressler.entries;
  assert.equal(line.contractRef, "C02");
  assert.equal(line.commodityName, "Aluminum");
  assert.deepEqual(line.quantity, { kind: "derived", scuMilli: 28_000 });
  assert.deepEqual(line.oppositeLocations, ["New Babbage", "Sakura Sun Goldenrod Workcenter"]);
  assert.deepEqual(line.parts.map((p) => p.requirementId), tressler.rows.map((r) => r.requirementId));
  assert.equal(new Set(line.parts.map((p) => p.stopId)).size, 2);
  assert.ok(isCombinedKey(line.key));
  // Group total equals the combined line: nothing counted twice.
  assert.deepEqual(tressler.pickup, { scuMilli: 28_000, complete: true, rows: 2 });
});

test("identical commodity in different contracts stays on separate lines", () => {
  const other = contract(5, "Other", 5_000, [req("Aluminum", 10, [stop("Port Tressler")], [stop("Area18")])]);
  const tressler = group(groupByLocation([c02(), other]), "Port Tressler");
  assert.deepEqual(
    tressler.entries.map((e) => [e.contractRef, e.quantity.kind === "unknown" ? null : e.quantity.scuMilli]),
    [
      ["C02", 28_000],
      ["C05", 10_000],
    ],
  );
});

test("unknown quantities are never summed into a combined line", () => {
  const c = contract(6, "Mixed", 6_000, [
    req("Gold", 4, [stop("Hub"), stop("Elsewhere")], [stop("Dest")]), // Hub share unknown
    req("Gold", 6, [stop("Hub")], [stop("Dest 2")]), // Hub derived 6
    req("Gold", 2, [stop("Hub", 2)], [stop("Dest 3")]), // Hub known 2
  ]);
  const hub = group(groupByLocation([c]), "Hub");
  assert.equal(hub.rows.length, 3);
  assert.deepEqual(
    hub.entries.map((e) => [e.parts.length, e.quantity]),
    [
      [1, { kind: "unknown" }],
      [2, { kind: "derived", scuMilli: 8_000 }],
    ],
  );
  assert.deepEqual(hub.pickup, { scuMilli: 8_000, complete: false, rows: 3 });
});

test("mixed pickup and delivery at one location: pickups first, never combined across operations", () => {
  const a = contract(1, "In", 1_000, [req("Waste", 4, [stop("Area18")], [stop("Port Tressler")])]);
  const b = contract(1, "Out", 1_000, [req("Waste", 3, [stop("Port Tressler")], [stop("Area18")])]);
  b.id = 1; // same contract id on purpose: operations must still stay apart
  const tressler = group(groupByLocation([{ ...a, requirements: [...a.requirements, ...b.requirements] }]), "Port Tressler");
  assert.deepEqual(tressler.entries.map((e) => e.operation), ["pickup", "delivery"]);
  assert.ok(tressler.hasPickup);
});

test("aggregateRows keeps single rows unchanged and is order-preserving", () => {
  const rows = group(groupByLocation([exampleC()]), "Port Tressler").rows;
  const entries = aggregateRows(rows, "location:port tressler");
  assert.equal(entries.length, 1);
  assert.equal(entries[0].key, rows[0].key);
  assert.deepEqual(entries[0].parts, [rows[0]]);
});

/* ---------- Task 04.1 / 04.2: picker suggestions and search ---------- */

const sugg = (names: string[]) => fromNames(names);
const ranked = (names: string[], q: string) => rankSuggestions(sugg(names), q).map((s) => s.name);

test("bundled list merges with user entries without duplicates; bundled spelling wins", () => {
  const bundled = [
    { name: "Covalex Distribution Center S4DC05", system: "Stanton", body: "microTech" },
    { name: "Port Tressler", system: "Stanton", body: "microTech" },
    { name: "Area18", system: "Stanton", body: null },
  ].map(fromReferenceLocation);
  const user = fromUserLocations([
    { id: 1, name: "port  tressler" },
    { id: 2, name: "NB" },
  ]);
  const form = fromNames(["  New   Babbage ", "PORT TRESSLER", ""]);
  const merged = mergeSuggestions(bundled, user, form);
  assert.deepEqual(
    merged.map((s) => [s.name, s.detail]),
    [
      ["Covalex Distribution Center S4DC05", "microTech · Stanton"],
      ["Port Tressler", "microTech · Stanton"],
      ["Area18", "Stanton"],
      ["NB", null],
      ["New Babbage", null],
    ],
  );
  assert.equal(normalizeName("  Seraphim   Station "), "Seraphim Station");
});

test("commodity suggestions show the humanized group", () => {
  const merged = mergeSuggestions(
    [
      { name: "Aluminum", group: "Metal" },
      { name: "Processed Food", group: "ProcessedGoods" },
      { name: "Quartz", group: null },
    ].map(fromReferenceCommodity),
    fromNames(["aluminum", "Waste"]),
  );
  assert.deepEqual(
    merged.map((s) => [s.name, s.detail]),
    [
      ["Aluminum", "Metal"],
      ["Processed Food", "Processed Goods"],
      ["Quartz", null],
      ["Waste", null],
    ],
  );
  assert.equal(humanizeGroup("Raw_Minerals"), "Raw Minerals");
});

test("search is case-insensitive with prefix matches first", () => {
  const names = ["Area18", "Everus Harbor", "Port Tressler", "Seraphim Station"];
  assert.deepEqual(ranked(names, "TRESS"), ["Port Tressler"]);
  assert.deepEqual(ranked(names, "s"), ["Seraphim Station", "Everus Harbor", "Port Tressler"]);
  assert.deepEqual(ranked(names, ""), names);
  assert.deepEqual(ranked(names, "zzz"), []);
});

test("ranking: starts with, then word start, then contains; alphabetical within each", () => {
  const names = ["Babbage Depot", "New Babbage", "Babbage", "Grabbabbage", "Port Tressler", "Abbey Babbage Hub"];
  assert.deepEqual(ranked(names, "babbage"), ["Babbage", "Babbage Depot", "Abbey Babbage Hub", "New Babbage", "Grabbabbage"]);
  assert.equal(matchTier("Port Tressler", "tress"), 2);
  assert.equal(matchTier("New Babbage", "babb"), 2);
  assert.equal(matchTier("ARC-L1 Wide Forest Station", "l1"), 2, "hyphen starts a word");
  assert.equal(matchTier("Port Tressler", "essl"), 3);
  assert.equal(matchTier("Port Tressler", "port"), 1);
  assert.equal(matchTier("Port Tressler", "xyz"), null);
});

test("options offer Add new only when no exact match exists", () => {
  const s = sugg(["Port Tressler"]);
  const names = (opts: ReturnType<typeof buildOptions>) =>
    opts.map((o) => (o.kind === "existing" ? ["existing", o.suggestion.name] : ["create", o.name]));
  assert.deepEqual(names(buildOptions(s, "port   tressler")), [["existing", "Port Tressler"]]);
  assert.deepEqual(names(buildOptions(s, " HUR-L1 ")), [["create", "HUR-L1"]]);
  assert.deepEqual(names(buildOptions(s, "Port")), [
    ["existing", "Port Tressler"],
    ["create", "Port"],
  ]);
  assert.deepEqual(buildOptions([], ""), [], "no-results state");
});

test("results are capped at 50", () => {
  const many = Array.from({ length: 120 }, (_, i) => `Station ${String(i).padStart(3, "0")}`);
  assert.equal(MAX_OPTIONS, 50);
  assert.equal(rankSuggestions(sugg(many), "station").length, 50);
  assert.equal(rankSuggestions(sugg(many), "").length, 50);
  assert.equal(buildOptions(sugg(many), "sta").length, 51, "50 matches plus the Add new option");
});

test("arrow-key navigation wraps and handles empty lists", () => {
  assert.equal(moveActive(-1, 3, 1), 0);
  assert.equal(moveActive(-1, 3, -1), 2);
  assert.equal(moveActive(2, 3, 1), 0);
  assert.equal(moveActive(0, 3, -1), 2);
  assert.equal(moveActive(1, 3, 1), 2);
  assert.equal(moveActive(-1, 0, 1), -1);
});

test("form validation treats whitespace variants of one location as duplicates", () => {
  const draft = emptyDraft();
  draft.title = "T";
  draft.rewardAuec = "1";
  const r = draft.requirements[0];
  r.commodityName = "Gold";
  r.quantityScu = "2";
  r.pickups = [
    { ...emptyStop(), locationName: "Port Tressler" },
    { ...emptyStop(), locationName: "port   TRESSLER" },
  ];
  r.deliveries[0].locationName = "Area18";
  assert.deepEqual(validateDraft(draft).map((i) => i.path), ["requirements.0.pickups.1.locationName"]);
});

/* ---------- Task 04.3: summary row, empty-query ordering, sorting ---------- */

import { capacityState, formatPercent, summarizeContracts, summaryForList } from "../../src/features/contracts/summary.ts";
import { compareNames, sortKey } from "../../src/features/contracts/catalogSearch.ts";

test("summary counts active contracts and sums requirement totals only", () => {
  // Example B: two requirements, two pickups each, one destination → 10 SCU, not 4+6 per stop.
  const b = exampleB();
  // Example C: one pickup, two destinations with known split 5 + 4 → 9 SCU, not 18.
  const c = exampleC();
  // Fractional SCU: 7.5 + 0.125.
  const frac = contract(9, "Fractional", 9_000, [
    req("Quantainium", 7.5, [stop("HUR-L1", 2.25), stop("HUR-L2")], [stop("Lorville"), stop("Area18")]),
    req("Gold", 0.125, [stop("A")], [stop("B")]),
  ]);
  frac.totalScuMilli = frac.requirements.reduce((s, r) => s + r.quantityScuMilli, 0);
  const done = { ...exampleA(), status: "completed" as const };
  const summary = summarizeContracts([b, c, frac, done]);
  assert.equal(summary.activeCount, 3, "completed contracts are not counted");
  assert.equal(summary.totalScuMilli, 10_000 + 9_000 + 7_625);
  assert.equal(formatScu(summary.totalScuMilli), "26.625");
});

test("summary formatting: whole numbers without decimals, up to 3 decimals when needed", () => {
  assert.equal(formatScu(0), "0");
  assert.equal(formatScu(12_000), "12");
  assert.equal(formatScu(4_500), "4.5");
  assert.equal(formatScu(125), "0.125");
  assert.equal(formatScu(4_600_000), "4,600");
});

test("summary is empty-zero for no contracts and pending while loading or on error", () => {
  assert.deepEqual(summaryForList({ status: "ready", contracts: [] }), { activeCount: 0, totalScuMilli: 0 });
  assert.equal(summaryForList({ status: "loading" }), null);
  assert.equal(summaryForList({ status: "error" }), null);
});

test("capacity percentages: unknown, 0 %, 50 %, exactly 100 %, over 100 %", () => {
  assert.deepEqual(capacityState(12_000, null), { kind: "unknown" });
  assert.deepEqual(capacityState(12_000, 0), { kind: "unknown" }, "zero capacity is treated as unknown");
  assert.deepEqual(capacityState(0, 96), { kind: "known", percent: 0, fill: 0, over: false });
  assert.deepEqual(capacityState(48_000, 96), { kind: "known", percent: 50, fill: 0.5, over: false });
  assert.deepEqual(capacityState(96_000, 96), { kind: "known", percent: 100, fill: 1, over: false });
  const over = capacityState(107_520, 96);
  assert.equal(over.kind, "known");
  if (over.kind === "known") {
    assert.equal(over.over, true);
    assert.equal(over.fill, 1, "bar drawn full");
    assert.equal(formatPercent(over.percent), "112 %", "real percentage shown");
  }
  assert.equal(formatPercent(0), "0 %");
  assert.equal(formatPercent(0.4), "<1 %");
  assert.equal(formatPercent(50), "50 %");
  assert.equal(formatPercent(100), "100 %");
  assert.equal(formatPercent(99.6), "99 %", "never rounds up to a full 100 %");
  assert.equal(formatPercent(100.3), "101 %", "an overload never shows as 100 %");
  // Large capacities (e.g. 4,600 SCU) with fractional loads.
  const big = capacityState(2_300_500, 4_600);
  assert.ok(big.kind === "known" && Math.abs(big.percent - 50.0108695) < 1e-6);
});

test("empty search lists already-used entries first; typing keeps the 04.2 ranking", () => {
  const bundled = ["Area18", "Port Tressler", "Seraphim Station", "Everus Harbor"].map((name) =>
    fromReferenceLocation({ name, system: "Stanton", body: null }),
  );
  const merged = mergeSuggestions(
    bundled,
    fromUserLocations([
      { id: 1, name: "seraphim station" }, // also bundled: keeps bundled spelling, counts as used
      { id: 2, name: "NB" },
    ]),
    fromNames(["Lorville"]), // typed in this form only: not "used"
  );
  assert.deepEqual(rankSuggestions(merged, "").map((s) => s.name), [
    "NB",
    "Seraphim Station",
    "Area18",
    "Everus Harbor",
    "Lorville",
    "Port Tressler",
  ]);
  assert.deepEqual(rankSuggestions(merged, "s").map((s) => s.name), ["Seraphim Station", "Everus Harbor", "Port Tressler"]);
  const commodities = mergeSuggestions(
    [{ name: "Aluminum", group: "Metal" }, { name: "Waste", group: "Waste" }].map(fromReferenceCommodity),
    fromNames(["Stims", "waste"], true),
  );
  assert.deepEqual(rankSuggestions(commodities, "  ").map((s) => s.name), ["Stims", "Waste", "Aluminum"]);
});

test("alphabetical sorting ignores leading punctuation and quotes", () => {
  assert.equal(sortKey('"The Orphanage"'), 'The Orphanage"');
  const names = ['"The Orphanage"', "Area18", "Teasa Spaceport", "The Grove", "'t Hoekje", "Zephyr"];
  assert.deepEqual([...names].sort(compareNames), ["Area18", "'t Hoekje", "Teasa Spaceport", "The Grove", '"The Orphanage"', "Zephyr"]);
  assert.deepEqual(ranked(['"The Orphanage"', "Area18", "Zephyr"], ""), ["Area18", '"The Orphanage"', "Zephyr"]);
});

/* ---------- Task 05: complete / abandon ---------- */

import {
  bulkConfirmation,
  pruneExpansion,
  singleConfirmation,
  statusFor,
  successNotice,
  validExpansionKeys,
} from "../../src/features/contracts/finalize.ts";

test("single confirmation shows reference, title, SCU and reward", () => {
  const b = { ...exampleB(), rewardAuec: 24_250 };
  const complete = singleConfirmation(b, "complete");
  assert.equal(complete.heading, "Mark C02 complete?");
  assert.equal(complete.title, "Small Cargo Haul");
  assert.equal(complete.totalScuMilli, 10_000);
  assert.equal(complete.rewardAuec, 24_250);
  assert.deepEqual(complete.ids, [2]);
  assert.deepEqual(complete.refs, ["C02"]);
  assert.equal(complete.danger, false);
  assert.match(complete.note, /leave the active list and move to history/);
  const abandon = singleConfirmation(b, "abandon");
  assert.equal(abandon.heading, "Abandon C02?");
  assert.equal(abandon.danger, true, "abandon uses danger styling");
  assert.equal(statusFor("complete"), "completed");
  assert.equal(statusFor("abandon"), "abandoned");
});

test("bulk confirmation lists the shown contracts and sends exactly their ids", () => {
  const shown = [exampleC(), exampleA(), exampleB()]; // ids 3, 4, 2 (SCU 9, 12, 10)
  const bulk = bulkConfirmation(shown, "abandon");
  assert.equal(bulk.heading, "Abandon 3 active contracts?");
  assert.equal(bulk.count, 3);
  assert.deepEqual(bulk.ids, [2, 3, 4]);
  assert.deepEqual(bulk.refs, ["C02", "C03", "C04"]);
  assert.equal(bulk.totalScuMilli, 31_000);
  assert.equal(bulk.rewardAuec, 30_000);
  assert.equal(bulk.title, null);
  assert.equal(bulk.danger, true);
  // A contract added after the dialog opened is not part of the request.
  const later = contract(8, "Added later", 9_000, [req("Gold", 1, [stop("A")], [stop("B")])]);
  assert.ok(!bulk.ids.includes(later.id));
  assert.deepEqual(bulkConfirmation([exampleA()], "complete").heading, "Complete 1 active contract?");
});

test("success notice text", () => {
  assert.equal(successNotice(singleConfirmation(exampleB(), "complete")), "C02 completed");
  assert.equal(successNotice(bulkConfirmation(all(), "abandon")), "3 contracts abandoned");
});

test("expansion state is cleaned up for contracts that are gone", () => {
  const before = all();
  let state = new Map<string, boolean>([
    ["contract:2", true],
    ["contract:4", true],
    ["location:port tressler", false],
    ["location:devlin scrap & salvage", false],
  ]);
  const remaining = before.filter((c) => c.id !== 4); // C04 (only stop at Devlin) finished
  const valid = validExpansionKeys(groupByLocation(remaining), groupByContract(remaining));
  const pruned = pruneExpansion(state, valid);
  assert.deepEqual([...pruned.keys()].sort(), ["contract:2", "location:port tressler"]);
  assert.equal(pruneExpansion(pruned, valid), pruned, "unchanged state keeps its identity");
  state = new Map();
  assert.equal(pruneExpansion(state, valid), state);
});

test("summary recalculates after contracts are removed", () => {
  const before = summaryForList({ status: "ready", contracts: all() });
  assert.deepEqual(before, { activeCount: 3, totalScuMilli: 31_000 });
  const after = summaryForList({ status: "ready", contracts: all().filter((c) => c.id !== 2) });
  assert.deepEqual(after, { activeCount: 2, totalScuMilli: 21_000 });
  assert.deepEqual(summaryForList({ status: "ready", contracts: [] }), { activeCount: 0, totalScuMilli: 0 });
});

/* ---------- Task 05.1: edit contracts and reminder ticks ---------- */

import { draftFromContract, isDraftDirty, scuToInput } from "../../src/features/contracts/draft.ts";
import { parseRewardAuec, parseScuMilli } from "../../src/features/contracts/scu.ts";
import {
  applyReminders,
  captureReminders,
  nextDone,
  restoreReminders,
  tickState,
} from "../../src/features/contracts/reminders.ts";

/** Fractional, large, unknown and known quantities, plus a max box size. */
const editable = () => {
  const c = contract(5, "Edit me", 5_000, [
    req("Quantainium", 7.5, [stop("HUR-L1", 2.25), stop("HUR-L2")], [stop("Lorville"), stop("Area18")], { maxBoxSizeScuMilli: 2_000 }),
    req("Gold", 4_600, [stop("Port Tressler")], [stop("New Babbage", 4_600)]),
    req("Waste", 0.125, [stop("A")], [stop("B")]),
  ]);
  c.rewardAuec = 1_234_567;
  return c;
};

test("scuToInput produces parser-safe text (no thousands separator)", () => {
  assert.equal(scuToInput(12_000), "12");
  assert.equal(scuToInput(4_500), "4.5");
  assert.equal(scuToInput(125), "0.125");
  assert.equal(scuToInput(4_600_000), "4600", "never '4,600' (a comma is a decimal comma)");
  for (const milli of [0, 1, 999, 1_000, 2_250, 7_500, 4_600_000, 1_000_000_000]) {
    const parsed = parseScuMilli(scuToInput(milli));
    assert.ok(parsed.ok && parsed.value === milli, `round-trip ${milli}`);
  }
});

test("contract -> draft -> payload reproduces the stored values", () => {
  const c = editable();
  const draft = draftFromContract(c);
  assert.deepEqual(validateDraft(draft), [], "a stored contract is valid as a draft");
  const input = toInput(draft);
  assert.equal(input.title, "Edit me");
  assert.equal(parseRewardAuec(input.rewardAuec).ok && (parseRewardAuec(input.rewardAuec) as { value: number }).value, 1_234_567);
  input.requirements.forEach((r, i) => {
    const stored = c.requirements[i];
    assert.equal(r.commodityName, stored.commodityName);
    assert.deepEqual(parseScuMilli(r.quantityScu), { ok: true, value: stored.quantityScuMilli });
    assert.equal(r.maxBoxSizeScu === null ? null : (parseScuMilli(r.maxBoxSizeScu) as { value: number }).value, stored.maxBoxSizeScuMilli);
    const sameStops = (sent: typeof r.pickups, kept: StopDetail[]) =>
      sent.map((s) => [s.locationName, s.quantityScu === null ? null : (parseScuMilli(s.quantityScu) as { value: number }).value]);
    assert.deepEqual(sameStops(r.pickups, stored.pickups), stored.pickups.map((s) => [s.locationName, s.quantityScuMilli]));
    assert.deepEqual(sameStops(r.deliveries, stored.deliveries), stored.deliveries.map((s) => [s.locationName, s.quantityScuMilli]));
  });
  // Converting twice gives the same payload (row keys differ, payload does not).
  assert.deepEqual(toInput(draftFromContract(c)), input);
});

test("dirty-state detection ignores row keys and reacts to real changes", () => {
  const c = editable();
  const baseline = draftFromContract(c);
  assert.equal(isDraftDirty(draftFromContract(c), baseline), false, "a fresh copy is not dirty");
  const titled = { ...baseline, title: "Changed" };
  assert.equal(isDraftDirty(titled, baseline), true);
  const removed = { ...baseline, requirements: baseline.requirements.slice(1) };
  assert.equal(isDraftDirty(removed, baseline), true);
  const qty = structuredClone(baseline);
  qty.requirements[0].pickups[1].quantityScu = "1";
  assert.equal(isDraftDirty(qty, baseline), true);
  // Add mode: an empty form is clean; typing anything makes it dirty.
  assert.equal(isDraftDirty(emptyDraft(), emptyDraft()), false);
  const typed = emptyDraft();
  typed.requirements[0].commodityName = "Gold";
  assert.equal(isDraftDirty(typed, emptyDraft()), true);
});

test("tick state of a row: none, mixed, all; clicking a mixed row ticks all", () => {
  assert.equal(tickState([null]), "none");
  assert.equal(tickState([null, null]), "none");
  assert.equal(tickState([5, null]), "mixed");
  assert.equal(tickState([5, 6]), "all");
  assert.equal(nextDone("none"), true);
  assert.equal(nextDone("mixed"), true);
  assert.equal(nextDone("all"), false);
  // Combined manifest line (C02 Aluminum 13 + 15 from Port Tressler) reflects both stops.
  const two = c02();
  const ids = two.requirements.map((r) => r.pickups[0].id);
  const half = applyReminders([two], [ids[0]], 100);
  const line = group(groupByLocation(half), "Port Tressler").entries[0];
  assert.equal(tickState(line.parts.map((p) => p.reminderDoneAt)), "mixed");
  const full = applyReminders(half, ids, 200);
  assert.deepEqual(full[0].requirements.map((r) => r.pickups[0].reminderDoneAt), [100, 200], "already-ticked stops keep their time");
  assert.equal(tickState(group(groupByLocation(full), "Port Tressler").entries[0].parts.map((p) => p.reminderDoneAt)), "all");
});

test("optimistic ticks roll back exactly on failure", () => {
  const start = [editable(), exampleB()];
  const ids = [start[0].requirements[0].pickups[0].id, start[1].requirements[1].deliveries[0].id];
  const preTicked = applyReminders(start, [ids[1]], 50);
  const snapshot = captureReminders(preTicked, ids);
  assert.deepEqual([...snapshot.values()], [null, 50]);
  const optimistic = applyReminders(preTicked, ids, 999);
  assert.deepEqual(captureReminders(optimistic, ids), new Map([[ids[0], 999], [ids[1], 50]]));
  const rolledBack = restoreReminders(optimistic, snapshot);
  assert.deepEqual(rolledBack, preTicked, "everything back as before");
  const unticked = applyReminders(preTicked, ids, null);
  assert.deepEqual([...captureReminders(unticked, ids).values()], [null, null]);
  assert.deepEqual(start[0].requirements[0].pickups[0].reminderDoneAt, null, "inputs never mutated");
});

test("ticks never change the summary or totals", () => {
  const contracts = all();
  const everyStop = contracts.flatMap((c) => c.requirements.flatMap((r) => [...r.pickups, ...r.deliveries].map((s) => s.id)));
  const ticked = applyReminders(contracts, everyStop, 123);
  assert.deepEqual(summaryForList({ status: "ready", contracts: ticked }), summaryForList({ status: "ready", contracts }));
  assert.deepEqual(
    groupByLocation(ticked).map((g) => [g.key, g.pickup, g.delivery, g.entries.map((e) => e.key)]),
    groupByLocation(contracts).map((g) => [g.key, g.pickup, g.delivery, g.entries.map((e) => e.key)]),
    "grouping, totals and order unchanged",
  );
});

/* ---------- Task 06 Part A: badge and checkbox are the same control ---------- */

import { reminderBadgeText, reminderIntent, reminderLabel } from "../../src/features/contracts/reminders.ts";

test("badge and checkbox resolve to the same next state and stop ids", () => {
  const line = (contracts: ContractDetail[]) => group(groupByLocation(contracts), "Port Tressler").entries[0];
  const two = c02(); // combined line: Aluminum 13 + 15 from Port Tressler
  const ids = two.requirements.map((r) => r.pickups[0].id);
  const cases: [ContractDetail[], string, boolean][] = [
    [[two], "none", true],
    [applyReminders([two], [ids[0]], 10), "mixed", true],
    [applyReminders([two], ids, 10), "all", false],
  ];
  for (const [contracts, state, done] of cases) {
    const entry = line(contracts);
    // Both controls get the same intent object computed once per row.
    const badge = reminderIntent(entry.parts);
    const checkbox = reminderIntent(entry.parts);
    assert.deepEqual(badge, checkbox);
    assert.equal(badge.state, state);
    assert.equal(badge.done, done, `${state}: next value`);
    assert.deepEqual(badge.stopIds, ids, "all stops of the combined row");
  }
  // By Contract: one stop per checkbox.
  const single = exampleA().requirements[0].deliveries[0];
  assert.deepEqual(reminderIntent([{ stopId: single.id, reminderDoneAt: null }]), { state: "none", done: true, stopIds: [single.id] });
});

test("badge text and shared accessible labels", () => {
  assert.equal(reminderBadgeText("pickup", "none"), "Pick up");
  assert.equal(reminderBadgeText("pickup", "mixed"), "Pick up", "mixed keeps the normal text");
  assert.equal(reminderBadgeText("pickup", "all"), "Picked up");
  assert.equal(reminderBadgeText("delivery", "none"), "Turn in");
  assert.equal(reminderBadgeText("delivery", "all"), "Turned in");
  assert.equal(reminderLabel("pickup", "none"), "Mark picked up");
  assert.equal(reminderLabel("pickup", "mixed"), "Mark picked up");
  assert.equal(reminderLabel("pickup", "all"), "Mark not picked up");
  assert.equal(reminderLabel("delivery", "none"), "Mark turned in");
  assert.equal(reminderLabel("delivery", "all"), "Mark not turned in");
});
