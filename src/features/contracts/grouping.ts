/**
 * Pure display transformations of saved contracts. Nothing here mutates the
 * input records or invents cargo values; every row keeps its original
 * contract, requirement and stop identifiers.
 */

import type { BoxGroupDetail, ContractDetail, RequirementDetail, StopDetail } from "./types.ts";

export type Operation = "pickup" | "delivery";

/**
 * How much cargo a stop handles, for display only.
 * - known: stored on the stop.
 * - derived: the stop is the requirement's only location for this operation,
 *   so it necessarily handles the whole requirement (never stored).
 * - unknown: shared between several locations with no stored split.
 */
export type StopQuantity =
  | { kind: "known"; scuMilli: number }
  | { kind: "derived"; scuMilli: number }
  | { kind: "unknown" };

/** Short display label for a contract, derived from its stable database id. */
export function contractRef(contractId: number): string {
  return `C${String(contractId).padStart(2, "0")}`;
}

/** Case- and whitespace-insensitive identity for a location name. */
export function locationKey(name: string): string {
  return name.trim().replace(/\s+/g, " ").toLowerCase();
}

export function stopQuantity(stop: StopDetail, siblings: readonly StopDetail[], requirement: RequirementDetail): StopQuantity {
  if (stop.quantityScuMilli !== null) return { kind: "known", scuMilli: stop.quantityScuMilli };
  if (siblings.length === 1) return { kind: "derived", scuMilli: requirement.quantityScuMilli };
  return { kind: "unknown" };
}

/* ------------------------------------------------------------------------- */
/* By Location                                                               */
/* ------------------------------------------------------------------------- */

export interface OperationRow {
  /** Stable identity: one row per stored stop. */
  key: string;
  /** Reminder tick of this stop (UTC ms) or null. Display only. */
  reminderDoneAt: number | null;
  operation: Operation;
  contractId: number;
  contractRef: string;
  contractTitle: string;
  requirementId: number;
  stopId: number;
  commodityName: string;
  locationName: string;
  quantity: StopQuantity;
  /** Total of the whole cargo objective (always available in row details). */
  requirementScuMilli: number;
  /** Delivery locations for a pickup row, pickup locations for a delivery row. */
  oppositeLocations: string[];
  maxBoxSizeScuMilli: number | null;
  boxGroups: BoxGroupDetail[];
}

/** Known/derived SCU for one operation at a location. */
export interface OperationTotal {
  scuMilli: number;
  /** False when at least one row's quantity is unknown (then scuMilli is a lower bound). */
  complete: boolean;
  rows: number;
}

/**
 * One manifest line. Usually a single stop; several stops are combined only
 * when they share contract, location, commodity and operation and every
 * quantity is known or unambiguously derived. `parts` keeps the originals.
 */
export interface ManifestEntry {
  key: string;
  operation: Operation;
  contractId: number;
  contractRef: string;
  contractTitle: string;
  commodityName: string;
  quantity: StopQuantity;
  /** Underlying stops (length 1 unless combined), in stable order. */
  parts: OperationRow[];
  /** Distinct opposite locations across all parts, in order of appearance. */
  oppositeLocations: string[];
}

export interface LocationGroup {
  key: string;
  locationName: string;
  /** Whether the location has at least one pickup (sorted first). */
  hasPickup: boolean;
  /** Every stop at this location, one row per stored stop. */
  rows: OperationRow[];
  /** Display lines: rows with eligible same-contract duplicates combined. */
  entries: ManifestEntry[];
  contractCount: number;
  operationCount: number;
  /** Totals per operation; pickup and delivery are never added together. */
  pickup: OperationTotal | null;
  delivery: OperationTotal | null;
}

const byNumber = (a: number, b: number) => a - b;
const compareNames = (a: string, b: string) => a.localeCompare(b, "en", { sensitivity: "base" }) || (a < b ? -1 : a > b ? 1 : 0);

function rowOrder(a: OperationRow, b: OperationRow): number {
  if (a.operation !== b.operation) return a.operation === "pickup" ? -1 : 1;
  return byNumber(a.contractId, b.contractId) || byNumber(a.requirementId, b.requirementId) || byNumber(a.stopId, b.stopId);
}

function total(rows: OperationRow[], operation: Operation): OperationTotal | null {
  const matching = rows.filter((r) => r.operation === operation);
  if (matching.length === 0) return null;
  let scuMilli = 0;
  let complete = true;
  for (const row of matching) {
    if (row.quantity.kind === "unknown") complete = false;
    else scuMilli += row.quantity.scuMilli;
  }
  return { scuMilli, complete, rows: matching.length };
}

