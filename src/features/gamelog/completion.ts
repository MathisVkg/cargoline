// Complete contracts finished in game (Task 11 Part B), pure helpers.
import { contractRef } from "../contracts/grouping.ts";
import type { ContractDetail } from "../contracts/types.ts";
import { completionText, type Completion, type CompletionBanner } from "./flow.ts";

/** The reported contracts that are still active, once each, in arrival order. */
export function askRows(items: readonly Completion[], active: readonly ContractDetail[]): ContractDetail[] {
  const seen = new Set<number>();
  const rows: ContractDetail[] = [];
  for (const item of items) {
    if (seen.has(item.contractId)) continue;
    seen.add(item.contractId);
    const contract = active.find((c) => c.id === item.contractId);
    if (contract) rows.push(contract);
  }
  return rows;
}

/** The ids an automatic run finalises: only contracts still active (the others are left alone). */
export function autoIds(items: readonly Completion[], active: readonly ContractDetail[]): number[] {
  return askRows(items, active).map((c) => c.id);
}

/** "C06 completed from game" / "3 contracts completed from game". */
export function bannerText(banner: CompletionBanner): string {
  return completionText(banner.contractIds.map(contractRef));
}
