// Frontend unit tests for the Cargo arrangement view (Tasks 13.1 and 13.2): estimation (contract
// only, never the ship), destinations, compact packing, platform sizing, the separate overflow
// platform, zoom, the block-mode safety net, colours, highlight and timings.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  BOX_PALETTE,
  CONTAINER_SIZES_SCU,
  SEVERAL_COLOR,
  SEVERAL_KEY,
  allowedMaxScu,
  colorKey,
  estimateBoxes,
  greedyBoxes,
  legend,
  type EstimatedBox,
} from "../../src/features/cargo/boxes.ts";
import {
  BLOCK_SAFETY_LIMIT,
  BOX_DIMENSIONS,
  MAX_LAYERS,
  MIN_DEPTH,
  PLATFORM_GAP,
  ZOOM_RATIO,
  dimensionsFor,
  isBehind,
  layoutCargo,
  platformFor,
  silhouettesOverlap,
  type Layout,
  type Placed,
} from "../../src/features/cargo/layout.ts";
import {
  NO_HIGHLIGHT,
  activeSelection,
  highlightedBoxes,
  highlightedLines,
  linesForBox,
  reduceHighlight,
} from "../../src/features/cargo/highlight.ts";
import { lineKey, type RequirementCargo } from "../../src/features/cargo/summary.ts";

let n = 0;
function req(partial: Partial<RequirementCargo> & { totalScuMilli: number }): RequirementCargo {
  n++;
  const deliveries = partial.deliveries ?? (partial.deliveryLocations ?? ["Baijini Point"]).map((locationName) => ({ locationName, scuMilli: null }));
  return {
    key: `requirement:${String(n).padStart(4, "0")}`,
    contractId: n,
    contractRef: `C${String(n).padStart(2, "0")}`,
    requirementId: n,
    commodityName: "Aluminum",
    maxBoxScuMilli: null,
    pickupLocations: ["Port Tressler"],
    deliveryLocations: deliveries.map((d) => d.locationName),
    deliveries,
    boxGroups: [],
    ...partial,
  };
}
const dest = (locationName: string) => [{ locationName, scuMilli: null }];
const sizes = (boxes: EstimatedBox[]) => boxes.map((b) => b.sizeScu);
const total = (boxes: EstimatedBox[]) => boxes.reduce((s, b) => s + b.scuMilli, 0);
const SHIPS = { "no ship": null, "600i": 44, Hermes: 288, "Hull B": 512, C2: 696, Ironclad: 2200 } as const;

/* ------------------------------------------------------------- estimation */

test("allowed maximum: the contract's max box, else 32 (never the ship)", () => {
  assert.deepEqual([...CONTAINER_SIZES_SCU], [1, 2, 4, 8, 16, 24, 32]);
  assert.equal(allowedMaxScu(null), 32, "none: 32");
  assert.equal(allowedMaxScu(8_000), 8, "8 → 8-SCU boxes");
  assert.equal(allowedMaxScu(12_000), 8, "largest standard size not above 12");
  assert.equal(allowedMaxScu(500), 1, "never below 1 SCU");
  assert.deepEqual(sizes(estimateBoxes([req({ totalScuMilli: 32_000, maxBoxScuMilli: 8_000 })])), [8, 8, 8, 8]);
  assert.deepEqual(sizes(estimateBoxes([req({ totalScuMilli: 64_000 })])), [32, 32]);
});

test("greedy fill, largest first; fractional remainder as one partial 1-SCU box", () => {
  assert.deepEqual(greedyBoxes(84_000, 32).map((b) => b.sizeScu), [32, 32, 16, 4]);
  assert.deepEqual(greedyBoxes(84_000, 8).map((b) => b.sizeScu), [8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 4]);
  assert.deepEqual(greedyBoxes(12_500, 32).map((b) => [b.sizeScu, b.scuMilli, b.partial]), [[8, 8_000, false], [4, 4_000, false], [1, 500, true]]);
  assert.deepEqual(greedyBoxes(250, 32).map((b) => [b.sizeScu, b.scuMilli, b.partial]), [[1, 250, true]]);
});

