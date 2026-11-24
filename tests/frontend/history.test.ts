// Frontend unit tests for the History page (Task 12): headings, totals, row formatting, status
// labels, Restore / Clear texts, the empty state and the shared live refresh.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  EMPTY_STATS,
  clearNotice,
  clearText,
  defaultExpanded,
  finishedCounts,
  formatFinished,
  formatRunDate,
  hasHistory,
  restoredNotice,
  restoreQuestion,
  runHeading,
  runLabel,
  runTotals,
  statusLabel,
} from "../../src/features/history/model.ts";
import type { History, HistoryRun } from "../../src/features/history/types.ts";
import type { ContractDetail } from "../../src/features/contracts/types.ts";
import { initialFlow, reduceFlow, type FlowAction } from "../../src/features/gamelog/flow.ts";

const run = (runId: number, runNumber: number, extra: Partial<HistoryRun> = {}): HistoryRun => ({
  runId,
  runNumber,
  runDate: "2026-11-20",
  completed: 4,
  abandoned: 1,
  completedScuMilli: 312_000,
  earnedAuec: 248_500,
  contracts: [],
  ...extra,
});

test("run heading and totals", () => {
  assert.equal(runLabel(3), "#003");
  assert.equal(runLabel(1234), "#1234");
  assert.equal(formatRunDate("2026-11-20"), "20 Nov 2026");
  assert.equal(formatRunDate("2027-01-05"), "5 Jan 2027");
  assert.equal(runHeading(run(1, 3)), "Run #003 · 20 Nov 2026");
  assert.equal(runTotals(run(1, 3)), "4 completed · 1 abandoned · 312 SCU · 248,500 aUEC");
  assert.equal(runTotals(run(1, 3, { abandoned: 0, completedScuMilli: 12_500 })), "4 completed · 12.5 SCU · 248,500 aUEC");
  assert.equal(runTotals(run(1, 3, { completed: 0, completedScuMilli: 0, earnedAuec: 0 })), "0 completed · 1 abandoned · 0 SCU · 0 aUEC");
});

test("row date, status labels", () => {
  assert.equal(formatFinished(new Date(2026, 10, 20, 14, 32).getTime()), "20 Nov 14:32");
  assert.equal(formatFinished(new Date(2026, 0, 3, 9, 5).getTime()), "3 Jan 09:05");
  assert.equal(formatFinished(null), "—");
  assert.equal(statusLabel("completed"), "Completed");
  assert.equal(statusLabel("abandoned"), "Abandoned");
});

test("only the newest run starts expanded", () => {
  const runs = [run(9, 3), run(4, 2), run(1, 1)];
  assert.deepEqual(runs.map((r) => defaultExpanded(runs, r.runId)), [true, false, false]);
  assert.equal(defaultExpanded([], 1), false);
});

test("empty state: zeros, nothing to clear", () => {
  const empty: History = { stats: EMPTY_STATS, runs: [] };
  assert.deepEqual(EMPTY_STATS, { completedScuMilli: 0, completedContracts: 0, estimatedAuec: 0 });
  assert.equal(hasHistory(empty), false);
  assert.equal(hasHistory(null), false, "loading: Clear History disabled");
  assert.equal(hasHistory({ stats: EMPTY_STATS, runs: [run(1, 1)] }), true);
});

test("Clear History texts", () => {
  const h: History = { stats: EMPTY_STATS, runs: [run(2, 2, { completed: 3, abandoned: 2 }), run(1, 1, { completed: 1, abandoned: 0 })] };
  const counts = finishedCounts(h);
  assert.deepEqual(counts, { completed: 4, abandoned: 2 });
  assert.equal(
    clearText(counts.completed, counts.abandoned),
    "Erase all history: 4 completed contracts and 2 abandoned contracts, their screenshots, and the lifetime statistics. Active contracts and settings are not affected. This can't be undone.",
  );
  assert.match(clearText(1, 0), /1 completed contract and 0 abandoned contracts/);
  assert.equal(clearNotice({ completed: 4, abandoned: 2, runs: 2, captures: 1, fileErrors: [] }), "History cleared: 6 contracts, 2 runs, 1 screenshot removed.");
  assert.equal(clearNotice({ completed: 1, abandoned: 0, runs: 1, captures: 0, fileErrors: [] }), "History cleared: 1 contract, 1 run, 0 screenshots removed.");
});

test("Restore texts", () => {
  const c = { id: 6, run: { id: 1, runNumber: 3, runDate: "2026-11-20" } } as ContractDetail;
  assert.equal(restoreQuestion(c), "Restore C06 to active? It goes back to the Contracts page in its original run (Run #003).");
  assert.equal(restoredNotice(6), "C06 restored");
});

test("live refresh: a finalize, a restore or a clear bumps the shared list version", () => {
  const changed: FlowAction = { type: "contractsChanged" };
  const s = [changed, changed, changed].reduce(reduceFlow, initialFlow);
  assert.equal(s.listVersion, 3, "History and Contracts both reload on every change");
  assert.equal(s.modal, initialFlow.modal, "nothing else changes");
  // Completions from the game bump it too (Task 11).
  const auto = reduceFlow(
    reduceFlow(initialFlow, {
      type: "gameLog",
      event: { kind: "ended", missionId: "m", state: "MISSION_STATE_COMPLETED", at: null, contractId: 1, completeFromGame: "automatic" },
    }),
    { type: "completionDue" },
  );
  const done = reduceFlow(auto, { type: "autoCompleted", batch: auto.completion.running!.batch, completed: [1], failed: [], now: 1 });
  assert.equal(done.listVersion, 1);
});
