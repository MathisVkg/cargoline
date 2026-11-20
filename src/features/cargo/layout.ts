// Isometric layout of estimated boxes (pure; Task 13.1). Grid units: 1 unit = 1 SCU cube edge.
//
// - Platform: wider than deep. With a ship, sized so its capacity fits in about MAX_LAYERS of
//   height; without one, sized to the cargo. Cargo above the ship's capacity spills into an
//   overflow zone beyond the platform (tinted amber, never blocking).
// - Packing: boxes grouped by colour key (each group is one contiguous slab along x), largest
//   first. A heightmap places each box at the lowest x, then lowest z, then lowest y where its
//   footprint is flat (fully supported) and the stack stays within MAX_LAYERS. Boxes may turn
//   their length axis (x or y), nothing else. Deterministic.
// - Above BLOCK_MODE_THRESHOLD boxes, each group is one solid block of its exact volume.
// - Painter's order: a topological sort of "behind" relations between boxes whose isometric
//   silhouettes overlap (separating-axis test on x−y, y−z, x−z), tie-broken by x+y+z.
import { colorKey, legend, type ColorMode, type EstimatedBox, type LegendEntry } from "./boxes.ts";

/** Width (x) × length (y) × height (z) in grid units, per container size. */
export const BOX_DIMENSIONS: Record<number, readonly [number, number, number]> = {
  1: [1, 1, 1],
  2: [1, 2, 1],
  4: [2, 2, 1],
  8: [2, 2, 2],
  16: [2, 4, 2],
  24: [2, 6, 2],
  32: [2, 8, 2],
};
export const MAX_LAYERS = 4;
export const BLOCK_MODE_THRESHOLD = 300;
const MIN_DEPTH = 4;
const MIN_WIDTH = 8;
const MILLI = 1000;

/** Dimensions for a container size (a non-standard stored size uses the next standard size up). */
export function dimensionsFor(sizeScu: number): readonly [number, number, number] {
  const sizes = Object.keys(BOX_DIMENSIONS).map(Number).sort((a, b) => a - b);
  const size = sizes.find((s) => s >= sizeScu) ?? sizes[sizes.length - 1];
  return BOX_DIMENSIONS[size];
}

export interface Platform {
  /** Along x (the long side). */
  width: number;
  /** Along y. */
  depth: number;
  height: number;
}

/**
 * A floor of `units / MAX_LAYERS` cells, about twice as wide as deep, depth even.
 * - `tight` (a ship's capacity): no minimum size, so the platform holds about the capacity and
 *   no more (a 46 SCU ship gets 6 × 2, not the 8 × 4 = 128 SCU minimum), and overflow starts
 *   where the capacity ends. Still wider than deep.
 * - otherwise (sized to the cargo): at least 8 × 4 so small loads still read as a platform.
 */
export function platformFor(units: number, tight = false): Platform {
  const area = Math.max(1, Math.ceil(units / MAX_LAYERS));
  let depth = Math.max(tight ? 2 : MIN_DEPTH, Math.ceil(Math.sqrt(area / 2)));
  if (depth % 2) depth += 1;
  if (tight) {
    while (depth > 2 && Math.ceil(area / depth) <= depth) depth -= 2;
    return { width: Math.max(depth + 1, Math.ceil(area / depth)), depth, height: MAX_LAYERS };
  }
  const width = Math.max(MIN_WIDTH, depth + 2, Math.ceil(area / depth));
  return { width, depth, height: MAX_LAYERS };
}

export interface Placed {
  id: string;
  x: number;
  y: number;
  z: number;
  w: number;
  d: number;
  h: number;
  colorKey: string;
  /** In the overflow zone (beyond the ship's platform). */
  overflow: boolean;
  /** Boxes mode: the box. Block mode: null. */
  box: EstimatedBox | null;
  /** Block mode: the group's SCU (exact). */
  blockScuMilli: number | null;
}

