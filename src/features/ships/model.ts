// Ship selector logic (pure; Task 11).
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

/** Search results: favourites first, then the rest, each alphabetical. */
export function shipOptions(ships: readonly ShipEntry[], query: string): { favorites: ShipEntry[]; others: ShipEntry[] } {
  const found = ships.filter((s) => matchesQuery(s, query)).sort(byName);
  return { favorites: found.filter((s) => s.isFavorite), others: found.filter((s) => !s.isFavorite) };
}

/** "C2 Hercules · 696 SCU" (summary label). */
export function shipLabel(ship: Pick<ShipEntry, "name" | "cargoScu">): string {
  return `${ship.name} · ${ship.cargoScu.toLocaleString("en-US")} SCU`;
}

/** "100 ships · game 4.10.3-LIVE.13165946 · Star Citizen Wiki" (Settings). */
export function catalogLine(info: CatalogInfo): string {
  const source = /star citizen wiki/i.test(info.source) ? "Star Citizen Wiki" : info.source;
  return `${info.count} ship${info.count === 1 ? "" : "s"} · game ${info.gameVersion} · ${source}`;
}

/** Replaces one ship's favourite flag (optimistic star toggle). */
export function withFavorite(ships: readonly ShipEntry[], id: string, favorite: boolean): ShipEntry[] {
  return ships.map((s) => (s.id === id ? { ...s, isFavorite: favorite } : s));
}

export const STALE_SHIP_NOTICE = "Your selected ship is no longer in the list";