test("estimateBoxes: exact groups, totals preserved exactly, deterministic", () => {
  const r1 = req({ totalScuMilli: 84_000, maxBoxScuMilli: 8_000 });
  const r3 = req({ totalScuMilli: 12_500 });
  const r4 = req({ totalScuMilli: 30_000, boxGroups: [{ sizeScuMilli: 8_000, count: 3 }, { sizeScuMilli: 2_000, count: 3 }] });
  const r5 = req({ totalScuMilli: 20_000, boxGroups: [{ sizeScuMilli: 8_000, count: 2 }] });
  const boxes = estimateBoxes([r1, r3, r4, r5]);
  const of = (r: RequirementCargo) => boxes.filter((b) => b.requirementKey === r.key);
  assert.deepEqual(sizes(of(r1)), [8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 4]);
  assert.deepEqual(of(r4).map((b) => [b.sizeScu, b.quality]), [[8, "exact"], [8, "exact"], [8, "exact"], [2, "exact"], [2, "exact"], [2, "exact"]]);
  assert.deepEqual(of(r5).map((b) => [b.sizeScu, b.quality]), [[8, "exact"], [8, "exact"], [4, "estimated"]], "the rest is estimated, never dropped");
  for (const r of [r1, r3, r4, r5]) assert.equal(total(of(r)), r.totalScuMilli, `total of ${r.key}`);
  assert.equal(of(r3).filter((b) => b.partial).length, 1);
  assert.deepEqual(estimateBoxes([r1, r3]), estimateBoxes([r1, r3]), "deterministic");
});

test("several destinations: unknown split stays neutral; a known split assigns in order", () => {
  const unknown = req({ totalScuMilli: 40_000, deliveries: [{ locationName: "Area18", scuMilli: null }, { locationName: "Seraphim Station", scuMilli: null }] });
  const known = req({ totalScuMilli: 40_000, deliveries: [{ locationName: "Area18", scuMilli: 30_000 }, { locationName: "Seraphim Station", scuMilli: 10_000 }] });
  const partly = req({ totalScuMilli: 40_000, deliveries: [{ locationName: "Area18", scuMilli: 8_000 }, { locationName: "B", scuMilli: null }, { locationName: "C", scuMilli: null }] });
  const exactKnown = req({
    totalScuMilli: 40_000,
    deliveries: [{ locationName: "Area18", scuMilli: 32_000 }, { locationName: "Seraphim Station", scuMilli: 8_000 }],
    boxGroups: [{ sizeScuMilli: 8_000, count: 5 }],
  });
  const boxes = estimateBoxes([unknown, known, partly, exactKnown]);
  const of = (r: RequirementCargo) => boxes.filter((b) => b.requirementKey === r.key);
  assert.ok(of(unknown).every((b) => b.destination === null && b.destinations.join() === "Area18,Seraphim Station"));
  assert.equal(colorKey(of(unknown)[0], "destination"), SEVERAL_KEY);
  assert.deepEqual(of(known).map((b) => [b.destination, b.sizeScu]), [["Area18", 24], ["Area18", 4], ["Area18", 2], ["Seraphim Station", 8], ["Seraphim Station", 2]]);
  assert.deepEqual(of(partly).map((b) => [b.destination, b.scuMilli]), [["Area18", 8_000], [null, 32_000]]);
  assert.deepEqual(of(exactKnown).map((b) => b.destination), ["Area18", "Area18", "Area18", "Area18", "Seraphim Station"]);
  for (const r of [unknown, known, partly, exactKnown]) assert.equal(total(of(r)), 40_000);
});

/* ---------------------------------------------------------------- helpers */

