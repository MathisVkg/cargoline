// Linked highlight between the Pickup / Destination summary and the box view (pure; Task 13.1).
//
// - A pickup line (location L, commodity C) ↔ boxes of commodity C picked up at L.
// - A destination line (L, C) ↔ boxes of commodity C going to L, including "Several
//   destinations" boxes whose possible destinations include L.
// - A location heading ↔ every box picked up at / going to that location.
// - A legend entry ↔ the boxes of that colour key.
// - A box ↔ its pickup lines and its destination line(s).
// Hover shows a selection; a click pins it; clicking the same thing again or Escape clears it.
import { locationKey, type Operation } from "../contracts/grouping.ts";
import { colorKey, type ColorMode, type EstimatedBox } from "./boxes.ts";
import { commodityKey, lineKey, locationHeadingKey } from "./summary.ts";

export type Selection =
  | { kind: "line"; operation: Operation; location: string; commodity: string }
  | { kind: "location"; operation: Operation; location: string }
  | { kind: "group"; key: string }
  | { kind: "box"; id: string };

export function selectionId(s: Selection): string {
  switch (s.kind) {
    case "line":
      return `line:${lineKey(s.operation, s.location, s.commodity)}`;
    case "location":
      return `location:${locationHeadingKey(s.operation, s.location)}`;
    case "group":
      return `group:${s.key}`;
    case "box":
      return `box:${s.id}`;
  }
}

const has = (names: readonly string[], location: string) => names.some((n) => locationKey(n) === locationKey(location));

/** The boxes a selection highlights. */
export function boxMatches(box: EstimatedBox, s: Selection, mode: ColorMode): boolean {
  switch (s.kind) {
    case "box":
      return box.id === s.id;
    case "group":
      return colorKey(box, mode) === s.key;
    case "location":
      return s.operation === "pickup" ? has(box.pickups, s.location) : has(box.destinations, s.location);
    case "line":
      if (commodityKey(box.commodityName) !== commodityKey(s.commodity)) return false;
      return s.operation === "pickup" ? has(box.pickups, s.location) : has(box.destinations, s.location);
  }
}

export function highlightedBoxes(boxes: readonly EstimatedBox[], s: Selection | null, mode: ColorMode): Set<string> {
  if (!s) return new Set();
  return new Set(boxes.filter((b) => boxMatches(b, s, mode)).map((b) => b.id));
}

/** Summary line keys (see `lineKey`) a box belongs to. */
export function linesForBox(box: EstimatedBox): string[] {
  return [
    ...box.pickups.map((l) => lineKey("pickup", l, box.commodityName)),
    ...box.destinations.map((l) => lineKey("delivery", l, box.commodityName)),
  ];
}

/** Summary line keys highlighted by a selection. */
export function highlightedLines(boxes: readonly EstimatedBox[], s: Selection | null, mode: ColorMode): Set<string> {
  if (!s) return new Set();
  if (s.kind === "line") return new Set([lineKey(s.operation, s.location, s.commodity)]);
  const lines = new Set<string>();
  for (const box of boxes) {
    if (!boxMatches(box, s, mode)) continue;
    if (s.kind === "location") {
      // Only the lines of that location and card.
      const list = s.operation === "pickup" ? box.pickups : box.destinations;
      for (const l of list) if (locationKey(l) === locationKey(s.location)) lines.add(lineKey(s.operation, l, box.commodityName));
    } else {
      for (const k of linesForBox(box)) lines.add(k);
    }
  }
  return lines;
}

/** Hover and pin state. */
export interface HighlightState {
  hover: Selection | null;
  pinned: Selection | null;
}

export const NO_HIGHLIGHT: HighlightState = { hover: null, pinned: null };

export type HighlightAction =
  | { type: "hover"; selection: Selection | null }
  | { type: "click"; selection: Selection }
  | { type: "clear" };

export function reduceHighlight(state: HighlightState, action: HighlightAction): HighlightState {
  switch (action.type) {
    case "hover":
      return { ...state, hover: action.selection };
    case "click": {
      const same = state.pinned && selectionId(state.pinned) === selectionId(action.selection);
      return { ...state, pinned: same ? null : action.selection };
    }
    case "clear":
      return NO_HIGHLIGHT;
  }
}

/** The selection in effect: a pin wins over hover. */
export function activeSelection(state: HighlightState): Selection | null {
  return state.pinned ?? state.hover;
}
