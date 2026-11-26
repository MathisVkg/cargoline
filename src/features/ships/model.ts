// Ship selector logic (pure; Tasks 11 and 15).
import type { CatalogInfo, ShipEntry } from "./types.ts";

/** Lowercase, accents removed, whitespace collapsed. */
export function fold(text: string): string {
  return text
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/\s+/g, " ")
    .trim();
}

/** Every word of the query must appear in the name, the manufacturer or its code. */
export function matchesQuery(ship: ShipEntry, query: string): boolean {
  const words = fold(query).split(" ").filter(Boolean);
  if (words.length === 0) return true;
  const haystack = fold(`${ship.name} ${ship.manufacturer} ${ship.manufacturerCode}`);
  return words.every((w) => haystack.includes(w));
}

const byName = (a: ShipEntry, b: ShipEntry) => {
  const x = fold(a.name);
  const y = fold(b.name);
  return x < y ? -1 : x > y ? 1 : 0;
};

/**
 * Search results: Favourites (catalog or custom), then Custom ships, then All ships (catalog),
 * each alphabetical. A ship appears once.
 */
export function shipOptions(
  ships: readonly ShipEntry[],
  query: string,
): { favorites: ShipEntry[]; custom: ShipEntry[]; others: ShipEntry[] } {
  const found = ships.filter((s) => matchesQuery(s, query)).sort(byName);
  return {
    favorites: found.filter((s) => s.isFavorite),
    custom: found.filter((s) => !s.isFavorite && s.isCustom),
    others: found.filter((s) => !s.isFavorite && !s.isCustom),
  };
}

/** "C2 Hercules · 696 SCU" (summary label). */
export function shipLabel(ship: Pick<ShipEntry, "name" | "cargoScu">): string {
  return `${ship.name} · ${ship.cargoScu.toLocaleString("en-US")} SCU`;
}

/** "100 ships, 2 custom · game 4.10.3-LIVE.13165946 · Star Citizen Wiki" (Settings). */
export function catalogLine(info: CatalogInfo, customCount = 0): string {
  const source = /star citizen wiki/i.test(info.source) ? "Star Citizen Wiki" : info.source;
  const custom = customCount > 0 ? `, ${customCount} custom` : "";
  return `${info.count} ship${info.count === 1 ? "" : "s"}${custom} · game ${info.gameVersion} · ${source}`;
}

export const CUSTOM_NAME_MAX = 60;
export const CUSTOM_SCU_MAX = 100_000;

export interface CustomShipDraft {
  name: string;
  /** As typed in the field. */
  cargo: string;
}

export type CustomShipIssues = Partial<Record<"name" | "cargoScu", string>>;

const nameKey = (name: string) => name.trim().split(/\s+/).join(" ").toLowerCase();

/**
 * The checks the dialog shows inline before saving (the backend checks the same and is
 * authoritative). `others` are the other custom ships (for the unique-name rule).
 */
export function customShipIssues(draft: CustomShipDraft, others: readonly Pick<ShipEntry, "id" | "name">[], editingId: string | null = null): CustomShipIssues {
  const issues: CustomShipIssues = {};
  const name = draft.name.trim();
  if (!name) issues.name = "Enter a name.";
  else if ([...name].length > CUSTOM_NAME_MAX) issues.name = `Use at most ${CUSTOM_NAME_MAX} characters.`;
  else if (others.some((o) => o.id !== editingId && nameKey(o.name) === nameKey(name))) issues.name = "You already have a custom ship with this name.";
  const cargo = draft.cargo.trim().replace(/[\s,]/g, "");
  const value = /^\d+$/.test(cargo) ? Number(cargo) : NaN;
  if (!(Number.isInteger(value) && value >= 1 && value <= CUSTOM_SCU_MAX)) issues.cargoScu = "Enter a whole number of SCU from 1 to 100,000.";
  return issues;
}

/** The dialog's input once valid: trimmed name, whole SCU. */
export function customShipInput(draft: CustomShipDraft): { name: string; cargoScu: number } {
  return { name: draft.name.trim(), cargoScu: Number(draft.cargo.trim().replace(/[\s,]/g, "")) };
}

/** "Delete custom ship X? Contracts are not affected." */
export function deleteCustomQuestion(name: string): string {
  return `Delete custom ship ${name}? Contracts are not affected.`;
}

/** Replaces one ship's favourite flag (optimistic star toggle). */
export function withFavorite(ships: readonly ShipEntry[], id: string, favorite: boolean): ShipEntry[] {
  return ships.map((s) => (s.id === id ? { ...s, isFavorite: favorite } : s));
}

export const STALE_SHIP_NOTICE = "Your selected ship is no longer in the list";