export interface Layout {
  mode: "boxes" | "blocks";
  platform: Platform;
  /** x where the overflow zone starts (the platform width), or null without overflow. */
  overflowFrom: number | null;
  /** The furthest x used (≥ platform width when there is overflow). */
  extentX: number;
  /** Painter's order: back to front. */
  items: Placed[];
  legend: LegendEntry[];
  /** True when the ship was given and cargo exceeds its capacity. */
  overCapacity: boolean;
}

export interface LayoutOptions {
  mode: ColorMode;
  /** Selected ship's cargo capacity in SCU, or null. */
  shipCapacityScu: number | null;
}

/* ---------------------------------------------------------------- packing */

class Heightmap {
  private cols: number[][] = [];
  readonly depth: number;
  constructor(depth: number) {
    this.depth = depth;
  }
  private col(x: number): number[] {
    while (this.cols.length <= x) this.cols.push(new Array(this.depth).fill(0));
    return this.cols[x];
  }
  /** The common height of the footprint, or -1 if it is not flat. */
  flat(x: number, y: number, w: number, d: number): number {
    const z = this.col(x)[y];
    for (let i = x; i < x + w; i++) {
      const c = this.col(i);
      for (let j = y; j < y + d; j++) if (c[j] !== z) return -1;
    }
    return z;
  }
  raise(x: number, y: number, w: number, d: number, top: number) {
    for (let i = x; i < x + w; i++) {
      const c = this.col(i);
      for (let j = y; j < y + d; j++) c[j] = top;
    }
  }
  full(x: number, height: number): boolean {
    return this.col(x).every((v) => v >= height);
  }
}

/** Places one box at the lowest x, then z, then y (flat support, within the height limit). */
function place(map: Heightmap, startX: number, dims: readonly [number, number, number], height: number) {
  const [bw, bl, bh] = dims;
  const orientations: [number, number][] = bw === bl ? [[bw, bl]] : [[bw, bl], [bl, bw]];
  for (let x = startX; ; x++) {
    let best: { y: number; z: number; w: number; d: number } | null = null;
    for (const [w, d] of orientations) {
      if (d > map.depth) continue;
      for (let y = 0; y + d <= map.depth; y++) {
        const z = map.flat(x, y, w, d);
        if (z < 0 || z + bh > height) continue;
        if (!best || z < best.z || (z === best.z && y < best.y)) best = { y, z, w, d };
      }
    }
    if (best) return { x, ...best, h: bh };
  }
}

interface Group {
  key: string;
  boxes: EstimatedBox[];
  scuMilli: number;
}

function groupsFor(boxes: readonly EstimatedBox[], entries: LegendEntry[], mode: ColorMode): Group[] {
  const byKey = new Map<string, EstimatedBox[]>();
  for (const box of boxes) {
    const key = colorKey(box, mode);
    const list = byKey.get(key) ?? [];
    list.push(box);
    byKey.set(key, list);
  }
  // Legend order (alphabetical, "Several destinations" last) = slab order along x.
  return entries.map((e) => ({
    key: e.key,
    scuMilli: e.scuMilli,
    boxes: [...(byKey.get(e.key) ?? [])].sort(
      (a, b) => b.sizeScu - a.sizeScu || (a.requirementKey < b.requirementKey ? -1 : a.requirementKey > b.requirementKey ? 1 : 0) || idIndex(a) - idIndex(b),
    ),
  }));
}

const idIndex = (b: EstimatedBox) => Number(b.id.slice(b.id.lastIndexOf("|") + 1));

function packBoxes(groups: Group[], depth: number): Placed[] {
  const map = new Heightmap(depth);
  const placed: Placed[] = [];
  let groupStart = 0;
  for (const group of groups) {
    let start = groupStart;
    let groupEnd = groupStart;
    for (const box of group.boxes) {
      while (map.full(start, MAX_LAYERS)) start++;
      const p = place(map, start, dimensionsFor(box.sizeScu), MAX_LAYERS);
      map.raise(p.x, p.y, p.w, p.d, p.z + p.h);
      placed.push({ id: box.id, x: p.x, y: p.y, z: p.z, w: p.w, d: p.d, h: p.h, colorKey: group.key, overflow: false, box, blockScuMilli: null });
      groupEnd = Math.max(groupEnd, p.x + p.w);
    }
    groupStart = groupEnd; // the next group starts after this slab: groups stay contiguous
  }
  return placed;
}

