import { invoke } from "@tauri-apps/api/core";
import type { CommandError, ContractDetail, ContractGrouping, LocationEntry, NewContractInput, ReferenceCommodity, ReferenceLocation } from "./types";

/** Typed wrappers around the Rust contract commands. */

export function createContract(input: NewContractInput): Promise<ContractDetail> {
  return call<ContractDetail>("create_contract", { input });
}

export function listActiveContracts(): Promise<ContractDetail[]> {
  return call<ContractDetail[]>("list_active_contracts");
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toCommandError(error);
  }
}

function isCommandError(value: unknown): value is CommandError {
  return typeof value === "object" && value !== null && typeof (value as { kind?: unknown }).kind === "string";
}

/** Normalises anything thrown by `invoke` into a CommandError. */
export function toCommandError(error: unknown): CommandError {
  if (isCommandError(error)) return error;
  const message = error instanceof Error ? error.message : String(error);
  return { kind: "internal", message };
}

/** Human-readable text for non-validation errors. */
export function describeError(error: CommandError): string {
  switch (error.kind) {
    case "validation":
      return "Some fields need attention.";
    case "unavailable":
      return `The local database is not available. ${error.message}`;
    case "database":
      return `A database operation failed and nothing was changed. ${error.message}`;
    case "internal":
      return `Unexpected error: ${error.message}`;
  }
}

export function getContractGrouping(): Promise<ContractGrouping> {
  return call<ContractGrouping>("get_contract_grouping");
}

/** Persists the display mode only; contract records are never modified. */
export function setContractGrouping(grouping: ContractGrouping): Promise<ContractGrouping> {
  return call<ContractGrouping>("set_contract_grouping", { grouping });
}

export function listLocations(): Promise<LocationEntry[]> {
  return call<LocationEntry[]>("list_locations");
}

/** Distinct commodity names already used in saved contracts. */
export function listUsedCommodities(): Promise<string[]> {
  return call<string[]>("list_used_commodities");
}

/** Bundled reference lists (static data shipped with the app). */
export function listReferenceLocations(): Promise<ReferenceLocation[]> {
  return call<ReferenceLocation[]>("list_reference_locations");
}

export function listReferenceCommodities(): Promise<ReferenceCommodity[]> {
  return call<ReferenceCommodity[]>("list_reference_commodities");
}