const overlap3d = (a: Placed, b: Placed) =>
  a.x < b.x + b.w - 1e-9 && b.x < a.x + a.w - 1e-9 && a.y < b.y + b.d - 1e-9 && b.y < a.y + a.d - 1e-9 && a.z < b.z + b.h - 1e-9 && b.z < a.z + a.h - 1e-9;

/** Every invariant, on every platform. */
function assertValid(layout: Layout) {
  const items = layout.items;
  for (let i = 0; i < items.length; i++) for (let j = i + 1; j < items.length; j++) assert.ok(!overlap3d(items[i], items[j]), `overlap ${items[i].id} ${items[j].id}`);
  for (const p of items) {
    if (p.z > 0) {
      for (let x = p.x; x < p.x + p.w - 1e-9; x++)
        for (let y = p.y; y < p.y + p.d - 1e-9; y++) {
          const below = items.some((q) => q !== p && Math.abs(q.z + q.h - p.z) < 1e-9 && x >= q.x - 1e-9 && x < q.x + q.w - 1e-9 && y >= q.y && y < q.y + q.d);
          assert.ok(below, `floating ${p.id} at ${x},${y},${p.z}`);
        }
    }
    assert.ok(p.z + p.h <= MAX_LAYERS + 1e-9, `height ${p.id}`);
    const platform = layout.platforms.find((q) => q.kind === (p.overflow ? "overflow" : "main"));
    assert.ok(platform, `platform for ${p.id}`);
    assert.ok(p.x >= platform.x - 1e-9 && p.x + p.w <= platform.x + platform.width + 1e-9, `outside its platform (x) ${p.id}`);
    assert.ok(p.y >= 0 && p.y + p.d <= platform.depth + 1e-9, `outside its platform (y) ${p.id}`);
  }
  // Painter's order: whenever two silhouettes overlap, the one behind is drawn first.
  for (let i = 0; i < items.length; i++)
    for (let j = i + 1; j < items.length; j++)
      if (silhouettesOverlap(items[i], items[j])) assert.ok(!isBehind(items[j], items[i]) || isBehind(items[i], items[j]), `order ${items[i].id} / ${items[j].id}`);
  // Contiguous groups on each platform: their x-ranges never interleave.
  for (const kind of ["main", "overflow"] as const) {
    const ranges = new Map<string, [number, number]>();
    for (const p of items.filter((i) => (i.overflow ? "overflow" : "main") === kind)) {
      const r = ranges.get(p.colorKey) ?? [Infinity, -Infinity];
      ranges.set(p.colorKey, [Math.min(r[0], p.x), Math.max(r[1], p.x + p.w)]);
    }
    const sorted = [...ranges.values()].sort((a, b) => a[0] - b[0]);
    for (let i = 1; i < sorted.length; i++) assert.ok(sorted[i][0] >= sorted[i - 1][1] - 1e-9, `${kind}: groups interleave`);
  }
}

/** About 218 SCU of mixed cargo (the desktop scenario). */
function load218(): RequirementCargo[] {
  return [
    req({ totalScuMilli: 84_000, deliveries: dest("Baijini Point") }),
    req({ totalScuMilli: 60_000, commodityName: "Gold", maxBoxScuMilli: 8_000, deliveries: dest("Area18 TDD") }),
    req({ totalScuMilli: 42_000, commodityName: "Titanium", maxBoxScuMilli: 4_000, deliveries: dest("Seraphim Station") }),
    req({ totalScuMilli: 32_000, commodityName: "Agricium", deliveries: dest("Everus Harbor") }),
  ];
}
/** About 328 SCU, with an unknown split and a fractional requirement. */
function load328(): RequirementCargo[] {
  return [
    ...load218(),
    req({ totalScuMilli: 97_500, commodityName: "Laranite", deliveries: dest("Port Olisar") }),
    req({ totalScuMilli: 12_500, commodityName: "Medical Supplies", maxBoxScuMilli: 2_000, deliveries: [{ locationName: "Seraphim Station", scuMilli: null }, { locationName: "Port Tressler", scuMilli: null }] }),
  ];
}