function packBlocks(groups: Group[], depth: number): Placed[] {
  let x = 0;
  return groups.map((g) => {
    const units = g.scuMilli / MILLI;
    const w = units / (depth * MAX_LAYERS); // exact volume: w × depth × MAX_LAYERS = units
    const item: Placed = { id: `block|${g.key}`, x, y: 0, z: 0, w, d: depth, h: MAX_LAYERS, colorKey: g.key, overflow: false, box: null, blockScuMilli: g.scuMilli };
    x += w;
    return item;
  });
}

/* --------------------------------------------------------- painter's order */

/** Strict overlap of the two boxes' isometric silhouettes (SAT on x−y, y−z, x−z). */
export function silhouettesOverlap(a: Placed, b: Placed): boolean {
  const range = (p: Placed, f: (x: number, y: number, z: number) => number) => {
    const xs = [p.x, p.x + p.w];
    const ys = [p.y, p.y + p.d];
    const zs = [p.z, p.z + p.h];
    let lo = Infinity;
    let hi = -Infinity;
    for (const x of xs) for (const y of ys) for (const z of zs) {
      const v = f(x, y, z);
      if (v < lo) lo = v;
      if (v > hi) hi = v;
    }
    return [lo, hi] as const;
  };
  const eps = 1e-9;
  for (const f of [(x: number, y: number) => x - y, (_x: number, y: number, z: number) => y - z, (x: number, _y: number, z: number) => x - z]) {
    const [a0, a1] = range(a, f);
    const [b0, b1] = range(b, f);
    if (a1 <= b0 + eps || b1 <= a0 + eps) return false;
  }
  return true;
}

/** True when `a` must be drawn before `b` (a is behind b on a separating axis). */
export function isBehind(a: Placed, b: Placed): boolean {
  const eps = 1e-9;
  return a.x + a.w <= b.x + eps || a.y + a.d <= b.y + eps || a.z + a.h <= b.z + eps;
}

export function painterSort(items: Placed[]): Placed[] {
  const n = items.length;
  const order = items.map((_, i) => i).sort((i, j) => depthKey(items[i]) - depthKey(items[j]) || i - j);
  const rank = new Array<number>(n);
  order.forEach((idx, r) => (rank[idx] = r));
  const after: number[][] = Array.from({ length: n }, () => []);
  const indegree = new Array<number>(n).fill(0);
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      if (!silhouettesOverlap(items[i], items[j])) continue;
      if (isBehind(items[i], items[j])) {
        after[i].push(j);
        indegree[j]++;
      } else if (isBehind(items[j], items[i])) {
        after[j].push(i);
        indegree[i]++;
      }
    }
  }
  // Kahn's algorithm, always taking the ready item with the smallest depth key (deterministic).
  const ready = order.filter((i) => indegree[i] === 0);
  const out: Placed[] = [];
  while (ready.length) {
    ready.sort((a, b) => rank[a] - rank[b]);
    const i = ready.shift()!;
    out.push(items[i]);
    for (const j of after[i]) if (--indegree[j] === 0) ready.push(j);
  }
  // Disjoint boxes never form a cycle; keep any leftovers anyway rather than dropping them.
  if (out.length < n) for (const i of order) if (!out.includes(items[i])) out.push(items[i]);
  return out;
}

const depthKey = (p: Placed) => p.x + p.w / 2 + p.y + p.d / 2 + p.z + p.h / 2;

/* ------------------------------------------------------------------ layout */

