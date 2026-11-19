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
  /** Already used by the user (saved location or commodity in a contract): listed first when the search is empty. */
  used?: boolean;
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

/** Plain names (saved contracts, the current form). `used` marks names the user already saved. */
export const fromNames = (names: readonly string[], used = false): Suggestion[] =>
  names.map((name) => ({ name: normalizeName(name), detail: null, used })).filter((s) => s.name !== "");

/** The user's saved location catalog: always marked as used. */
export const fromUserLocations = (entries: readonly LocationEntry[]): Suggestion[] =>
  fromNames(
    entries.map((e) => e.name),
    true,
  );

/**
 * Merges suggestion lists, de-duplicated case- and whitespace-insensitively.
 * Earlier lists win, so pass the bundled list first: its spelling and detail
 * are shown. A duplicate's `used` flag is kept, so a bundled place the user
 * has saved still counts as used. Stored data is never changed by this.
 */
export function mergeSuggestions(...lists: readonly (readonly Suggestion[])[]): Suggestion[] {
  const byKey = new Map<string, Suggestion>();
  for (const list of lists) {
    for (const s of list) {
      const key = nameKey(s.name);
      if (!key) continue;
      const existing = byKey.get(key);
      if (!existing) byKey.set(key, s);
      else if (s.used && !existing.used) byKey.set(key, { ...existing, used: true });
    }
  }
  return [...byKey.values()];
}

/** Sort key ignoring leading punctuation and quotes: `"The Orphanage"` sorts under T. Display is unchanged. */
export const sortKey = (name: string): string => name.replace(/^[^\p{L}\p{N}]+/u, "");

/** Alphabetical, case-insensitive, ignoring leading punctuation; ties broken on the raw name. */
export const compareNames = (a: string, b: string): number =>
  sortKey(a).localeCompare(sortKey(b), "en", { sensitivity: "base" }) || (a < b ? -1 : a > b ? 1 : 0);

const alpha = compareNames;
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

/**
 * Ranked suggestions, at most `limit`.
 * - Empty search: entries the user already used first, then the rest, each alphabetical.
 * - Typed search: by match tier, then alphabetical (the `used` flag plays no part).
 */
export function rankSuggestions(suggestions: readonly Suggestion[], query: string, limit = MAX_OPTIONS): Suggestion[] {
  const empty = nameKey(query) === "";
  return suggestions
    .map((s) => ({ s, tier: matchTier(s.name, query) }))
    .filter((x): x is { s: Suggestion; tier: 0 | 1 | 2 | 3 } => x.tier !== null)
    .sort((a, b) =>
      empty
        ? Number(Boolean(b.s.used)) - Number(Boolean(a.s.used)) || alpha(a.s.name, b.s.name)
        : a.tier - b.tier || alpha(a.s.name, b.s.name),
    )
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
