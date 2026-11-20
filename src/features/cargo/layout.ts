// Isometric layout of estimated boxes (pure; Tasks 13.1 and 13.2). Grid units: 1 unit = 1 SCU edge.
//
// - Boxes never depend on the ship; only the platform does.
// - Compact packing: the cargo is packed into its own footprint (about 2:1, area ≈ cargo / 4
//   layers, growing only when packing gaps need it), placed at the front-left corner of the
//   platform. Colour groups are contiguous slabs along x, largest boxes first. A heightmap places
//   each box at the lowest x, then z, then y, on a flat (fully supported) footprint, at most
//   MAX_LAYERS high. Only the length axis may turn. Deterministic.
// - Main platform: sized to the ship's capacity (about 2:1, depth ≥ MIN_DEPTH so a tiny ship is
//   never a thin line), or to the cargo without a ship.
// - Over capacity: the main platform takes cargo up to the capacity (whole colour groups in legend
//   order first, then boxes of the remaining groups that still fit), the rest goes on a separate
//   overflow platform to the right, after a gap.
// - Large platform, little cargo (> ZOOM_RATIO × the cargo footprint): the view zooms to the cargo
//   plus a margin; the full platform is shown in an overview inset.
// - Safety net: more than BLOCK_SAFETY_LIMIT boxes → one block per colour group (exact volume).
// - Painter's order: a topological sort of "behind" relations between boxes whose isometric
//   silhouettes overlap (separating axes on x−y, y−z, x−z), tie-broken by x+y+z.
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
/** Platforms are never shallower than this, so a tiny ship is not a thin line. */
export const MIN_DEPTH = 4;
/** Above this many boxes, each colour group is drawn as one block (performance safety net). */
export const BLOCK_SAFETY_LIMIT = 2000;
/** Zoom to the cargo when the platform floor is more than this many times the cargo footprint. */
export const ZOOM_RATIO = 8;
/** Margin (grid units) around the cargo when zoomed. */
export const ZOOM_MARGIN = 3;
/** Gap (grid units) between the main and the overflow platform. */
export const PLATFORM_GAP = 3;
const MILLI = 1000;
const EPS = 1e-9;

/** Dimensions for a container size (a non-standard stored size uses the next standard size up). */
export function dimensionsFor(sizeScu: number): readonly [number, number, number] {
  const sizes = Object.keys(BOX_DIMENSIONS).map(Number).sort((a, b) => a - b);
  const size = sizes.find((s) => s >= sizeScu) ?? sizes[sizes.length - 1];
  return BOX_DIMENSIONS[size];
}

export interface Footprint {
  /** Along x (the long side). */
  width: number;
  /** Along y. */
  depth: number;
}

const even = (n: number) => (n % 2 ? n + 1 : n);

/** From this capacity on, a platform is at least 8 deep so a 32-SCU container fits lengthwise. */
export const LONG_BOX_CAPACITY = 128;

/**
 * A platform for `units` SCU: wider than deep (width ≥ 1.5 × depth, about 2:1 for large ones),
 * depth even and ≥ MIN_DEPTH (≥ 8 from LONG_BOX_CAPACITY), floor ≥ units / MAX_LAYERS. Readable for
 * any capacity: 1 → 6 × 4, 44 → 6 × 4, 288 → 12 × 8, 512 → 16 × 8, 696 → 18 × 10,
 * 2,200 → 31 × 18, 4,608 → 48 × 24.
 */
export function platformFor(units: number): Footprint {
  const area = Math.max(1, Math.ceil(units / MAX_LAYERS));
  const depth = Math.max(MIN_DEPTH, units >= LONG_BOX_CAPACITY ? 8 : 0, even(Math.ceil(Math.sqrt(area / 2))));
  return { width: Math.max(Math.ceil(1.5 * depth), Math.ceil(area / depth)), depth };
}

