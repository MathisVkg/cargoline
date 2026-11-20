// Box estimation and colours for the Cargo arrangement view (pure; Task 13.1). Milli-SCU.
//
// Rules:
// 1. Stored `exact` box groups are used as they are (and any cargo they don't cover is filled
//    with estimated boxes, so cargo is never dropped).
// 2. Otherwise the largest allowed container = the requirement's max box (from Details or typed
//    in), else 32 SCU; then the largest standard size not above it. The ship never changes the
//    boxes (Task 13.2): the catalog's max container value is unreliable (Hermes and Ironclad
//    report 1 SCU, Hull B and Hull C report 16 for grids that hold 32-SCU containers).
// 3. Greedy fill, largest first. A fractional remainder becomes one extra 1-SCU box marked
//    `partial` that carries exactly the remainder.
// 4. Destinations: one delivery → that destination. Several deliveries with known quantities →
//    each known part gets its own boxes (in delivery order). Any unknown part stays with the
//    requirement: a neutral "Several destinations" key, never split by guessing.
import { locationKey } from "../contracts/grouping.ts";
import { commodityKey, compareNames, type RequirementCargo } from "./summary.ts";

/** Standard container sizes, in SCU. */
export const CONTAINER_SIZES_SCU = [1, 2, 4, 8, 16, 24, 32] as const;
export const MAX_CONTAINER_SCU = 32;
const MILLI = 1000;

export interface EstimatedBox {
  /** Deterministic: `<requirement key>|<n>`. */
  id: string;
  requirementKey: string;
  contractRef: string;
  commodityName: string;
  /** Nominal container size in SCU (drives the drawn dimensions). */
  sizeScu: number;
  /** Cargo carried by this box; equals the size except for a `partial` box. */
  scuMilli: number;
  quality: "exact" | "estimated";
  /** The fractional remainder box. */
  partial: boolean;
  /** The destination of this box, or null when the split between several is unknown. */
  destination: string | null;
  /** The deliveries this box may go to (one, or the unknown-split ones). */
  destinations: string[];
  pickups: string[];
}

/** The largest standard container size not above `limitScu`. */
export function largestSizeAtMost(limitScu: number): number {
  let best: number = CONTAINER_SIZES_SCU[0];
  for (const size of CONTAINER_SIZES_SCU) if (size <= limitScu) best = size;
  return best;
}

/** The largest container allowed for a requirement: its max box, else 32. Never the ship. */
export function allowedMaxScu(requirementMaxMilli: number | null): number {
  let limit = MAX_CONTAINER_SCU;
  if (requirementMaxMilli !== null && requirementMaxMilli > 0) limit = Math.min(limit, requirementMaxMilli / MILLI);
  return largestSizeAtMost(limit);
}

/** Greedy split of `amountMilli` into containers ≤ `maxScu`: [{sizeScu, scuMilli, partial}]. */
export function greedyBoxes(amountMilli: number, maxScu: number): { sizeScu: number; scuMilli: number; partial: boolean }[] {
  const out: { sizeScu: number; scuMilli: number; partial: boolean }[] = [];
  let whole = Math.floor(amountMilli / MILLI);
  const remainder = amountMilli - whole * MILLI;
  for (let i = CONTAINER_SIZES_SCU.length - 1; i >= 0; i--) {
    const size = CONTAINER_SIZES_SCU[i];
    if (size > maxScu) continue;
    while (whole >= size) {
      out.push({ sizeScu: size, scuMilli: size * MILLI, partial: false });
      whole -= size;
    }
  }
  if (remainder > 0) out.push({ sizeScu: 1, scuMilli: remainder, partial: true });
  return out;
}

interface Part {
  destination: string | null;
  destinations: string[];
  amountMilli: number;
}

/** How a requirement's cargo divides between its deliveries (never guessed). */
export function destinationParts(req: RequirementCargo): Part[] {
  const deliveries = req.deliveries.length
    ? req.deliveries
    : req.deliveryLocations.map((locationName) => ({ locationName, scuMilli: null }));
  if (deliveries.length === 1) {
    return [{ destination: deliveries[0].locationName, destinations: [deliveries[0].locationName], amountMilli: req.totalScuMilli }];
  }
  const parts: Part[] = [];
  let known = 0;
  for (const d of deliveries) {
    if (d.scuMilli === null) continue;
    const amount = Math.min(d.scuMilli, req.totalScuMilli - known);
    if (amount <= 0) continue;
    parts.push({ destination: d.locationName, destinations: [d.locationName], amountMilli: amount });
    known += amount;
  }
  const unknown = deliveries.filter((d) => d.scuMilli === null).map((d) => d.locationName);
  const rest = req.totalScuMilli - known;
  if (rest > 0) {
    // Not covered by a known quantity: belongs to the requirement as a whole.
    parts.push({ destination: null, destinations: unknown.length ? unknown : deliveries.map((d) => d.locationName), amountMilli: rest });
  }
  return parts;
}

