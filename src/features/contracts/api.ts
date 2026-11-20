import { invoke } from "@tauri-apps/api/core";
import type {
  CommandError,
  ContractDetail,
  ContractGrouping,
  FinalStatus,
  LocationEntry,
  NewContractInput,
  ReferenceCommodity,
  ReferenceLocation,
  StopDetail,
} from "./types";

/** Typed wrappers around the Rust contract commands. */

/**
 * Saves a new contract; `captureId` (the form's unsaved screenshot) is linked in the same
 * transaction. `fromOcr`: the form was filled from that screenshot (source "ocr").
 */
export function createContract(
  input: NewContractInput,
  captureId: number | null = null,
  fromOcr = false,
  gameMissionId: string | null = null,
  fromGameLog = false,
): Promise<ContractDetail> {
  return call<ContractDetail>("create_contract", { input, captureId, fromOcr, gameMissionId, fromGameLog });
}

export function listActiveContracts(): Promise<ContractDetail[]> {
  return call<ContractDetail[]>("list_active_contracts");
}

export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
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
    case "notFound":
      return "This contract no longer exists.";
    case "notFinished":
      return `This contract is ${error.status}, not finished.`;
    case "notActive":
      return `This contract is already ${error.status}.`;
    case "conflict":
      return "The active contract list changed in the meantime. Nothing was changed; the list has been reloaded.";
    case "capture":
    case "ocr":
    case "cannotReactivate":
      return error.message;
    case "unknownShip":
      return "This ship is not in the catalog.";
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

/** Marks one active contract completed (run and cargo unchanged). */
export function completeContract(id: number): Promise<ContractDetail> {
  return call<ContractDetail>("complete_contract", { id });
}

/** Marks one active contract abandoned (run and cargo unchanged). */
export function abandonContract(id: number): Promise<ContractDetail> {
  return call<ContractDetail>("abandon_contract", { id });
}

/**
 * Finalises exactly `ids` (the contracts the confirmation dialog showed), all
 * or nothing. A stale list rejects with `{ kind: "conflict" }`.
 */
export function finalizeActiveContracts(ids: number[], status: FinalStatus): Promise<ContractDetail[]> {
  return call<ContractDetail[]>("finalize_active_contracts", { ids, status });
}

/**
 * Undo of a recent completion: sets `completed` contracts (finished within 5 minutes) back to
 * active, all or nothing. Run, ticks and cargo are untouched.
 */
export function reactivateContracts(ids: number[]): Promise<ContractDetail[]> {
  return call<ContractDetail[]>("reactivate_contracts", { ids });
}

/** Edits an active contract (same input and validation as createContract). */
export function updateContract(id: number, input: NewContractInput): Promise<ContractDetail> {
  return call<ContractDetail>("update_contract", { id, input });
}

/** Ticks or unticks the Pick Up / Turn In reminder of the given stops. */
export function setStopReminder(stopIds: number[], done: boolean): Promise<StopDetail[]> {
  return call<StopDetail[]>("set_stop_reminder", { stopIds, done });
}
