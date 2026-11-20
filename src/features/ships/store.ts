// Shared ship state for the top-bar selector and the Contracts summary (Task 11).
import { useSyncExternalStore } from "react";
import { describeError, toCommandError } from "../contracts/api";
import { getSelectedShip, listShips, setSelectedShip, setShipFavorite } from "./api";
import { withFavorite } from "./model";
import type { ShipEntry } from "./types";

export interface ShipState {
  loaded: boolean;
  ships: ShipEntry[];
  selected: ShipEntry | null;
  /** The saved ship left the catalog and the selection was cleared. */
  stale: boolean;
  error: string | null;
}

let state: ShipState = { loaded: false, ships: [], selected: null, stale: false, error: null };
const listeners = new Set<() => void>();
let loading: Promise<void> | null = null;

function set(next: Partial<ShipState>) {
  state = { ...state, ...next };
  listeners.forEach((l) => l());
}

function message(e: unknown): string {
  return describeError(toCommandError(e));
}

/** Loads the list and the selection once (again after a failure). */
export function loadShips(): Promise<void> {
  if (state.loaded && !state.error) return Promise.resolve();
  loading ??= Promise.all([listShips(), getSelectedShip()])
    .then(([ships, sel]) => set({ loaded: true, ships, selected: sel.ship, stale: sel.stale, error: null }))
    .catch((e) => set({ loaded: true, error: message(e) }))
    .finally(() => (loading = null));
  return loading;
}

/** Selects a ship (`null` = No ship) and persists it. */
export async function selectShip(id: string | null): Promise<void> {
  try {
    const saved = await setSelectedShip(id);
    set({ selected: saved.ship, stale: false, error: null });
  } catch (e) {
    set({ error: message(e) });
  }
}

/** Optimistic star toggle; rolled back if saving fails. Never changes the selection. */
export async function toggleFavorite(id: string, favorite: boolean): Promise<void> {
  set({ ships: withFavorite(state.ships, id, favorite) });
  try {
    await setShipFavorite(id, favorite);
  } catch (e) {
    set({ ships: withFavorite(state.ships, id, !favorite), error: message(e) });
  }
}

export function dismissStale(): void {
  set({ stale: false });
}

export function clearShipError(): void {
  set({ error: null });
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useShips(): ShipState {
  return useSyncExternalStore(subscribe, () => state);
}
