// Frontend unit tests for the Cargo page summary (Task 13).
import { test } from "node:test";
import assert from "node:assert/strict";
import { formatAmount, requirementCargo, summarizeCargo, type OperationSummary } from "../../src/features/cargo/summary.ts";
import { summarizeContracts } from "../../src/features/contracts/summary.ts";
import type { ContractDetail, RequirementDetail, StopDetail } from "../../src/features/contracts/types.ts";

let nextId = 1;
const stop = (locationName: string, quantityScuMilli: number | null = null, ticked = false): StopDetail => ({
  id: nextId++,
  locationName,
  quantityScuMilli,
  reminderDoneAt: ticked ? 1 : null,
});
const req = (commodityName: string, quantityScuMilli: number, pickups: StopDetail[], deliveries: StopDetail[], maxBox: number | null = null): RequirementDetail => ({
  id: nextId++,
  commodityName,
  quantityScuMilli,
  maxBoxSizeScuMilli: maxBox,
  boxGroups: [],
  pickups,
  deliveries,
});
const contract = (id: number, requirements: RequirementDetail[], status: ContractDetail["status"] = "active"): ContractDetail =>
  ({
    id,
    run: { id: 1, runNumber: 1, runDate: "2026-11-20" },
    title: `Contract ${id}`,
    rewardAuec: 1000,
    status,
    source: "manual",
    acceptedAt: id,
    finishedAt: status === "active" ? null : id + 1,
    totalScuMilli: requirements.reduce((s, r) => s + r.quantityScuMilli, 0),
    requirements,
  }) as ContractDetail;

const loc = (summary: OperationSummary, name: string) => summary.locations.find((l) => l.locationName === name)!;
const texts = (summary: OperationSummary) =>
  summary.locations.map((l) => [l.locationName, formatAmount(l.total), l.lines.map((x) => `${x.commodityName} ${formatAmount(x.amount)} ${x.contractRefs.join("·")}`)]);

test("amount wording", () => {
  assert.equal(formatAmount({ scuMilli: 84_000, complete: true, anyKnown: true }), "84 SCU");
  assert.equal(formatAmount({ scuMilli: 12_500, complete: false, anyKnown: true }), "12.5 SCU + ?");
  assert.equal(formatAmount({ scuMilli: 0, complete: false, anyKnown: false }), "? SCU");
  assert.equal(formatAmount({ scuMilli: 0, complete: true, anyKnown: false }), "0 SCU");
});

test("totals per operation with known, derived and unknown quantities", () => {
  const contracts = [
    // Derived: one pickup, one delivery → each handles the whole 84.
    contract(1, [req("Aluminum", 84_000, [stop("Port Tressler")], [stop("Baijini Point")])]),
    // Known split at pickup, unknown split at delivery (two deliveries, no quantities).
    contract(2, [req("Titanium", 30_000, [stop("Everus Harbor", 20_000), stop("Port Tressler", 10_000)], [stop("Seraphim Station"), stop("Baijini Point")])]),
  ];
  const s = summarizeCargo(contracts);
  assert.equal(formatAmount(s.pickup.total), "114 SCU");
  assert.equal(formatAmount(s.delivery.total), "84 SCU + ?");
  assert.equal(formatAmount(loc(s.delivery, "Seraphim Station").total), "? SCU");
  assert.equal(formatAmount(loc(s.delivery, "Baijini Point").total), "84 SCU + ?");
  // Never added together or to the active SCU (each requirement once).
  assert.equal(s.activeScuMilli, 114_000);
});

test("merging across contracts, not across commodities; sorting; refs", () => {
  const contracts = [
    // Different case / spacing still merges; the spelling of the lowest contract id is shown
    // (the By Location rule).
    contract(6, [req("aluminum ", 10_000, [stop("port  tressler")], [stop("Baijini Point")])]),
    contract(5, [req("Aluminum", 5_000, [stop("Port Tressler")], [stop("Baijini Point")])]),
    contract(7, [req("Gold", 2_000, [stop("Port Tressler")], [stop("Area18")])]),
    contract(8, [req("Agricium", 1_000, [stop("Everus Harbor")], [stop("Area18")])]),
  ];
  const s = summarizeCargo(contracts);
  assert.deepEqual(texts(s.pickup), [
    ["Everus Harbor", "1 SCU", ["Agricium 1 SCU C08"]],
    ["Port Tressler", "17 SCU", ["Aluminum 15 SCU C05·C06", "Gold 2 SCU C07"]],
  ]);
  assert.deepEqual(texts(s.delivery), [
    ["Area18", "3 SCU", ["Agricium 1 SCU C08", "Gold 2 SCU C07"]],
    ["Baijini Point", "15 SCU", ["Aluminum 15 SCU C05·C06"]],
  ]);
});

