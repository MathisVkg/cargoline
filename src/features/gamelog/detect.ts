// What Settings does with a Detect result. Pure.
import type { DetectResult, FoundLog } from "./types.ts";

export type DetectOutcome =
  | { kind: "none"; message: string }
  /** Fill the path and save it (the backend validates it). */
  | { kind: "one"; message: string; log: FoundLog }
  /** Let the user choose; the first is recommended (LIVE, then most recent). */
  | { kind: "several"; message: string; choices: FoundLog[] };

export function detectOutcome(result: DetectResult): DetectOutcome {
  const [first] = result.found;
  if (!first) return { kind: "none", message: result.message };
  if (result.found.length === 1) return { kind: "one", message: result.message, log: first };
  return { kind: "several", message: result.message, choices: result.found };
}

/** "LIVE · modified 19/11/2026" for the choice list. */
export function choiceLabel(log: FoundLog, recommended: boolean): string {
  const when = log.modifiedMs ? ` · modified ${new Date(log.modifiedMs).toLocaleDateString()}` : "";
  return `${log.channel}${when}${recommended ? " (recommended)" : ""}`;
}
