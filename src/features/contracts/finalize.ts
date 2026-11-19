/**
 * Pure helpers for completing / abandoning contracts (no DOM; unit-tested):
 * confirmation dialog content, the exact id list a bulk dialog sends, the
 * success notice, and expansion-state cleanup after contracts disappear.
 */

import { contractRef, type ContractGroup, type ExpansionState, type LocationGroup } from "./grouping.ts";
import type { ContractDetail, FinalStatus } from "./types.ts";

export type FinalAction = "complete" | "abandon";

export const statusFor = (action: FinalAction): FinalStatus => (action === "complete" ? "completed" : "abandoned");

/** Everything a confirmation dialog shows, plus the ids it will send. */
export interface Confirmation {
  action: FinalAction;
  /** Exactly the contracts shown; the request finalises these and nothing else. */
  ids: number[];
  refs: string[];
  heading: string;
  confirmLabel: string;
  /** Abandon uses danger styling on the confirm button. */
  danger: boolean;
  note: string;
  /** Single-contract details (null for bulk). */
  title: string | null;
  count: number;
  totalScuMilli: number;
  rewardAuec: number;
}

export function singleConfirmation(contract: ContractDetail, action: FinalAction): Confirmation {
  const ref = contractRef(contract.id);
  return {
    action,
    ids: [contract.id],
    refs: [ref],
    heading: action === "complete" ? `Mark ${ref} complete?` : `Abandon ${ref}?`,
    confirmLabel: action === "complete" ? "Mark complete" : "Abandon contract",
    danger: action === "abandon",
    note: "The contract and all its cargo will leave the active list and move to history.",
    title: contract.title,
    count: 1,
    totalScuMilli: contract.totalScuMilli,
    rewardAuec: contract.rewardAuec,
  };
}

/** Bulk dialog for exactly `contracts` (the active list at the moment it opened). */
export function bulkConfirmation(contracts: readonly ContractDetail[], action: FinalAction): Confirmation {
  const ordered = [...contracts].sort((a, b) => a.id - b.id);
  const n = ordered.length;
  const plural = n === 1 ? "contract" : "contracts";
  return {
    action,
    ids: ordered.map((c) => c.id),
    refs: ordered.map((c) => contractRef(c.id)),
    heading: action === "complete" ? `Complete ${n} active ${plural}?` : `Abandon ${n} active ${plural}?`,
    confirmLabel: action === "complete" ? `Complete ${n}` : `Abandon ${n}`,
    danger: action === "abandon",
    note: `These ${plural} and all their cargo will leave the active list and move to history.`,
    title: null,
    count: n,
    totalScuMilli: ordered.reduce((sum, c) => sum + c.totalScuMilli, 0),
    rewardAuec: ordered.reduce((sum, c) => sum + c.rewardAuec, 0),
  };
}

/** Short success text: "C02 completed", "3 contracts abandoned". */
export function successNotice(confirmation: Confirmation): string {
  const verb = statusFor(confirmation.action);
  return confirmation.count === 1 ? `${confirmation.refs[0]} ${verb}` : `${confirmation.count} contracts ${verb}`;
}

/** Keeps only expansion choices whose group or line still exists. */
export function pruneExpansion(state: ExpansionState, validKeys: ReadonlySet<string>): ExpansionState {
  const next = new Map<string, boolean>();
  for (const [key, value] of state) if (validKeys.has(key)) next.set(key, value);
  return next.size === state.size ? state : next;
}

/** Every expandable key that exists in the current views (groups and combined lines). */
export function validExpansionKeys(locationGroups: readonly LocationGroup[], contractGroups: readonly ContractGroup[]): Set<string> {
  const keys = new Set<string>();
  for (const group of locationGroups) {
    keys.add(group.key);
    for (const entry of group.entries) keys.add(entry.key);
  }
  for (const group of contractGroups) keys.add(group.key);
  return keys;
}