/**
 * The packing depth for a compact cargo footprint of `units` (about 2:1), but at least the
 * longest box's length, so 24- and 32-SCU boxes can lie along y: otherwise each colour slab that
 * holds one of them would be 6–8 units wide and the cargo would become a long strip.
 */
export function compactDepth(units: number, longestBox = 2): number {
  const area = Math.max(1, Math.ceil(units / MAX_LAYERS));
  return Math.max(2, even(longestBox), even(Math.ceil(Math.sqrt(area / 2))));
}

const longestOf = (groups: { boxes: EstimatedBox[] }[]) =>
  Math.max(2, ...groups.flatMap((g) => g.boxes.map((b) => dimensionsFor(b.sizeScu)[1])));

export interface PlatformArea {
  kind: "main" | "overflow";
  /** Left edge along x (0 for the main platform). */
  x: number;
  width: number;
  depth: number;
  /** Cargo on this platform. */
  scuMilli: number;
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
  /** On the overflow platform. */
  overflow: boolean;
  /** Boxes mode: the box. Block mode: null. */
  box: EstimatedBox | null;
  /** Block mode: the block's SCU (exact). */
  blockScuMilli: number | null;
}

export interface Layout {
  mode: "boxes" | "blocks";
  /** The main platform, then the overflow platform when over capacity. */
  platforms: PlatformArea[];
  /** Painter's order: back to front. */
  items: Placed[];
  legend: LegendEntry[];
  overCapacity: boolean;
  /** Cargo on the overflow platform. */
  overflowScuMilli: number;
  /** Bounding box of the cargo on the main platform. */
  cargoFootprint: Footprint;
  /** Zoomed view region of the main platform, or null to show it whole. */
  zoom: Footprint | null;
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

const idIndex = (b: EstimatedBox) => Number(b.id.slice(b.id.lastIndexOf("|") + 1));
const compareBoxes = (a: EstimatedBox, b: EstimatedBox) =>
  b.sizeScu - a.sizeScu ||
  (a.requirementKey < b.requirementKey ? -1 : a.requirementKey > b.requirementKey ? 1 : 0) ||
  idIndex(a) - idIndex(b);

function groupsFor(boxes: readonly EstimatedBox[], entries: LegendEntry[], mode: ColorMode): Group[] {
  const byKey = new Map<string, EstimatedBox[]>();
  for (const box of boxes) {
    const key = colorKey(box, mode);
    const list = byKey.get(key) ?? [];
    list.push(box);
    byKey.set(key, list);
  }
  // Legend order (alphabetical, "Several destinations" last) = slab order along x.
  return entries
    .map((e) => {
      const list = [...(byKey.get(e.key) ?? [])].sort(compareBoxes);
      return { key: e.key, boxes: list, scuMilli: list.reduce((s, b) => s + b.scuMilli, 0) };
    })
    .filter((g) => g.boxes.length > 0);
}

/**
 * Splits groups between the main platform (up to `capacityMilli` of cargo) and the overflow.
 * Whole groups in legend order first; then, in the same order, boxes of the remaining groups
 * that still fit (so a group is split only when it does not fit whole).
 */
export function splitForCapacity(groups: Group[], capacityMilli: number): { main: Group[]; overflow: Group[] } {
  let free = capacityMilli;
  const inMain = new Set<EstimatedBox>();
  for (const g of groups) {
    if (g.scuMilli <= free) {
      g.boxes.forEach((b) => inMain.add(b));
      free -= g.scuMilli;
    }
  }
  for (const g of groups) {
    for (const b of g.boxes) {
      if (inMain.has(b) || b.scuMilli > free) continue;
      inMain.add(b);
      free -= b.scuMilli;
    }
  }
  const part = (keep: boolean) =>
    groups
      .map((g) => {
        const boxes = g.boxes.filter((b) => inMain.has(b) === keep);
        return { key: g.key, boxes, scuMilli: boxes.reduce((s, b) => s + b.scuMilli, 0) };
      })
      .filter((g) => g.boxes.length > 0);
  return { main: part(true), overflow: part(false) };
}

/** Packs groups as contiguous slabs into a footprint of the given depth, starting at `x0`. */
function packBoxes(groups: Group[], depth: number, x0: number, overflow: boolean): Placed[] {
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
      placed.push({ id: box.id, x: x0 + p.x, y: p.y, z: p.z, w: p.w, d: p.d, h: p.h, colorKey: group.key, overflow, box, blockScuMilli: null });
      groupEnd = Math.max(groupEnd, p.x + p.w);
    }
    groupStart = groupEnd; // the next group starts after this slab: groups stay contiguous
  }
  return placed;
}

