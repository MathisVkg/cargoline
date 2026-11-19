/**
 * Pure logic for the Contracts summary row (no DOM; unit-tested).
 *
 * Totals come from Rust: each contract's `totalScuMilli` is already the sum
 * of its cargo requirement quantities, so pickup and delivery stops are never
 * added together here. Everything stays in integer milli-SCU until display.
 */

import { MILLI_PER_SCU } from "./scu.ts";
import type { ContractDetail } from "./types.ts";

export interface ContractsSummary {
  activeCount: number;
  /** Sum of the active contracts' requirement quantities, in milli-SCU. */
  totalScuMilli: number;
}

/** Summary of the active contracts in `contracts` (other statuses are ignored). */
export function summarizeContracts(contracts: readonly ContractDetail[]): ContractsSummary {
  let activeCount = 0;
  let totalScuMilli = 0;
  for (const contract of contracts) {
    if (contract.status !== "active") continue;
    activeCount += 1;
    totalScuMilli += contract.totalScuMilli;
  }
  return { activeCount, totalScuMilli };
}

/** Where the active list stands; the summary shows numbers only when "ready". */
export type ListStatus =
  | { status: "loading" }
  | { status: "error" }
  | { status: "ready"; contracts: readonly ContractDetail[] };

/** `null` while loading or on error, so no misleading zeros are shown. */
export function summaryForList(list: ListStatus): ContractsSummary | null {
  return list.status === "ready" ? summarizeContracts(list.contracts) : null;
}

/**
 * Ship capacity meter state.
 * - unknown: no ship selected (neutral, not an error).
 * - known: `percent` is the real value (may exceed 100); `fill` is clamped to 0..1.
 */
export type CapacityState =
  | { kind: "unknown" }
  | { kind: "known"; percent: number; fill: number; over: boolean };

/**
 * Informational only: compares active SCU with a ship capacity (whole SCU).
 * Never blocks or changes anything.
 */
export function capacityState(activeScuMilli: number, capacityScu: number | null): CapacityState {
  if (capacityScu === null || !Number.isFinite(capacityScu) || capacityScu <= 0) return { kind: "unknown" };
  const capacityMilli = capacityScu * MILLI_PER_SCU;
  const ratio = activeScuMilli / capacityMilli;
  return { kind: "known", percent: ratio * 100, fill: Math.min(1, Math.max(0, ratio)), over: activeScuMilli > capacityMilli };
}

/** "0 %", "<1 %", "50 %", "100 %", "112 %". Never rounds a real overload down to 100 %. */
export function formatPercent(percent: number): string {
  if (percent <= 0) return "0 %";
  if (percent < 1) return "<1 %";
  const rounded = Math.round(percent);
  // Rounding must not cross the 100 % line: 100.4 % shows 101 %, 99.6 % shows 99 %.
  if (percent > 100 && rounded <= 100) return "101 %";
  if (percent < 100 && rounded >= 100) return "99 %";
  return `${rounded} %`;
}
