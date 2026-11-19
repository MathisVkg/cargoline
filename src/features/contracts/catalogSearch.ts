/**
 * Pure helpers for the searchable "type or pick" fields (locations,
 * commodities). No DOM; unit-tested. Filtering is client-side.
 */

import type { LocationEntry, ReferenceCommodity, ReferenceLocation } from "./types.ts";

/** Trims and collapses inner whitespace (mirrors Rust `normalize_display`). */
export function normalizeName(name: string): string {
  return name.trim().replace(/\s+/g, " ");
}

/** Case- and whitespace-insensitive identity (mirrors Rust `normalize_key`). */
export function nameKey(name: string): string {
  return normalizeName(name).toLowerCase();
}

/** One selectable suggestion. */
export interface Suggestion {
  name: string;
  /** Small secondary text, e.g. "microTech · Stanton" or "Metal". */
  detail: string | null;
}

export type CatalogOption = { kind: "existing"; suggestion: Suggestion } | { kind: "create"; name: string };

export const MAX_OPTIONS = 50;

/** Bundled location with "body · system" as secondary text when known. */
export const fromReferenceLocation = (l: ReferenceLocation): Suggestion => ({
  name: l.name,
  detail: [l.body, l.system].filter(Boolean).join(" · ") || null,
});

/** "ProcessedGoods" → "Processed Goods", "Raw_Minerals" → "Raw Minerals". */
export const humanizeGroup = (group: string): string => group.replace(/_/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2");

/** Bundled commodity with its group as secondary text when known. */
export const fromReferenceCommodity = (c: ReferenceCommodity): Suggestion => ({
  name: c.name,
  detail: c.group ? humanizeGroup(c.group) : null,
});

/** Plain names (user catalog, saved contracts, the current form). */
export const fromNames = (names: readonly string[]): Suggestion[] =>
  names.map((name) => ({ name: normalizeName(name), detail: null })).filter((s) => s.name !== "");

export const fromUserLocations = (entries: readonly LocationEntry[]): Suggestion[] => fromNames(entries.map((e) => e.name));

/**
 * Merges suggestion lists, de-duplicated case- and whitespace-insensitively.
 * Earlier lists win, so pass the bundled list first: its spelling and detail
 * are shown. Stored data is never changed by this.
 */
export function mergeSuggestions(...lists: readonly (readonly Suggestion[])[]): Suggestion[] {
  const byKey = new Map<string, Suggestion>();
  for (const list of lists) {
    for (const s of list) {
      const key = nameKey(s.name);
      if (key && !byKey.has(key)) byKey.set(key, s);
    }
  }
  return [...byKey.values()];
}

const alpha = (a: string, b: string) => a.localeCompare(b, "en", { sensitivity: "base" }) || (a < b ? -1 : a > b ? 1 : 0);
const escapeRegExp = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Match tier for `query` against `name`, or null when it does not match:
 * 0 exact, 1 starts with the text, 2 a word starts with it
 * ("tress" → "Port Tressler"), 3 contains it anywhere.
 */
export function matchTier(name: string, query: string): 0 | 1 | 2 | 3 | null {
  const key = nameKey(name);
  const q = nameKey(query);
  if (!q) return 3;
  if (key === q) return 0;
  if (key.startsWith(q)) return 1;
  if (new RegExp(`[^\\p{L}\\p{N}]${escapeRegExp(q)}`, "u").test(key)) return 2;
  if (key.includes(q)) return 3;
  return null;
}

/** Ranked suggestions: by tier, then alphabetical. At most `limit` results. */
export function rankSuggestions(suggestions: readonly Suggestion[], query: string, limit = MAX_OPTIONS): Suggestion[] {
  return suggestions
    .map((s) => ({ s, tier: matchTier(s.name, query) }))
    .filter((x): x is { s: Suggestion; tier: 0 | 1 | 2 | 3 } => x.tier !== null)
    .sort((a, b) => a.tier - b.tier || alpha(a.s.name, b.s.name))
    .slice(0, limit)
    .map((x) => x.s);
}

/** Listbox options: ranked matches, plus "Add new …" when the text is new. */
export function buildOptions(suggestions: readonly Suggestion[], query: string, limit = MAX_OPTIONS): CatalogOption[] {
  const typed = normalizeName(query);
  const ranked = rankSuggestions(suggestions, query, limit).map((suggestion): CatalogOption => ({ kind: "existing", suggestion }));
  const exact = typed !== "" && suggestions.some((s) => nameKey(s.name) === nameKey(typed));
  return typed !== "" && !exact ? [...ranked, { kind: "create", name: typed }] : ranked;
}

/** Arrow-key movement through `count` options, wrapping; -1 means none active. */
export function moveActive(current: number, count: number, delta: 1 | -1): number {
  if (count === 0) return -1;
  if (current < 0) return delta === 1 ? 0 : count - 1;
  return (current + delta + count) % count;
}