/** Safety net: one block per group, `depth` deep and MAX_LAYERS high, exact volume. */
function packBlocks(groups: Group[], depth: number, x0: number, overflow: boolean): Placed[] {
  let x = x0;
  return groups.map((g) => {
    const w = g.scuMilli / MILLI / (depth * MAX_LAYERS);
    const item: Placed = { id: `block|${overflow ? "overflow|" : ""}${g.key}`, x, y: 0, z: 0, w, d: depth, h: MAX_LAYERS, colorKey: g.key, overflow, box: null, blockScuMilli: g.scuMilli };
    x += w;
    return item;
  });
}

/** Splits groups at `capacityMilli` exactly (block mode: volumes can be fractional). */
function splitBlocks(groups: Group[], capacityMilli: number): { main: Group[]; overflow: Group[] } {
  let free = capacityMilli;
  const main: Group[] = [];
  const overflow: Group[] = [];
  for (const g of groups) {
    const take = Math.min(g.scuMilli, Math.max(0, free));
    free -= take;
    if (take > 0) main.push({ ...g, scuMilli: take });
    if (g.scuMilli - take > 0) overflow.push({ ...g, scuMilli: g.scuMilli - take });
  }
  return { main, overflow };
}

const extentOf = (items: Placed[], x0: number): Footprint => ({
  width: Math.max(0, ...items.map((p) => p.x + p.w - x0)),
  depth: Math.max(0, ...items.map((p) => p.y + p.d)),
});
const unitsOf = (groups: Group[], blocks: boolean) =>
  groups.reduce((s, g) => s + (blocks ? g.scuMilli / MILLI : g.boxes.reduce((t, b) => t + b.sizeScu, 0)), 0);

/* --------------------------------------------------------- painter's order */

type Ranges = [number, number, number, number, number, number];
const rangesOf = (p: Placed): Ranges => [
  p.x - (p.y + p.d), p.x + p.w - p.y, // x − y
  p.y - (p.z + p.h), p.y + p.d - p.z, // y − z
  p.x - (p.z + p.h), p.x + p.w - p.z, // x − z
];
const apart = (a: Ranges, b: Ranges) =>
  a[1] <= b[0] + EPS || b[1] <= a[0] + EPS || a[3] <= b[2] + EPS || b[3] <= a[2] + EPS || a[5] <= b[4] + EPS || b[5] <= a[4] + EPS;

/** Strict overlap of the two boxes' isometric silhouettes (SAT on x−y, y−z, x−z). */
export function silhouettesOverlap(a: Placed, b: Placed): boolean {
  return !apart(rangesOf(a), rangesOf(b));
}

/** True when `a` must be drawn before `b` (a is behind b on a separating axis). */
export function isBehind(a: Placed, b: Placed): boolean {
  return a.x + a.w <= b.x + EPS || a.y + a.d <= b.y + EPS || a.z + a.h <= b.z + EPS;
}

const depthKey = (p: Placed) => p.x + p.w / 2 + p.y + p.d / 2 + p.z + p.h / 2;

