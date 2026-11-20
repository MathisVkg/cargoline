import { call } from "../contracts/api";
import type { ContractDetail } from "../contracts/types";
import type { ClearReport, History } from "./types";

export function getHistory(): Promise<History> {
  return call<History>("get_history");
}

/** Completed or abandoned → active, all or nothing. Errors: `notFound`, `notFinished`. */
export function restoreContracts(ids: number[]): Promise<ContractDetail[]> {
  return call<ContractDetail[]>("restore_contracts", { ids });
}

/** Erases every finished contract, their screenshots and empty runs. */
export function clearHistory(): Promise<ClearReport> {
  return call<ClearReport>("clear_history");
}