export function layoutCargo(boxes: readonly EstimatedBox[], options: LayoutOptions): Layout {
  const entries = legend(boxes, options.mode);
  const groups = groupsFor(boxes, entries, options.mode);
  const cargoMilli = boxes.reduce((s, b) => s + b.scuMilli, 0);
  const cargoUnits = boxes.reduce((s, b) => s + b.sizeScu, 0);
  const ship = options.shipCapacityScu !== null && options.shipCapacityScu > 0 ? options.shipCapacityScu : null;
  const overCapacity = ship !== null && cargoMilli > ship * MILLI;
  const blocks = boxes.length > BLOCK_MODE_THRESHOLD;

  let platform = ship !== null ? platformFor(ship, true) : platformFor(Math.max(cargoUnits, 1));
  let items = blocks ? packBlocks(groups, platform.depth) : packBoxes(groups, platform.depth);
  let extentX = Math.max(0, ...items.map((p) => p.x + p.w));

  if (ship === null) {
    // Sized to the cargo: the platform grows to hold everything.
    platform = { ...platform, width: Math.max(platform.width, Math.ceil(extentX - 1e-9)) };
  } else if (!overCapacity) {
    // Within capacity, packing gaps must not look like overflow: widen the platform instead.
    platform = { ...platform, width: Math.max(platform.width, Math.ceil(extentX - 1e-9)) };
  }
  let overflowFrom: number | null = null;
  if (ship !== null && overCapacity && extentX > platform.width + 1e-9) {
    overflowFrom = platform.width;
    items = items.map((p) => (p.x + p.w > platform.width + 1e-9 ? { ...p, overflow: true } : p));
  }
  extentX = Math.max(extentX, platform.width);
  return { mode: blocks ? "blocks" : "boxes", platform, overflowFrom, extentX, items: painterSort(items), legend: entries, overCapacity };
}

/* -------------------------------------------------------------- projection */

const COS30 = Math.cos(Math.PI / 6);

/** Isometric projection of a grid point to SVG units (viewer at +x, +y, +z). */
export function project(x: number, y: number, z: number, unit = 10): [number, number] {
  return [(x - y) * COS30 * unit, ((x + y) * 0.5 - z) * unit];
}

/** The three visible faces of a box: top, front (x = x1), side (y = y1), as SVG point lists. */
export function faces(p: Pick<Placed, "x" | "y" | "z" | "w" | "d" | "h">, unit = 10) {
  const { x, y, z, w, d, h } = p;
  const pt = (a: number, b: number, c: number) => project(a, b, c, unit).map((v) => v.toFixed(2)).join(",");
  const x1 = x + w;
  const y1 = y + d;
  const z1 = z + h;
  return {
    top: [pt(x, y, z1), pt(x1, y, z1), pt(x1, y1, z1), pt(x, y1, z1)].join(" "),
    front: [pt(x1, y, z), pt(x1, y1, z), pt(x1, y1, z1), pt(x1, y, z1)].join(" "),
    side: [pt(x, y1, z), pt(x1, y1, z), pt(x1, y1, z1), pt(x, y1, z1)].join(" "),
  };
}

/** SVG viewBox covering the platform (and overflow) and every box, with a margin. */
export function viewBoxFor(layout: Layout, unit = 10, margin = 12): string {
  const pts: [number, number][] = [];
  const { depth, height } = layout.platform;
  for (const x of [0, layout.extentX]) for (const y of [0, depth]) pts.push(project(x, y, 0, unit));
  pts.push(project(0, 0, height, unit), project(layout.extentX, 0, height, unit));
  for (const p of layout.items) pts.push(project(p.x, p.y, p.z + p.h, unit), project(p.x + p.w, p.y, p.z + p.h, unit));
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  const minX = Math.min(...xs) - margin;
  const minY = Math.min(...ys) - margin;
  return `${minX.toFixed(1)} ${minY.toFixed(1)} ${(Math.max(...xs) - minX + margin).toFixed(1)} ${(Math.max(...ys) - minY + margin).toFixed(1)}`;
}
