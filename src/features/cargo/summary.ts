// Cargo page: Pickup and Destination summary (pure; Task 13). Milli-SCU integers throughout.
//
// Built on the By Location rows (`groupByLocation`): one row per stored stop, with the stop's
// quantity already classified as known / derived / unknown (`stopQuantity`). Nothing here
// invents a split:
// - a line merges every stop of the same commodity at the same location for one operation,
//   across contracts; known and derived quantities are summed, any unknown part makes the
//   line "N SCU + ?" (or "? SCU" when nothing is known);
// - location and card totals are the sum of their lines, with the same "+ ?" rule;
// - pickup and delivery totals are never added to each other or to the active SCU, which
//   counts each requirement once (exactly like the Contracts summary);
// - ticks only dim a line (all its stops ticked); totals never change because of them.
import { contractRef, groupByLocation, locationKey, type Operation, type OperationRow } from "../contracts/grouping.ts";
import { formatScu } from "../contracts/scu.ts";
import { summarizeContracts } from "../contracts/summary.ts";
import type { ContractDetail } from "../contracts/types.ts";

/** A quantity that may be partly unknown. `complete: false` → `scuMilli` is a lower bound. */
export interface Amount {
  scuMilli: number;
  complete: boolean;
  /** False when no part at all is known (shown "? SCU"). */
  anyKnown: boolean;
}

export interface SummaryLine {
  key: string;
  commodityName: string;
  amount: Amount;
  /** Contracts contributing to the line, e.g. ["C05", "C06"]. */
  contractRefs: string[];
  stopIds: number[];
  /** Every stop of the line is ticked (picked up / turned in): dimmed. */
  allTicked: boolean;
}

export interface SummaryLocation {
  key: string;
  locationName: string;
  total: Amount;
  /** Alphabetical by commodity. */
  lines: SummaryLine[];
}

export interface OperationSummary {
  operation: Operation;
  total: Amount;
  /** Alphabetical. */
  locations: SummaryLocation[];
}

export interface CargoSummary {
  /** Each requirement once; equals the Contracts summary's total SCU. */
  activeScuMilli: number;
  activeContracts: number;
  pickup: OperationSummary;
  delivery: OperationSummary;
}

const EMPTY: Amount = { scuMilli: 0, complete: true, anyKnown: false };

function addRow(amount: Amount, row: OperationRow): Amount {
  if (row.quantity.kind === "unknown") return { ...amount, complete: false };
  return { scuMilli: amount.scuMilli + row.quantity.scuMilli, complete: amount.complete, anyKnown: true };
}

function addAmounts(a: Amount, b: Amount): Amount {
  return { scuMilli: a.scuMilli + b.scuMilli, complete: a.complete && b.complete, anyKnown: a.anyKnown || b.anyKnown };
}

export const compareNames = (a: string, b: string) =>
  a.localeCompare(b, "en", { sensitivity: "base" }) || (a < b ? -1 : a > b ? 1 : 0);
/** Case- and whitespace-insensitive identity for a commodity name. */
export const commodityKey = (name: string) => name.trim().replace(/\s+/g, " ").toLowerCase();

/** Identity of a summary line; the box view uses the same keys for the linked highlight. */
export function lineKey(operation: Operation, location: string, commodity: string): string {
  return `${operation}|${locationKey(location)}|${commodityKey(commodity)}`;
}

/** Identity of a location heading in one card. */
export function locationHeadingKey(operation: Operation, location: string): string {
  return `${operation}|${locationKey(location)}`;
}

