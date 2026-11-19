// Text and action of Game.log notices on the Contracts page (pure).
import { contractRef } from "../contracts/grouping.ts";
import type { ContractDetail } from "../contracts/types.ts";
import type { FinalAction } from "../contracts/finalize.ts";
import type { FlowNotice } from "./flow.ts";
import { MISSION_STATE_COMPLETED } from "./types.ts";

/** Short readable state: `MISSION_STATE_FAILED` → `failed`. */
export function stateLabel(state: string): string {
  return state.replace(/^MISSION_STATE_/, "").toLowerCase().replace(/_/g, " ");
}

/**
 * - Accepted (automatic capture off): "Accepted in game: <title>" · Add contract.
 * - Completed: "C06 completed in game" · Mark complete (the existing confirmation).
 * - Any other end state: "C06 ended in game (<state>)" · Abandon contract.
 * Null when the contract is no longer active (the notice no longer applies).
 */
export function noticeView(
  notice: FlowNotice,
  active: readonly ContractDetail[],
): { text: string; action: "add" | FinalAction; label: string } | null {
  if (notice.kind === "accepted") return { text: `Accepted in game: ${notice.event.title}`, action: "add", label: "Add contract" };
  if (!active.some((c) => c.id === notice.contractId)) return null;
  const ref = contractRef(notice.contractId);
  return notice.state === MISSION_STATE_COMPLETED
    ? { text: `${ref} completed in game`, action: "complete", label: "Mark complete" }
    : { text: `${ref} ended in game (${stateLabel(notice.state)})`, action: "abandon", label: "Abandon contract" };
}