const bbox = (items: Placed[]) => {
  const xs = items.map((p) => [p.x, p.x + p.w]).flat();
  const ys = items.map((p) => [p.y, p.y + p.d]).flat();
  return { x0: Math.min(...xs), y0: Math.min(...ys), width: Math.max(...xs) - Math.min(...xs), depth: Math.max(...ys) - Math.min(...ys) };
};

/* ------------------------------------------------- the ship never changes boxes */

test("identical boxes and mode with no ship, 600i, Hermes, Hull B, C2 and Ironclad", () => {
  for (const load of [load218(), load328()]) {
    const boxes = estimateBoxes(load);
    const reference = layoutCargo(boxes, { mode: "destination", shipCapacityScu: null });
    // The same boxes (id, container size, dimensions); a box may lie along x or y.
    const key = (l: Layout) => l.items.map((i) => `${i.box?.id}:${i.box?.sizeScu}:${[i.w, i.d].sort().join("x")}x${i.h}`).sort();
    for (const [name, capacity] of Object.entries(SHIPS)) {
      const layout = layoutCargo(boxes, { mode: "destination", shipCapacityScu: capacity });
      assert.equal(layout.mode, "boxes", `${name}: boxes, not blocks`);
      assert.deepEqual(key(layout), key(reference), `${name}: the same boxes`);
      assertValid(layout);
    }
  }
});

/* -------------------------------------------------------- compact packing */

test("compact footprint: near 2:1 or square, area close to what the cargo needs, no strip", () => {
  for (const load of [load218(), load328()]) {
    const boxes = estimateBoxes(load);
    const units = boxes.reduce((s, b) => s + b.sizeScu, 0);
    for (const [name, capacity] of Object.entries(SHIPS)) {
      const layout = layoutCargo(boxes, { mode: "destination", shipCapacityScu: capacity });
      const main = layout.items.filter((i) => !i.overflow);
      const b = bbox(main);
      const ratio = b.width / b.depth;
      assert.ok(ratio >= 0.75 && ratio <= 3, `${name}: footprint ${b.width}×${b.depth} is not a strip`);
      if (!layout.overCapacity) {
        const needed = units / MAX_LAYERS;
        assert.ok(b.width * b.depth <= 1.6 * needed + 16, `${name}: footprint ${b.width}×${b.depth} vs ${needed} needed`);
      }
      assert.deepEqual([b.x0, b.y0], [0, 0], `${name}: at the front-left corner`);
    }
  }
});

test("platform sizing: shapes readable from 1 to 4,608 SCU; depth minimum", () => {
  const shapes = [1, 44, 46, 288, 512, 696, 2200, 4608].map((u) => [u, platformFor(u)] as const);
  for (const [u, p] of shapes) {
    assert.ok(p.depth >= MIN_DEPTH, `${u}: depth ≥ ${MIN_DEPTH}`);
    assert.ok(p.width >= 1.5 * p.depth - 1e-9 && p.width <= 3 * p.depth, `${u}: ${p.width}×${p.depth} is wider than deep, not a line`);
    assert.ok(p.width * p.depth * MAX_LAYERS >= u, `${u}: holds its capacity`);
    if (u >= 128) assert.ok(p.depth >= 8, `${u}: a 32-SCU box fits lengthwise`);
  }
  assert.deepEqual(Object.fromEntries(shapes.map(([u, p]) => [u, `${p.width}x${p.depth}`])), {
    1: "6x4",
    44: "6x4",
    46: "6x4",
    288: "12x8",
    512: "16x8",
    696: "18x10",
    2200: "31x18",
    4608: "48x24",
  });
});