test("a merged line with an unknown part shows the known part + ?", () => {
  const contracts = [
    contract(1, [req("Aluminum", 10_000, [stop("Port Tressler")], [stop("A")])]),
    contract(2, [req("Aluminum", 8_000, [stop("Port Tressler"), stop("Everus Harbor")], [stop("A")])]),
  ];
  const line = loc(summarizeCargo(contracts).pickup, "Port Tressler").lines[0];
  assert.equal(formatAmount(line.amount), "10 SCU + ?", "never invents a split of the 8");
  assert.deepEqual(line.contractRefs, ["C01", "C02"]);
});

test("a location that is both a pickup and a delivery appears in both cards", () => {
  const contracts = [
    contract(1, [req("Aluminum", 10_000, [stop("Port Tressler")], [stop("Baijini Point")])]),
    contract(2, [req("Gold", 4_000, [stop("Baijini Point")], [stop("Port Tressler")])]),
  ];
  const s = summarizeCargo(contracts);
  assert.deepEqual(texts(s.pickup), [
    ["Baijini Point", "4 SCU", ["Gold 4 SCU C02"]],
    ["Port Tressler", "10 SCU", ["Aluminum 10 SCU C01"]],
  ]);
  assert.deepEqual(texts(s.delivery), [
    ["Baijini Point", "10 SCU", ["Aluminum 10 SCU C01"]],
    ["Port Tressler", "4 SCU", ["Gold 4 SCU C02"]],
  ]);
});

test("dimming: all ticked dims, partial and none do not; totals never change", () => {
  const build = (a: boolean, b: boolean) =>
    summarizeCargo([
      contract(1, [req("Aluminum", 10_000, [stop("Port Tressler", null, a)], [stop("X")])]),
      contract(2, [req("Aluminum", 6_000, [stop("Port Tressler", null, b)], [stop("X")])]),
    ]);
  const line = (a: boolean, b: boolean) => loc(build(a, b).pickup, "Port Tressler").lines[0];
  assert.equal(line(true, true).allTicked, true);
  assert.equal(line(true, false).allTicked, false);
  assert.equal(line(false, false).allTicked, false);
  for (const [a, b] of [[true, true], [true, false], [false, false]] as const) {
    const s = build(a, b);
    assert.equal(formatAmount(s.pickup.total), "16 SCU");
    assert.equal(s.activeScuMilli, 16_000);
  }
});

test("empty state and finished contracts ignored", () => {
  const empty = summarizeCargo([]);
  assert.equal(empty.pickup.locations.length, 0);
  assert.equal(empty.delivery.locations.length, 0);
  assert.equal(formatAmount(empty.pickup.total), "0 SCU");
  assert.equal(empty.activeScuMilli, 0);
  const finished = summarizeCargo([contract(1, [req("Gold", 1_000, [stop("A")], [stop("B")])], "completed")]);
  assert.equal(finished.pickup.locations.length, 0);
});

test("active SCU equals the Contracts summary for the same data", () => {
  const contracts = [
    contract(1, [req("Aluminum", 12_500, [stop("A"), stop("B")], [stop("C")]), req("Gold", 250, [stop("A")], [stop("C")])]),
    contract(2, [req("Titanium", 30_000, [stop("A", 10_000), stop("B", 20_000)], [stop("C")])]),
  ];
  assert.equal(summarizeCargo(contracts).activeScuMilli, summarizeContracts(contracts).totalScuMilli);
  assert.equal(summarizeCargo(contracts).activeScuMilli, 42_750, "milli-SCU integers");
});

test("per-requirement export for the Task 13.1 visual", () => {
  const r1 = req("Aluminum", 12_500, [stop("A"), stop("B")], [stop("C")], 4_000);
  const r2 = req("Gold", 250, [stop("A")], [stop("D")]);
  const out = requirementCargo([contract(3, [r2]), contract(2, [r1]), contract(9, [req("X", 1, [stop("Z")], [stop("Y")])], "abandoned")]);
  assert.deepEqual(out, [
    {
      key: `requirement:${r1.id}`,
      contractId: 2,
      contractRef: "C02",
      requirementId: r1.id,
      commodityName: "Aluminum",
      totalScuMilli: 12_500,
      maxBoxScuMilli: 4_000,
      pickupLocations: ["A", "B"],
      deliveryLocations: ["C"],
      deliveries: [{ locationName: "C", scuMilli: null }],
      boxGroups: [],
    },
    {
      key: `requirement:${r2.id}`,
      contractId: 3,
      contractRef: "C03",
      requirementId: r2.id,
      commodityName: "Gold",
      totalScuMilli: 250,
      maxBoxScuMilli: null,
      pickupLocations: ["A"],
      deliveryLocations: ["D"],
      deliveries: [{ locationName: "D", scuMilli: null }],
      boxGroups: [],
    },
  ]);
  assert.ok(out.every((r) => Number.isInteger(r.totalScuMilli)));
});
