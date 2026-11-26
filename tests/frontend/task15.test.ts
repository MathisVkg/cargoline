// Frontend unit tests for Task 15: custom ships in the selector (groups, search, tag, dialog
// validation, delete text, labels) and aUEC per hour in the History run headings.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CUSTOM_NAME_MAX,
  catalogLine,
  customShipInput,
  customShipIssues,
  deleteCustomQuestion,
  shipLabel,
  shipOptions,
} from "../../src/features/ships/model.ts";
import type { ShipEntry } from "../../src/features/ships/types.ts";
import { formatDuration, formatRate, RATE_TOOLTIP, runTotals, runTotalsParts } from "../../src/features/history/model.ts";
import type { HistoryRun } from "../../src/features/history/types.ts";
import { capacityState, formatPercent } from "../../src/features/contracts/summary.ts";

const ship = (id: string, name: string, cargoScu: number, extra: Partial<ShipEntry> = {}): ShipEntry => ({
  id,
  name,
  manufacturer: extra.isCustom ? "Custom" : "Crusader Industries",
  manufacturerCode: extra.isCustom ? "" : "CRUS",
  cargoScu,
  maxContainerScu: extra.isCustom ? null : 32,
  isFavorite: false,
  isCustom: false,
  ...extra,
});

const SHIPS: ShipEntry[] = [
  ship("CRUS_Starlifter_C2", "C2 Hercules Starlifter", 696),
  ship("AEGS_Avenger", "Avenger Titan", 8, { isFavorite: true }),
  ship("custom:aa", "Retaliator Cargo", 74, { isCustom: true }),
  ship("custom:bb", "My Hauler", 120, { isCustom: true, isFavorite: true }),
  ship("custom:cc", "C2 Hercules Starlifter", 700, { isCustom: true }),
];

/* ------------------------------------------------------------ custom ships */

test("selector groups: Favourites, then Custom ships, then All ships, each alphabetical, once", () => {
  const { favorites, custom, others } = shipOptions(SHIPS, "");
  assert.deepEqual(favorites.map((s) => s.id), ["AEGS_Avenger", "custom:bb"], "a favourite custom ship is under Favourites");
  assert.deepEqual(custom.map((s) => s.id), ["custom:cc", "custom:aa"]);
  assert.deepEqual(others.map((s) => s.id), ["CRUS_Starlifter_C2"]);
  assert.equal(favorites.length + custom.length + others.length, SHIPS.length, "each ship appears once");
  // The same name as a catalog ship: both listed, told apart by the flag (the "Custom" tag).
  const twins = SHIPS.filter((s) => s.name === "C2 Hercules Starlifter").map((s) => s.isCustom);
  assert.deepEqual(twins.sort(), [false, true]);
});

test("search covers custom ships (by name, and by the word custom)", () => {
  assert.deepEqual(shipOptions(SHIPS, "retaliator").custom.map((s) => s.id), ["custom:aa"]);
  const c2 = shipOptions(SHIPS, "c2 hercules");
  assert.deepEqual([c2.custom.map((s) => s.id), c2.others.map((s) => s.id)], [["custom:cc"], ["CRUS_Starlifter_C2"]]);
  const all = shipOptions(SHIPS, "custom");
  assert.deepEqual([...all.favorites, ...all.custom].map((s) => s.id).sort(), ["custom:aa", "custom:bb", "custom:cc"]);
  assert.equal(all.others.length, 0);
});