const commodityKey = (name: string) => name.trim().replace(/\s+/g, " ").toLowerCase();

function uniqueLocations(names: string[]): string[] {
  const seen = new Set<string>();
  return names.filter((name) => {
    const key = locationKey(name);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function singleEntry(row: OperationRow): ManifestEntry {
  return {
    key: row.key,
    operation: row.operation,
    contractId: row.contractId,
    contractRef: row.contractRef,
    contractTitle: row.contractTitle,
    commodityName: row.commodityName,
    quantity: row.quantity,
    parts: [row],
    oppositeLocations: uniqueLocations(row.oppositeLocations),
  };
}

/**
 * Combines same-contract, same-commodity, same-operation rows of one location
 * when all their quantities are known or derived. Rows with an unknown
 * quantity are never summed and stay on their own line. Input must already
 * be in display order; combined lines take the position of their first row.
 */
export function aggregateRows(rows: readonly OperationRow[], groupKey: string): ManifestEntry[] {
  const buckets = new Map<string, OperationRow[]>();
  const order: (string | OperationRow)[] = [];
  for (const row of rows) {
    if (row.quantity.kind === "unknown") {
      order.push(row);
      continue;
    }
    const bucket = `${row.operation}|${row.contractId}|${commodityKey(row.commodityName)}`;
    const existing = buckets.get(bucket);
    if (existing) existing.push(row);
    else {
      buckets.set(bucket, [row]);
      order.push(bucket);
    }
  }
  return order.map((item) => {
    if (typeof item !== "string") return singleEntry(item);
    const parts = buckets.get(item)!;
    if (parts.length === 1) return singleEntry(parts[0]);
    const first = parts[0];
    const allKnown = parts.every((p) => p.quantity.kind === "known");
    const scuMilli = parts.reduce((sum, p) => sum + (p.quantity.kind === "unknown" ? 0 : p.quantity.scuMilli), 0);
    return {
      key: `${groupKey}|combined:${item}`,
      operation: first.operation,
      contractId: first.contractId,
      contractRef: first.contractRef,
      contractTitle: first.contractTitle,
      commodityName: first.commodityName,
      quantity: { kind: allKnown ? "known" : "derived", scuMilli },
      parts,
      oppositeLocations: uniqueLocations(parts.flatMap((p) => p.oppositeLocations)),
    };
  });
}

/**
 * Groups every stop of the given contracts by location: locations with at
 * least one pickup first, then delivery-only locations, each alphabetical.
 */
export function groupByLocation(contracts: readonly ContractDetail[]): LocationGroup[] {
  const groups = new Map<string, { locationName: string; rows: OperationRow[] }>();
  // Deterministic input order makes the chosen display spelling stable.
  const ordered = [...contracts].sort((a, b) => byNumber(a.id, b.id));

  for (const contract of ordered) {
    for (const requirement of contract.requirements) {
      const sides: [Operation, StopDetail[], StopDetail[]][] = [
        ["pickup", requirement.pickups, requirement.deliveries],
        ["delivery", requirement.deliveries, requirement.pickups],
      ];
      for (const [operation, stops, opposite] of sides) {
        for (const stop of stops) {
          const key = locationKey(stop.locationName);
          let group = groups.get(key);
          if (!group) {
            group = { locationName: stop.locationName.trim(), rows: [] };
            groups.set(key, group);
          }
          group.rows.push({
            key: `stop:${stop.id}`,
            operation,
            contractId: contract.id,
            contractRef: contractRef(contract.id),
            contractTitle: contract.title,
            requirementId: requirement.id,
            stopId: stop.id,
            reminderDoneAt: stop.reminderDoneAt ?? null,
            commodityName: requirement.commodityName,
            locationName: stop.locationName,
            quantity: stopQuantity(stop, stops, requirement),
            requirementScuMilli: requirement.quantityScuMilli,
            oppositeLocations: opposite.map((s) => s.locationName),
            maxBoxSizeScuMilli: requirement.maxBoxSizeScuMilli,
            boxGroups: requirement.boxGroups,
          });
        }
      }
    }
  }

  return [...groups.entries()]
    .map(([key, { locationName, rows }]) => {
      const sorted = [...rows].sort(rowOrder);
      const groupKey = `location:${key}`;
      return {
        key: groupKey,
        locationName,
        hasPickup: sorted.some((r) => r.operation === "pickup"),
        rows: sorted,
        entries: aggregateRows(sorted, groupKey),
        contractCount: new Set(sorted.map((r) => r.contractId)).size,
        operationCount: sorted.length,
        pickup: total(sorted, "pickup"),
        delivery: total(sorted, "delivery"),
      };
    })
    .sort(
      (a, b) =>
        Number(b.hasPickup) - Number(a.hasPickup) ||
        compareNames(a.locationName, b.locationName) ||
        compareNames(a.key, b.key),
    );
}

/* ------------------------------------------------------------------------- */
/* By Contract                                                               */
/* ------------------------------------------------------------------------- */

export interface StopEntry {
  stopId: number;
  locationName: string;
  quantity: StopQuantity;
  reminderDoneAt: number | null;
}

export interface ObjectiveRow {
  key: string;
  requirementId: number;
  index: number;
  commodityName: string;
  totalScuMilli: number;
  maxBoxSizeScuMilli: number | null;
  boxGroups: BoxGroupDetail[];
  pickups: StopEntry[];
  deliveries: StopEntry[];
}

export interface ContractGroup {
  key: string;
  contractId: number;
  contractRef: string;
  title: string;
  totalScuMilli: number;
  rewardAuec: number;
  acceptedAt: number;
  runNumber: number;
  runDate: string;
  objectives: ObjectiveRow[];
}

const entries = (stops: StopDetail[], requirement: RequirementDetail): StopEntry[] =>
  stops.map((stop) => ({
    stopId: stop.id,
    locationName: stop.locationName,
    quantity: stopQuantity(stop, stops, requirement),
    reminderDoneAt: stop.reminderDoneAt ?? null,
  }));

/** One group per contract, newest accepted first. */
export function groupByContract(contracts: readonly ContractDetail[]): ContractGroup[] {
  return [...contracts]
    .sort((a, b) => b.acceptedAt - a.acceptedAt || b.id - a.id)
    .map((contract) => ({
      key: `contract:${contract.id}`,
      contractId: contract.id,
      contractRef: contractRef(contract.id),
      title: contract.title,
      // Computed by Rust from requirements only.
      totalScuMilli: contract.totalScuMilli,
      rewardAuec: contract.rewardAuec,
      acceptedAt: contract.acceptedAt,
      runNumber: contract.run.runNumber,
      runDate: contract.run.runDate,
      objectives: contract.requirements.map((requirement, index) => ({
        key: `requirement:${requirement.id}`,
        requirementId: requirement.id,
        index,
        commodityName: requirement.commodityName,
        totalScuMilli: requirement.quantityScuMilli,
        maxBoxSizeScuMilli: requirement.maxBoxSizeScuMilli,
        boxGroups: requirement.boxGroups,
        pickups: entries(requirement.pickups, requirement),
        deliveries: entries(requirement.deliveries, requirement),
      })),
    }));
}

/* ------------------------------------------------------------------------- */
/* Box information                                                           */
/* ------------------------------------------------------------------------- */

export type BoxInfo =
  | { kind: "breakdown"; groups: BoxGroupDetail[]; estimated: boolean }
  | { kind: "max"; maxBoxSizeScuMilli: number }
  | { kind: "none" };

/** Stored breakdown first; otherwise the known max box size; never invented. */
export function boxInfo(boxGroups: readonly BoxGroupDetail[], maxBoxSizeScuMilli: number | null): BoxInfo {
  if (boxGroups.length > 0) {
    return {
      kind: "breakdown",
      groups: [...boxGroups].sort((a, b) => b.boxSizeScuMilli - a.boxSizeScuMilli),
      estimated: boxGroups.some((g) => g.sourceQuality === "estimated"),
    };
  }
  if (maxBoxSizeScuMilli !== null) return { kind: "max", maxBoxSizeScuMilli };
  return { kind: "none" };
}

/* ------------------------------------------------------------------------- */
/* Expansion state (temporary, per display mode)                             */
/* ------------------------------------------------------------------------- */

/** Explicit user choices; groups without one use the mode's default. */
export type ExpansionState = ReadonlyMap<string, boolean>;

export function isExpanded(state: ExpansionState, key: string, defaultExpanded: boolean): boolean {
  return state.get(key) ?? defaultExpanded;
}

export function toggleExpanded(state: ExpansionState, key: string, defaultExpanded: boolean): ExpansionState {
  const next = new Map(state);
  next.set(key, !isExpanded(state, key, defaultExpanded));
  return next;
}

export function setAllExpanded(keys: readonly string[], expanded: boolean): ExpansionState {
  return new Map(keys.map((key) => [key, expanded]));
}

/** Keys of combined manifest lines (their detail rows start collapsed). */
export function isCombinedKey(key: string): boolean {
  return key.includes("|combined:");
}
