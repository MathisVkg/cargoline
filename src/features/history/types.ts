import type { ContractDetail } from "../contracts/types.ts";

/** Lifetime statistics: completed contracts only (abandoned ones never count). */
export interface HistoryStats {
  completedScuMilli: number;
  completedContracts: number;
  /** Sum of completed rewards: contract values, not verified payouts. */
  estimatedAuec: number;
}

export interface HistoryRun {
  runId: number;
  runNumber: number;
  /** Local date `YYYY-MM-DD`. */
  runDate: string;
  completed: number;
  abandoned: number;
  completedScuMilli: number;
  earnedAuec: number;
  /** Union of the [accepted, finished] intervals of the run's finished contracts. */
  activeDurationMs: number;
  /** Completed rewards ÷ active hours, rounded to 100; null under 10 min or with nothing completed. */
  auecPerHour: number | null;
  /** Finished contracts of the run, newest finish first. */
  contracts: ContractDetail[];
}

/** `get_history`. */
export interface History {
  stats: HistoryStats;
  /** Newest run first; only runs with at least one finished contract. */
  runs: HistoryRun[];
}

/** `clear_history`. */
export interface ClearReport {
  completed: number;
  abandoned: number;
  runs: number;
  captures: number;
  fileErrors: string[];
}