test("dialog validation: messages for name and cargo", () => {
  const others = SHIPS.filter((s) => s.isCustom);
  assert.deepEqual(customShipIssues({ name: "Retaliator", cargo: "74" }, others), {});
  assert.deepEqual(customShipIssues({ name: "   ", cargo: "74" }, others), { name: "Enter a name." });
  assert.deepEqual(customShipIssues({ name: "x".repeat(CUSTOM_NAME_MAX + 1), cargo: "1" }, others), { name: "Use at most 60 characters." });
  assert.deepEqual(customShipIssues({ name: "x".repeat(CUSTOM_NAME_MAX), cargo: "1" }, others), {});
  assert.deepEqual(customShipIssues({ name: "  my   HAULER ", cargo: "5" }, others), { name: "You already have a custom ship with this name." });
  assert.deepEqual(customShipIssues({ name: "My Hauler", cargo: "150" }, others, "custom:bb"), {}, "editing keeps its own name");
  for (const cargo of ["", "0", "-3", "12.5", "100001", "abc"]) {
    assert.deepEqual(customShipIssues({ name: "New", cargo }, others), { cargoScu: "Enter a whole number of SCU from 1 to 100,000." }, cargo);
  }
  assert.deepEqual(customShipIssues({ name: "New", cargo: "100,000" }, others), {}, "thousands separators accepted");
  assert.deepEqual(customShipInput({ name: "  New  ", cargo: " 1,200 " }), { name: "New", cargoScu: 1200 });
  // A catalog ship with the same name is fine (only other custom ships count).
  assert.deepEqual(customShipIssues({ name: "Avenger Titan", cargo: "8" }, others), {});
});

test("delete confirmation, labels and the Settings line", () => {
  assert.equal(deleteCustomQuestion("Retaliator Cargo"), "Delete custom ship Retaliator Cargo? Contracts are not affected.");
  const custom = SHIPS[2];
  assert.equal(shipLabel(custom), "Retaliator Cargo · 74 SCU", "the Contracts / Cargo label");
  const state = capacityState(111_000, custom.cargoScu);
  assert.equal(state.kind === "known" && state.over, true);
  assert.equal(formatPercent(state.kind === "known" ? state.percent : 0), "150 %");
  const info = { count: 100, gameVersion: "4.10.3-LIVE.13165946", generatedAt: "2026-11-24", source: "Star Citizen Wiki API (api.star-citizen.wiki)" };
  assert.equal(catalogLine(info), "100 ships · game 4.10.3-LIVE.13165946 · Star Citizen Wiki");
  assert.equal(catalogLine(info, 0), "100 ships · game 4.10.3-LIVE.13165946 · Star Citizen Wiki");
  assert.equal(catalogLine(info, 3), "100 ships, 3 custom · game 4.10.3-LIVE.13165946 · Star Citizen Wiki");
});

/* ------------------------------------------------------------- aUEC per hour */

const MIN = 60_000;
const run = (extra: Partial<HistoryRun>): HistoryRun => ({
  runId: 1,
  runNumber: 3,
  runDate: "2026-11-20",
  completed: 16,
  abandoned: 5,
  completedScuMilli: 509_000,
  earnedAuec: 802_750,
  activeDurationMs: 135 * MIN,
  auecPerHour: 356_800,
  contracts: [],
  ...extra,
});

test("duration formatting", () => {
  assert.equal(formatDuration(45 * MIN), "45 min");
  assert.equal(formatDuration(135 * MIN), "2 h 15 min");
  assert.equal(formatDuration((26 * 60 + 5) * MIN), "26 h 5 min");
  assert.equal(formatDuration(120 * MIN), "2 h");
  assert.equal(formatDuration(30_000), "1 min", "rounded to the minute");
  assert.equal(formatDuration(0), "0 min");
});

test("run heading with and without a rate", () => {
  assert.equal(runTotals(run({})), "16 completed · 5 abandoned · 509 SCU · 802,750 aUEC · 2 h 15 min · ≈ 356,800 aUEC/h");
  const parts = runTotalsParts(run({}));
  assert.equal(parts.rate, "≈ 356,800 aUEC/h", "the rate is a separate part (it carries the tooltip)");
  assert.equal(RATE_TOOLTIP, "Estimate: completed rewards ÷ time with at least one contract running (accepted → finished).");
  assert.equal(
    runTotals(run({ completed: 1, abandoned: 0, earnedAuec: 20_000, completedScuMilli: 10_000, activeDurationMs: 5 * MIN, auecPerHour: null })),
    "1 completed · 10 SCU · 20,000 aUEC · 5 min",
    "under 10 minutes: duration, no rate",
  );
  assert.equal(runTotals(run({ activeDurationMs: 0, auecPerHour: null })), "16 completed · 5 abandoned · 509 SCU · 802,750 aUEC");
  assert.equal(formatRate(356_777.8), "≈ 356,800 aUEC/h", "rounded to the nearest 100");
});