test("large platform, little cargo: a compact pile in the corner, zoomed with the platform outlined", () => {
  const boxes = estimateBoxes(load218());
  const ironclad = layoutCargo(boxes, { mode: "destination", shipCapacityScu: 2200 });
  const main = ironclad.platforms[0];
  assert.deepEqual([main.width, main.depth], [31, 18], "the whole platform is kept");
  const b = bbox(ironclad.items);
  assert.ok(b.width < main.width / 2 && b.depth < main.depth, "a corner pile, not a band");
  // Hull C: 4,608 SCU platform with 218 SCU → more than ZOOM_RATIO × the cargo floor: zoomed.
  const hullC = layoutCargo(boxes, { mode: "destination", shipCapacityScu: 4608 });
  assert.ok(hullC.platforms[0].width * hullC.platforms[0].depth > ZOOM_RATIO * hullC.cargoFootprint.width * hullC.cargoFootprint.depth);
  assert.ok(hullC.zoom !== null && hullC.zoom.width < hullC.platforms[0].width && hullC.zoom.depth <= hullC.platforms[0].depth);
  assert.ok(hullC.zoom!.width >= hullC.cargoFootprint.width && hullC.zoom!.depth >= hullC.cargoFootprint.depth, "the zoom covers the cargo");
  assert.equal(layoutCargo(boxes, { mode: "destination", shipCapacityScu: 512 }).zoom, null, "no zoom when the cargo fills a fair part");
});

/* ------------------------------------------------------------- overflow */

test("over capacity: a separate overflow platform with the rest", () => {
  const boxes = estimateBoxes(load328());
  const cargo = total(boxes);
  for (const capacity of [44, 46, 288]) {
    const layout = layoutCargo(boxes, { mode: "destination", shipCapacityScu: capacity });
    assert.equal(layout.overCapacity, true);
    assert.equal(layout.platforms.length, 2);
    const [main, over] = layout.platforms;
    assert.equal(over.kind, "overflow");
    assert.ok(main.scuMilli <= capacity * 1000, `main ${main.scuMilli} ≤ ${capacity} SCU`);
    assert.equal(main.scuMilli + over.scuMilli, cargo, "nothing lost");
    assert.equal(layout.overflowScuMilli, over.scuMilli);
    const smallest = Math.min(...layout.items.filter((i) => i.overflow).map((i) => i.box!.scuMilli));
    assert.ok(over.scuMilli - (cargo - capacity * 1000) < smallest, "overflow = cargo − capacity, within one box");
    assert.ok(over.x >= main.x + main.width + PLATFORM_GAP - 1e-9, "a clear gap");
    assertValid(layout);
    assert.deepEqual(layoutCargo(boxes, { mode: "destination", shipCapacityScu: capacity }), layout, "deterministic");
  }
  assert.equal(layoutCargo(boxes, { mode: "destination", shipCapacityScu: null }).platforms.length, 1, "no ship: no overflow");
  assert.equal(layoutCargo(boxes, { mode: "destination", shipCapacityScu: 696 }).platforms.length, 1, "within capacity: no overflow");
});

test("overflow keeps whole groups together where possible", () => {
  // Groups (legend order): A 100, B 30, C 20. Capacity 60: A cannot fit whole; B and C can.
  const boxes = estimateBoxes([
    req({ totalScuMilli: 100_000, maxBoxScuMilli: 4_000, deliveries: dest("A") }),
    req({ totalScuMilli: 30_000, maxBoxScuMilli: 2_000, deliveries: dest("B") }),
    req({ totalScuMilli: 20_000, maxBoxScuMilli: 2_000, deliveries: dest("C") }),
  ]);
  const layout = layoutCargo(boxes, { mode: "destination", shipCapacityScu: 60 });
  const where = (key: string) => new Set(layout.items.filter((i) => i.colorKey === key).map((i) => (i.overflow ? "overflow" : "main")));
  assert.deepEqual([...where("b")], ["main"], "B whole on the main platform");
  assert.deepEqual([...where("c")], ["main"], "C whole on the main platform");
  assert.deepEqual([...where("a")].sort(), ["main", "overflow"], "only A is split (it does not fit)");
  // B 30 + C 20 + two of A's 4-SCU boxes = 58: a third 4-SCU box would exceed 60 (boxes are never cut).
  assert.equal(layout.platforms[0].scuMilli, 58_000, "filled to capacity, within one box");
  // A split group is highlighted together on both platforms.
  const lit = highlightedBoxes(boxes, { kind: "group", key: "a" }, "destination");
  const litWhere = new Set(layout.items.filter((i) => i.box && lit.has(i.box.id)).map((i) => i.overflow));
  assert.deepEqual([...litWhere].sort(), [false, true]);
});

