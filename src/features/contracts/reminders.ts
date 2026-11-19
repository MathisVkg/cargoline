/**
 * Pure helpers for the optional Pick Up / Turn In reminder ticks (no DOM;
 * unit-tested). Ticks are personal reminders only: they never change SCU
 * totals, grouping, ordering or completion.
 */

import type { ContractDetail, StopDetail } from "./types.ts";

export type TickState = "none" | "mixed" | "all";

/** State of one displayed row from the ticks of the stops it shows. */
export function tickState(doneAt: readonly (number | null)[]): TickState {
  const ticked = doneAt.filter((d) => d !== null).length;
  if (ticked === 0) return "none";
  return ticked === doneAt.length ? "all" : "mixed";
}

/** Clicking ticks everything unless everything is already ticked (mixed → all). */
export const nextDone = (state: TickState): boolean => state !== "all";

const mapStops = (contracts: readonly ContractDetail[], change: (stop: StopDetail) => StopDetail): ContractDetail[] =>
  contracts.map((c) => ({
    ...c,
    requirements: c.requirements.map((r) => ({
      ...r,
      pickups: r.pickups.map(change),
      deliveries: r.deliveries.map(change),
    })),
  }));

/** The current tick value of each stop in `stopIds`, for rolling back. */
export function captureReminders(contracts: readonly ContractDetail[], stopIds: readonly number[]): Map<number, number | null> {
  const wanted = new Set(stopIds);
  const snapshot = new Map<number, number | null>();
  for (const c of contracts)
    for (const r of c.requirements)
      for (const s of [...r.pickups, ...r.deliveries]) if (wanted.has(s.id)) snapshot.set(s.id, s.reminderDoneAt);
  return snapshot;
}

/** Optimistic update: tick (`doneAt` = timestamp) or untick (`null`) the stops. */
export function applyReminders(contracts: readonly ContractDetail[], stopIds: readonly number[], doneAt: number | null): ContractDetail[] {
  const ids = new Set(stopIds);
  return mapStops(contracts, (s) => (ids.has(s.id) ? { ...s, reminderDoneAt: s.reminderDoneAt !== null && doneAt !== null ? s.reminderDoneAt : doneAt } : s));
}

/** Writes back exact values per stop: a rollback snapshot or the server's response. */
export function restoreReminders(contracts: readonly ContractDetail[], values: ReadonlyMap<number, number | null>): ContractDetail[] {
  return mapStops(contracts, (s) => (values.has(s.id) ? { ...s, reminderDoneAt: values.get(s.id) ?? null } : s));
}

export type ReminderOperation = "pickup" | "delivery";

/** What one click on a row's tick control does: the same for the badge and the checkbox. */
export interface ReminderIntent {
  state: TickState;
  /** Value to send: true ticks, false unticks (mixed → tick all). */
  done: boolean;
  /** Every stop shown on the row (a combined row has several). */
  stopIds: number[];
}

export function reminderIntent(stops: readonly { stopId: number; reminderDoneAt: number | null }[]): ReminderIntent {
  const state = tickState(stops.map((s) => s.reminderDoneAt));
  return { state, done: nextDone(state), stopIds: stops.map((s) => s.stopId) };
}

/** Accessible label shared by the badge and the checkbox. */
export function reminderLabel(operation: ReminderOperation, state: TickState): string {
  const ticked = state === "all";
  if (operation === "pickup") return ticked ? "Mark not picked up" : "Mark picked up";
  return ticked ? "Mark not turned in" : "Mark turned in";
}

/** Visible badge text: "PICK UP" / "TURN IN", or "PICKED UP" / "TURNED IN" once fully ticked. */
export function reminderBadgeText(operation: ReminderOperation, state: TickState): string {
  const ticked = state === "all";
  return operation === "pickup" ? (ticked ? "Picked up" : "Pick up") : ticked ? "Turned in" : "Turn in";
}