/** Back-to-front order. A sweep on x−y limits the pair checks to boxes that can overlap. */
export function painterSort(items: Placed[]): Placed[] {
  const n = items.length;
  const ranges = items.map(rangesOf);
  const byU = items.map((_, i) => i).sort((i, j) => ranges[i][0] - ranges[j][0]);
  const key = items.map(depthKey);
  const after: number[][] = Array.from({ length: n }, () => []);
  const indegree = new Array<number>(n).fill(0);
  for (let a = 0; a < n; a++) {
    const i = byU[a];
    for (let b = a + 1; b < n; b++) {
      const j = byU[b];
      if (ranges[j][0] >= ranges[i][1] - EPS) break; // sorted by x−y start: no further overlap
      if (apart(ranges[i], ranges[j])) continue;
      if (isBehind(items[i], items[j])) {
        after[i].push(j);
        indegree[j]++;
      } else if (isBehind(items[j], items[i])) {
        after[j].push(i);
        indegree[i]++;
      }
    }
  }
  // Kahn's algorithm with a min-heap on (depth key, index): deterministic.
  const less = (a: number, b: number) => key[a] < key[b] - EPS || (Math.abs(key[a] - key[b]) <= EPS && a < b);
  const heap: number[] = [];
  const push = (v: number) => {
    heap.push(v);
    for (let c = heap.length - 1; c > 0; ) {
      const p = (c - 1) >> 1;
      if (!less(heap[c], heap[p])) break;
      [heap[c], heap[p]] = [heap[p], heap[c]];
      c = p;
    }
  };
  const pop = () => {
    const top = heap[0];
    const last = heap.pop()!;
    if (heap.length) {
      heap[0] = last;
      for (let p = 0; ; ) {
        const l = 2 * p + 1;
        const r = l + 1;
        let m = p;
        if (l < heap.length && less(heap[l], heap[m])) m = l;
        if (r < heap.length && less(heap[r], heap[m])) m = r;
        if (m === p) break;
        [heap[m], heap[p]] = [heap[p], heap[m]];
        p = m;
      }
    }
    return top;
  };
  for (let i = 0; i < n; i++) if (indegree[i] === 0) push(i);
  const out: Placed[] = [];
  const done = new Array<boolean>(n).fill(false);
  while (heap.length) {
    const i = pop();
    done[i] = true;
    out.push(items[i]);
    for (const j of after[i]) if (--indegree[j] === 0) push(j);
  }
  // Disjoint boxes never form a cycle; keep any leftovers anyway rather than dropping them.
  if (out.length < n) for (let i = 0; i < n; i++) if (!done[i]) out.push(items[i]);
  return out;
}

/* ------------------------------------------------------------------ layout */