/** Estimated (or stored) boxes for every requirement, in requirement order. Ship-independent. */
export function estimateBoxes(reqs: readonly RequirementCargo[]): EstimatedBox[] {
  const boxes: EstimatedBox[] = [];
  for (const req of reqs) {
    const maxScu = allowedMaxScu(req.maxBoxScuMilli);
    const parts = destinationParts(req);
    let n = 0;
    const push = (b: { sizeScu: number; scuMilli: number; partial: boolean }, quality: EstimatedBox["quality"], part: Part) =>
      boxes.push({
        id: `${req.key}|${n++}`,
        requirementKey: req.key,
        contractRef: req.contractRef,
        commodityName: req.commodityName,
        sizeScu: b.sizeScu,
        scuMilli: b.scuMilli,
        quality,
        partial: b.partial,
        destination: part.destination,
        destinations: part.destinations,
        pickups: req.pickupLocations,
      });

    if (req.boxGroups.length > 0) {
      // Stored boxes, assigned to the parts in order of their known quantities.
      const exact = req.boxGroups.flatMap((g) => Array.from({ length: g.count }, () => g.sizeScuMilli));
      let partIndex = 0;
      let filled = 0;
      let covered = 0;
      for (const sizeMilli of exact) {
        while (partIndex < parts.length - 1 && filled >= parts[partIndex].amountMilli) {
          partIndex++;
          filled = 0;
        }
        push({ sizeScu: sizeMilli / MILLI, scuMilli: sizeMilli, partial: false }, "exact", parts[partIndex]);
        filled += sizeMilli;
        covered += sizeMilli;
      }
      const rest = req.totalScuMilli - covered;
      if (rest > 0) for (const b of greedyBoxes(rest, maxScu)) push(b, "estimated", parts[parts.length - 1]);
      continue;
    }
    for (const part of parts) for (const b of greedyBoxes(part.amountMilli, maxScu)) push(b, "estimated", part);
  }
  return boxes;
}

/* ---------------------------------------------------------------- colours */

export type ColorMode = "destination" | "commodity";

/** Neutral key for boxes whose destination split is unknown. */
export const SEVERAL_KEY = "~several";
export const SEVERAL_LABEL = "Several destinations";
export const SEVERAL_COLOR = "#8b93a1";

/**
 * Box colours: readable on the dark theme, and distinct from the pickup green (#6cc18a) and the
 * delivery / accent amber (#e8a33d) used for labels, and from the danger red.
 */
export const BOX_PALETTE = [
  "#5b8def", // blue
  "#a07cf0", // violet
  "#45b8d4", // cyan
  "#e06aa6", // pink
  "#7480f2", // indigo
  "#c48bec", // lilac
  "#6fa8dc", // sky
  "#c75fcb", // magenta
  "#8aa4c8", // steel
  "#e88fb4", // rose
] as const;

export function colorKey(box: EstimatedBox, mode: ColorMode): string {
  if (mode === "commodity") return commodityKey(box.commodityName);
  return box.destination === null ? SEVERAL_KEY : locationKey(box.destination);
}

export function colorLabel(box: EstimatedBox, mode: ColorMode): string {
  if (mode === "commodity") return box.commodityName.trim();
  return box.destination === null ? SEVERAL_LABEL : box.destination.trim();
}

export interface LegendEntry {
  key: string;
  label: string;
  color: string;
  scuMilli: number;
  boxes: number;
}

/**
 * One entry per colour key. Keys are sorted alphabetically by label before colours are assigned,
 * so a destination or commodity keeps its colour between reloads. "Several destinations" is
 * always neutral and listed last.
 */
export function legend(boxes: readonly EstimatedBox[], mode: ColorMode): LegendEntry[] {
  const groups = new Map<string, LegendEntry>();
  for (const box of boxes) {
    const key = colorKey(box, mode);
    let entry = groups.get(key);
    if (!entry) {
      entry = { key, label: colorLabel(box, mode), color: "", scuMilli: 0, boxes: 0 };
      groups.set(key, entry);
    } else if (compareNames(colorLabel(box, mode), entry.label) < 0) {
      entry.label = colorLabel(box, mode); // the same spelling whatever the input order
    }
    entry.scuMilli += box.scuMilli;
    entry.boxes += 1;
  }
  const named = [...groups.values()].filter((e) => e.key !== SEVERAL_KEY).sort((a, b) => compareNames(a.label, b.label) || compareNames(a.key, b.key));
  named.forEach((e, i) => (e.color = BOX_PALETTE[i % BOX_PALETTE.length]));
  const several = groups.get(SEVERAL_KEY);
  if (several) several.color = SEVERAL_COLOR;
  return several ? [...named, several] : named;
}

/** "#5b8def" scaled by `factor` (< 1 darker, > 1 lighter towards white). */
export function shade(hex: string, factor: number): string {
  const n = parseInt(hex.slice(1), 16);
  const channel = (c: number) => {
    const v = factor <= 1 ? c * factor : c + (255 - c) * (factor - 1);
    return Math.max(0, Math.min(255, Math.round(v)));
  };
  const r = channel((n >> 16) & 255);
  const g = channel((n >> 8) & 255);
  const b = channel(n & 255);
  return `#${((1 << 24) | (r << 16) | (g << 8) | b).toString(16).slice(1)}`;
}