function operationSummary(contracts: readonly ContractDetail[], operation: Operation): OperationSummary {
  const locations: SummaryLocation[] = [];
  for (const group of groupByLocation(contracts)) {
    const rows = group.rows.filter((r) => r.operation === operation);
    if (rows.length === 0) continue;
    const lines = new Map<string, SummaryLine & { ticked: boolean[] }>();
    for (const row of rows) {
      const key = commodityKey(row.commodityName);
      let line = lines.get(key);
      if (!line) {
        line = {
          key: lineKey(operation, group.locationName, row.commodityName),
          commodityName: row.commodityName.trim(),
          amount: EMPTY,
          contractRefs: [],
          stopIds: [],
          allTicked: false,
          ticked: [],
        };
        lines.set(key, line);
      }
      line.amount = addRow(line.amount, row);
      if (!line.contractRefs.includes(row.contractRef)) line.contractRefs.push(row.contractRef);
      line.stopIds.push(row.stopId);
      line.ticked.push(row.reminderDoneAt !== null);
    }
    const sorted = [...lines.values()]
      .map(({ ticked, ...line }) => ({ ...line, allTicked: ticked.every(Boolean) }))
      .sort((a, b) => compareNames(a.commodityName, b.commodityName));
    locations.push({
      key: locationHeadingKey(operation, group.locationName),
      locationName: group.locationName,
      total: sorted.reduce((t, l) => addAmounts(t, l.amount), EMPTY),
      lines: sorted,
    });
  }
  locations.sort((a, b) => compareNames(a.locationName, b.locationName) || compareNames(a.key, b.key));
  return { operation, total: locations.reduce((t, l) => addAmounts(t, l.total), EMPTY), locations };
}

/** The Cargo page summary for the active contracts. */
export function summarizeCargo(contracts: readonly ContractDetail[]): CargoSummary {
  const active = contracts.filter((c) => c.status === "active");
  const { activeCount, totalScuMilli } = summarizeContracts(active);
  return {
    activeScuMilli: totalScuMilli,
    activeContracts: activeCount,
    pickup: operationSummary(active, "pickup"),
    delivery: operationSummary(active, "delivery"),
  };
}

/** "84 SCU", "12 SCU + ?", "? SCU" (the By Location wording). */
export function formatAmount(amount: Amount): string {
  if (amount.complete) return `${formatScu(amount.scuMilli)} SCU`;
  return amount.anyKnown ? `${formatScu(amount.scuMilli)} SCU + ?` : "? SCU";
}

/** Screen-reader wording for an incomplete amount. */
export function describeAmount(amount: Amount): string {
  if (amount.complete) return formatAmount(amount);
  return amount.anyKnown ? `at least ${formatScu(amount.scuMilli)} SCU, part unknown` : "unknown SCU";
}

/* ------------------------------------------------------- Task 13.1 export */

/** One cargo objective, for the isometric arrangement (Task 13.1). */
export interface RequirementCargo {
  key: string;
  contractId: number;
  contractRef: string;
  requirementId: number;
  commodityName: string;
  totalScuMilli: number;
  maxBoxScuMilli: number | null;
  pickupLocations: string[];
  deliveryLocations: string[];
  /** Each delivery with its stored quantity (null = unknown split). */
  deliveries: { locationName: string; scuMilli: number | null }[];
  /** Stored box groups (`exact` only), largest first. */
  boxGroups: { sizeScuMilli: number; count: number }[];
}

/** Every requirement of the active contracts, by contract then requirement id. */
export function requirementCargo(contracts: readonly ContractDetail[]): RequirementCargo[] {
  return contracts
    .filter((c) => c.status === "active")
    .sort((a, b) => a.id - b.id)
    .flatMap((c) =>
      [...c.requirements]
        .sort((a, b) => a.id - b.id)
        .map((r) => ({
          key: `requirement:${r.id}`,
          contractId: c.id,
          contractRef: contractRef(c.id),
          requirementId: r.id,
          commodityName: r.commodityName,
          totalScuMilli: r.quantityScuMilli,
          maxBoxScuMilli: r.maxBoxSizeScuMilli,
          pickupLocations: r.pickups.map((s) => s.locationName),
          deliveryLocations: r.deliveries.map((s) => s.locationName),
          deliveries: r.deliveries.map((s) => ({ locationName: s.locationName, scuMilli: s.quantityScuMilli })),
          boxGroups: r.boxGroups
            .filter((g) => g.sourceQuality === "exact")
            .map((g) => ({ sizeScuMilli: g.boxSizeScuMilli, count: g.boxCount }))
            .sort((a, b) => b.sizeScuMilli - a.sizeScuMilli),
        })),
    );
}