export function layoutCargo(boxes: readonly EstimatedBox[], options: LayoutOptions): Layout {
  const entries = legend(boxes, options.mode);
  const groups = groupsFor(boxes, entries, options.mode);
  const cargoMilli = boxes.reduce((s, b) => s + b.scuMilli, 0);
  const ship = options.shipCapacityScu !== null && options.shipCapacityScu > 0 ? options.shipCapacityScu : null;
  const overCapacity = ship !== null && cargoMilli > ship * MILLI;
  const blocks = boxes.length > BLOCK_SAFETY_LIMIT;

  const split = !overCapacity
    ? { main: groups, overflow: [] as Group[] }
    : blocks
      ? splitBlocks(groups, ship! * MILLI)
      : splitForCapacity(groups, ship! * MILLI);

  // Main platform: the ship, or the cargo (without a ship). The cargo is packed compactly at the
  // front-left corner; on a ship it never packs deeper than the platform.
  const mainUnits = unitsOf(split.main, blocks);
  let main = platformFor(ship ?? mainUnits);
  const wanted = compactDepth(mainUnits, blocks ? 2 : longestOf(split.main));
  const mainDepth = ship !== null ? Math.min(main.depth, wanted) : wanted;
  const mainItems = blocks ? packBlocks(split.main, mainDepth, 0, false) : packBoxes(split.main, mainDepth, 0, false);
  const cargoFootprint = extentOf(mainItems, 0);
  if (ship === null) {
    // Sized to the cargo, keeping the platform's wider-than-deep shape.
    const depth = Math.max(main.depth, mainDepth);
    main = { depth, width: Math.max(main.width, Math.ceil(1.5 * depth), Math.ceil(cargoFootprint.width - EPS)) };
  } else {
    // Packing gaps only (cargo within capacity): widen rather than show a false overflow.
    main = { ...main, width: Math.max(main.width, Math.ceil(cargoFootprint.width - EPS)) };
  }

  const platforms: PlatformArea[] = [{ kind: "main", x: 0, ...main, scuMilli: split.main.reduce((s, g) => s + g.scuMilli, 0) }];
  let items = mainItems;
  let overflowScuMilli = 0;
  if (split.overflow.length > 0) {
    const units = unitsOf(split.overflow, blocks);
    const x0 = main.width + PLATFORM_GAP;
    const depth = compactDepth(units, blocks ? 2 : longestOf(split.overflow));
    const overflowItems = blocks ? packBlocks(split.overflow, depth, x0, true) : packBoxes(split.overflow, depth, x0, true);
    const ext = extentOf(overflowItems, x0);
    const size = platformFor(units);
    overflowScuMilli = split.overflow.reduce((s, g) => s + g.scuMilli, 0);
    platforms.push({
      kind: "overflow",
      x: x0,
      width: Math.max(size.width, Math.ceil(ext.width - EPS)),
      depth: Math.max(size.depth, Math.ceil(ext.depth - EPS)),
      scuMilli: overflowScuMilli,
    });
    items = [...mainItems, ...overflowItems];
  }

  const cargoArea = Math.max(1, cargoFootprint.width * cargoFootprint.depth);
  const zoom =
    platforms.length === 1 && mainItems.length > 0 && main.width * main.depth > ZOOM_RATIO * cargoArea
      ? {
          width: Math.min(main.width, Math.ceil(cargoFootprint.width - EPS) + ZOOM_MARGIN),
          depth: Math.min(main.depth, Math.ceil(cargoFootprint.depth - EPS) + ZOOM_MARGIN),
        }
      : null;

  return {
    mode: blocks ? "blocks" : "boxes",
    platforms,
    items: painterSort(items),
    legend: entries,
    overCapacity,
    overflowScuMilli,
    cargoFootprint,
    zoom,
  };
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

/** The floor region drawn for each platform (the main one is cut to the zoom region). */
export function drawnPlatforms(layout: Layout): PlatformArea[] {
  return layout.platforms.map((p) => (p.kind === "main" && layout.zoom ? { ...p, width: layout.zoom.width, depth: layout.zoom.depth } : p));
}

/** SVG viewBox covering the drawn platforms and every box, with a margin. */
export function viewBoxFor(layout: Layout, unit = 10, margin = 12): string {
  const pts: [number, number][] = [];
  for (const p of drawnPlatforms(layout)) {
    for (const x of [p.x, p.x + p.width]) for (const y of [0, p.depth]) pts.push(project(x, y, 0, unit));
    pts.push(project(p.x, 0, MAX_LAYERS, unit), project(p.x + p.width, 0, MAX_LAYERS, unit));
  }
  for (const p of layout.items) pts.push(project(p.x, p.y, p.z + p.h, unit), project(p.x + p.w, p.y, p.z + p.h, unit), project(p.x, p.y + p.d, p.z, unit), project(p.x + p.w, p.y + p.d, p.z, unit));
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  const minX = Math.min(...xs) - margin;
  const minY = Math.min(...ys) - margin;
  return `${minX.toFixed(1)} ${minY.toFixed(1)} ${(Math.max(...xs) - minX + margin).toFixed(1)} ${(Math.max(...ys) - minY + margin).toFixed(1)}`;
}