/* ------------------------------------------------------------ block safety */

test("no block mode below the safety limit; the safety net above it, exact volumes", () => {
  const reqs = (count: number) => ["A", "B", "C", "D"].map((d) => req({ totalScuMilli: (count / 4) * 1000, maxBoxScuMilli: 1_000, deliveries: dest(d) }));
  const atLimit = estimateBoxes(reqs(BLOCK_SAFETY_LIMIT));
  assert.equal(atLimit.length, BLOCK_SAFETY_LIMIT);
  assert.equal(layoutCargo(atLimit, { mode: "destination", shipCapacityScu: null }).mode, "boxes");
  assert.equal(layoutCargo(estimateBoxes(reqs(400)), { mode: "destination", shipCapacityScu: 288 }).mode, "boxes", "no switch at 300 any more");
  const above = estimateBoxes(reqs(BLOCK_SAFETY_LIMIT + 4));
  const layout = layoutCargo(above, { mode: "destination", shipCapacityScu: null });
  assert.equal(layout.mode, "blocks");
  for (const block of layout.items) assert.ok(Math.abs(block.w * block.d * block.h - (block.blockScuMilli ?? 0) / 1000) < 1e-6, "volume = group SCU");
  // Over capacity in block mode: split exactly at the capacity.
  const over = layoutCargo(above, { mode: "destination", shipCapacityScu: 1000 });
  assert.equal(over.platforms[0].scuMilli, 1_000_000);
  assert.equal(over.overflowScuMilli, total(above) - 1_000_000);
});

test("dimensions table", () => {
  assert.deepEqual(BOX_DIMENSIONS[32], [2, 8, 2]);
  assert.deepEqual(dimensionsFor(3), BOX_DIMENSIONS[4], "non-standard stored size: next size up");
  for (const s of CONTAINER_SIZES_SCU) {
    const [w, l, h] = BOX_DIMENSIONS[s];
    assert.equal(w * l * h, s, `volume of ${s}`);
  }
});

/* ----------------------------------------------------------------- colours */

test("colours: stable across reorderings; destination vs commodity", () => {
  const load = load328();
  const a = legend(estimateBoxes(load), "destination");
  const b = legend(estimateBoxes([...load].reverse()), "destination");
  assert.deepEqual(a.map((e) => [e.label, e.color]), b.map((e) => [e.label, e.color]));
  assert.equal(a[a.length - 1].label, "Several destinations");
  assert.equal(a[a.length - 1].color, SEVERAL_COLOR);
  assert.equal(a[0].color, BOX_PALETTE[0]);
  const c = legend(estimateBoxes(load), "commodity");
  assert.deepEqual(c.map((e) => e.label), ["Agricium", "Aluminum", "Gold", "Laranite", "Medical Supplies", "Titanium"]);
  for (const color of BOX_PALETTE) assert.ok(!["#6cc18a", "#e8a33d", "#f07a6a"].includes(color), "not the label colours");
});

/* --------------------------------------------------------------- highlight */

