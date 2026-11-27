// History page text and formatting (pure).
import { contractRef } from "../contracts/grouping.ts";
import { formatAuec, formatScu } from "../contracts/scu.ts";
import type { ContractDetail, ContractStatus } from "../contracts/types.ts";
import type { ClearReport, History, HistoryRun, HistoryStats } from "./types.ts";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const pad = (n: number, width = 2) => String(n).padStart(width, "0");

export const EMPTY_STATS: HistoryStats = { completedScuMilli: 0, completedContracts: 0, estimatedAuec: 0 };

/** "#003". */
export function runLabel(runNumber: number): string {
  return `#${pad(runNumber, 3)}`;
}

/** `2026-11-20` → "20 Nov 2026" (a calendar date: no time zone shift). */
export function formatRunDate(date: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!m) return date;
  return `${Number(m[3])} ${MONTHS[Number(m[2]) - 1] ?? m[2]} ${m[1]}`;
}

/** "Run #003 · 20 Nov 2026". */
export function runHeading(run: Pick<HistoryRun, "runNumber" | "runDate">): string {
  return `Run ${runLabel(run.runNumber)} · ${formatRunDate(run.runDate)}`;
}

/** "45 min", "2 h 15 min", "26 h 5 min", "2 h" (rounded to the minute). */
export function formatDuration(ms: number): string {
  const minutes = Math.max(0, Math.round(ms / 60_000));
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h === 0) return `${m} min`;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

/** "≈ 356,800 aUEC/h" (the backend already rounds to 100). */
export function formatRate(auecPerHour: number): string {
  return `≈ ${formatAuec(Math.round(auecPerHour / 100) * 100)} aUEC/h`;
}

export const RATE_TOOLTIP = "Estimate: completed rewards ÷ time with at least one contract running (accepted → finished).";

/**
 * The run heading's totals: "16 completed · 5 abandoned · 509 SCU · 802,750 aUEC" (SCU and aUEC:
 * completed only), then the active time and, when known, the rate.
 */
export function runTotalsParts(run: HistoryRun): { base: string; rate: string | null } {
  const parts = [`${run.completed} completed`];
  if (run.abandoned > 0) parts.push(`${run.abandoned} abandoned`);
  parts.push(`${formatScu(run.completedScuMilli)} SCU`, `${formatAuec(run.earnedAuec)} aUEC`);
  if (run.activeDurationMs > 0) parts.push(formatDuration(run.activeDurationMs));
  return { base: parts.join(" · "), rate: run.auecPerHour === null ? null : formatRate(run.auecPerHour) };
}

/** The whole totals line, e.g. "… · 802,750 aUEC · 2 h 15 min · ≈ 356,800 aUEC/h". */
export function runTotals(run: HistoryRun): string {
  const { base, rate } = runTotalsParts(run);
  return rate ? `${base} · ${rate}` : base;
}

/** Finish time in local time: "20 Nov 14:32". */
export function formatFinished(ms: number | null): string {
  if (ms === null) return "—";
  const d = new Date(ms);
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Text, not only colour. */
export function statusLabel(status: ContractStatus): string {
  return status === "completed" ? "Completed" : status === "abandoned" ? "Abandoned" : "Active";
}

export function hasHistory(history: History | null): boolean {
  return !!history && history.runs.length > 0;
}

/** Totals for the Clear History dialog. */
export function finishedCounts(history: History | null): { completed: number; abandoned: number } {
  const runs = history?.runs ?? [];
  return {
    completed: runs.reduce((n, r) => n + r.completed, 0),
    abandoned: runs.reduce((n, r) => n + r.abandoned, 0),
  };
}

const contracts = (n: number, word: string) => `${n} ${word} contract${n === 1 ? "" : "s"}`;

/** The Clear History dialog body: exactly what goes. */
export function clearText(completed: number, abandoned: number): string {
  return (
    `Erase all history: ${contracts(completed, "completed")} and ${contracts(abandoned, "abandoned")}, their screenshots, ` +
    "and the lifetime statistics. Active contracts and settings are not affected. This can't be undone."
  );
}

/** The notice after Clear History. */
export function clearNotice(report: ClearReport): string {
  const total = report.completed + report.abandoned;
  const parts = [
    `${total} contract${total === 1 ? "" : "s"}`,
    `${report.runs} run${report.runs === 1 ? "" : "s"}`,
    `${report.captures} screenshot${report.captures === 1 ? "" : "s"}`,
  ];
  return `History cleared: ${parts.join(", ")} removed.`;
}

/** "Restore C06 to active? It goes back to the Contracts page in its original run (Run #003)." */
export function restoreQuestion(contract: Pick<ContractDetail, "id" | "run">): string {
  return `Restore ${contractRef(contract.id)} to active? It goes back to the Contracts page in its original run (Run ${runLabel(contract.run.runNumber)}).`;
}

export function restoredNotice(contractId: number): string {
  return `${contractRef(contractId)} restored`;
}

/** Only the newest run starts expanded. */
export function defaultExpanded(runs: readonly HistoryRun[], runId: number): boolean {
  return runs[0]?.runId === runId;
}
