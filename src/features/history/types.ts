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
