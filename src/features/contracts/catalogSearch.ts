/**
 * Pure helpers for the searchable catalog pickers (locations, commodities).
 * No DOM; unit-tested. Filtering is client-side over the listed catalog.
 */

import type { CatalogEntry } from "./types.ts";

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
  seenInHauling: boolean;
}

export type CatalogOption = { kind: "existing"; suggestion: Suggestion } | { kind: "create"; name: string };

export const MAX_OPTIONS = 50;

/** Secondary text for a location: "body · system" (whichever are known). */
export const locationDetail = (e: CatalogEntry): string | null => [e.body, e.system].filter(Boolean).join(" · ") || null;

/** Secondary text for a commodity: its kind/category. */
export const commodityDetail = (e: CatalogEntry): string | null => e.kind ?? null;

/**
 * Catalog entries plus names typed elsewhere in the current form,
 * de-duplicated by key (catalog spelling and details win).
 */
export function mergeSuggestions(
  catalog: readonly CatalogEntry[],
  formNames: readonly string[],
  detail: (entry: CatalogEntry) => string | null,
): Suggestion[] {
  const byKey = new Map<string, Suggestion>();
  for (const entry of catalog) {
    const key = nameKey(entry.name);
    if (!byKey.has(key)) byKey.set(key, { name: entry.name, detail: detail(entry), seenInHauling: entry.seenInHauling });
  }
  for (const name of formNames) {
    const key = nameKey(name);
    if (key && !byKey.has(key)) byKey.set(key, { name: normalizeName(name), detail: null, seenInHauling: false });
  }
  return [...byKey.values()];
}

const alpha = (a: string, b: string) => a.localeCompare(b, "en", { sensitivity: "base" }) || (a < b ? -1 : a > b ? 1 : 0);
const escapeRegExp = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Match tier for `query` against `name`, or null when it does not match:
 * 0 exact, 1 prefix, 2 word-start ("tress" → "Port Tressler"), 3 substring.
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
 * Ranked suggestions: by tier, then entries seen in hauling, then
 * alphabetical. At most `limit` results.
 */
export function rankSuggestions(suggestions: readonly Suggestion[], query: string, limit = MAX_OPTIONS): Suggestion[] {
  return suggestions
    .map((s) => ({ s, tier: matchTier(s.name, query) }))
    .filter((x): x is { s: Suggestion; tier: 0 | 1 | 2 | 3 } => x.tier !== null)
    .sort((a, b) => a.tier - b.tier || Number(b.s.seenInHauling) - Number(a.s.seenInHauling) || alpha(a.s.name, b.s.name))
    .slice(0, limit)
    .map((x) => x.s);
}

/** Listbox options: ranked matches, plus "create" when the text is new. */
export function buildOptions(suggestions: readonly Suggestion[], query: string, limit = MAX_OPTIONS): CatalogOption[] {
  const typed = normalizeName(query);
  const ranked = rankSuggestions(suggestions, query, limit).map((suggestion): CatalogOption => ({ kind: "existing", suggestion }));
  const exact = typed !== "" && suggestions.some((s) => nameKey(s.name) === nameKey(typed));
  return typed !== "" && !exact ? [...ranked, { kind: "create", name: typed }] : ranked;
}

/**
 * The canonical spelling when `text` equals a suggestion apart from case or
 * whitespace; otherwise `text` unchanged (free text is always allowed).
 */
export function canonicalSpelling(suggestions: readonly Suggestion[], text: string): string {
  const key = nameKey(text);
  if (!key) return text;
  return suggestions.find((s) => nameKey(s.name) === key)?.name ?? text;
}

/** Arrow-key movement through `count` options, wrapping; -1 means none active. */
export function moveActive(current: number, count: number, delta: 1 | -1): number {
  if (count === 0) return -1;
  if (current < 0) return delta === 1 ? 0 : count - 1;
  return (current + delta + count) % count;
}