test("highlight mapping: line ↔ boxes, location ↔ boxes, box → lines, pin and clear", () => {
  const boxes = estimateBoxes(load328());
  const gold = boxes.filter((b) => b.commodityName === "Gold").map((b) => b.id);
  const medical = boxes.filter((b) => b.commodityName === "Medical Supplies").map((b) => b.id);
  const line = { kind: "line", operation: "delivery", location: "Area18 TDD", commodity: "gold" } as const;
  assert.deepEqual([...highlightedBoxes(boxes, line, "destination")].sort(), [...gold].sort());
  assert.deepEqual([...highlightedBoxes(boxes, { kind: "line", operation: "delivery", location: "port tressler", commodity: "Medical Supplies" }, "destination")].sort(), [...medical].sort());
  assert.equal(highlightedBoxes(boxes, { kind: "location", operation: "pickup", location: "Port Tressler" }, "destination").size, boxes.length);
  const goldBox = boxes.find((b) => b.commodityName === "Gold")!;
  assert.deepEqual(linesForBox(goldBox), [lineKey("pickup", "Port Tressler", "Gold"), lineKey("delivery", "Area18 TDD", "Gold")]);
  assert.deepEqual([...highlightedLines(boxes, { kind: "box", id: goldBox.id }, "destination")], linesForBox(goldBox));
  let s = reduceHighlight(NO_HIGHLIGHT, { type: "hover", selection: line });
  s = reduceHighlight(s, { type: "click", selection: line });
  s = reduceHighlight(s, { type: "hover", selection: { kind: "box", id: goldBox.id } });
  assert.deepEqual(activeSelection(s), line, "a pin wins over hover");
  assert.equal(reduceHighlight(s, { type: "click", selection: { ...line } }).pinned, null, "clicking again clears");
  assert.equal(reduceHighlight(s, { type: "clear" }).pinned, null, "Escape clears");
});

/* ------------------------------------------------------------- performance */

function timed(label: string, fn: () => Layout, limitMs: number): Layout {
  fn(); // warm-up
  const runs = 5;
  const start = performance.now();
  let out!: Layout;
  for (let i = 0; i < runs; i++) out = fn();
  const ms = (performance.now() - start) / runs;
  console.log(`[timing] ${label}: ${ms.toFixed(2)} ms`);
  assert.ok(ms < limitMs, `${label} took ${ms.toFixed(1)} ms`);
  return out;
}

test("performance: 300 and 2,000 boxes, a 4,608 SCU load", () => {
  const six = ["A", "B", "C", "D", "E", "F"];
  const b300 = estimateBoxes(six.map((d) => req({ totalScuMilli: 50_000, maxBoxScuMilli: 1_000, deliveries: dest(d) })));
  assert.equal(b300.length, 300);
  assertValid(timed("layout of 300 boxes (1 SCU), C2", () => layoutCargo(b300, { mode: "destination", shipCapacityScu: 696 }), 50));

  const b2000 = estimateBoxes(six.map((d, i) => req({ totalScuMilli: (i < 2 ? 334 : 333) * 1000, maxBoxScuMilli: 1_000, deliveries: dest(d) })));
  assert.equal(b2000.length, 2000);
  const l2000 = timed("layout of 2,000 boxes (1 SCU), Ironclad", () => layoutCargo(b2000, { mode: "destination", shipCapacityScu: 2200 }), 250);
  assert.equal(l2000.mode, "boxes");
  timed("layout of 2,000 boxes over capacity (Hermes)", () => layoutCargo(b2000, { mode: "destination", shipCapacityScu: 288 }), 250);

  const hull = ["A", "B", "C", "D"].map((d) => req({ totalScuMilli: 1_152_000, deliveries: dest(d) }));
  const hullBoxes = estimateBoxes(hull);
  assert.equal(hullBoxes.length, 144);
  const lh = timed("layout of a 4,608 SCU load (Hull C)", () => layoutCargo(hullBoxes, { mode: "destination", shipCapacityScu: 4608 }), 50);
  assert.equal(lh.platforms.length, 1);
  assertValid(lh);
});
