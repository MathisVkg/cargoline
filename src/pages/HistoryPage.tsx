import { useCallback, useEffect, useId, useMemo, useState, type Dispatch, type ReactNode } from "react";
import { AlertTriangle, CalendarDays, CircleCheck, Info, LoaderCircle, MoreHorizontal, RotateCcw, RotateCw, Trash2, X } from "lucide-react";
import { EmptyState, PageContainer, PageHeader, Panel } from "../components/ui";
import { ConfirmDialog, type DialogOutcome } from "../components/ConfirmDialog";
import { ActionMenu } from "../features/contracts/ActionMenu";
import { describeError, toCommandError } from "../features/contracts/api";
import { BoxText, GroupHeader, OPERATION, StopQuantityText, plural } from "../features/contracts/display";
import { boxInfo, contractRef, groupByContract, type ObjectiveRow, type StopEntry } from "../features/contracts/grouping";
import { formatAuec, formatScu } from "../features/contracts/scu";
import type { CommandError, ContractDetail } from "../features/contracts/types";
import type { FlowAction, FlowState } from "../features/gamelog/flow";
import { clearHistory, getHistory, restoreContracts } from "../features/history/api";
import {
  EMPTY_STATS,
  clearNotice,
  clearText,
  defaultExpanded,
  finishedCounts,
  formatFinished,
  hasHistory,
  restoredNotice,
  restoreQuestion,
  runHeading,
  runTotals,
  statusLabel,
} from "../features/history/model";
import type { History, HistoryRun } from "../features/history/types";

type LoadState = { status: "loading" } | { status: "error"; error: CommandError } | { status: "ready"; history: History };

interface Notice {
  id: number;
  text: string;
  tone: "success" | "info";
}

const NOTICE_MS = 6000;

/**
 * History (Task 12): lifetime statistics (completed contracts only), finished contracts grouped
 * by their run, Restore to active, and Clear History. Reloads whenever contracts change
 * elsewhere (`flow.listVersion`).
 */
export function HistoryPage({ flow, dispatch }: { flow: FlowState; dispatch: Dispatch<FlowAction> }) {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [runOpen, setRunOpen] = useState<Map<number, boolean>>(new Map());
  const [rowOpen, setRowOpen] = useState<Set<number>>(new Set());
  const [restoring, setRestoring] = useState<ContractDetail | null>(null);
  const [clearing, setClearing] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);

  const refresh = useCallback(async () => {
    try {
      setState({ status: "ready", history: await getHistory() });
    } catch (error) {
      setState({ status: "error", error: toCommandError(error) });
    }
  }, []);

  // First load, and again when contracts are finalised / restored / completed from the game.
  useEffect(() => {
    void refresh();
  }, [refresh, flow.listVersion]);

  // The Ask dialog (complete from game) waits while a confirmation is open here.
  const dialogOpen = restoring !== null || clearing;
  useEffect(() => {
    dispatch({ type: "blocking", open: dialogOpen });
    return () => dispatch({ type: "blocking", open: false });
  }, [dialogOpen]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice((n) => (n?.id === notice.id ? null : n)), NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);
  const showNotice = (text: string, tone: Notice["tone"]) => setNotice({ id: Date.now(), text, tone });

  const history = state.status === "ready" ? state.history : null;
  const runs = history?.runs ?? [];
  const stats = history?.stats ?? EMPTY_STATS;
  const counts = useMemo(() => finishedCounts(history), [history]);

  const isRunOpen = (run: HistoryRun) => runOpen.get(run.runId) ?? defaultExpanded(runs, run.runId);
  const toggleRun = (run: HistoryRun) => setRunOpen((m) => new Map(m).set(run.runId, !isRunOpen(run)));
  const toggleRow = (id: number) =>
    setRowOpen((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const runRestore = async (contract: ContractDetail): Promise<DialogOutcome> => {
    try {
      await restoreContracts([contract.id]);
      setRestoring(null);
      showNotice(restoredNotice(contract.id), "success");
      dispatch({ type: "contractsChanged" }); // History, Contracts and the summary reload
      return { ok: true };
    } catch (thrown) {
      const error = toCommandError(thrown);
      if (error.kind === "notFound" || error.kind === "notFinished") {
        // The list on screen is out of date: explain and reload.
        setRestoring(null);
        showNotice(error.kind === "notFound" ? "This contract no longer exists." : `${contractRef(contract.id)} is already active.`, "info");
        void refresh();
        return { ok: true };
      }
      return { ok: false, message: describeError(error) };
    }
  };

  const runClear = async (): Promise<DialogOutcome> => {
    try {
      const report = await clearHistory();
      setClearing(false);
      setRowOpen(new Set());
      setRunOpen(new Map());
      showNotice(clearNotice(report), "success");
      dispatch({ type: "contractsChanged" });
      return { ok: true };
    } catch (thrown) {
      return { ok: false, message: `Nothing was erased. ${describeError(toCommandError(thrown))}` };
    }
  };

  return (
    <PageContainer>
      <PageHeader
        index="03"
        title="History"
        description="Contracts grouped into numbered runs by local acceptance date."
        actions={
          <button
            type="button"
            onClick={() => setClearing(true)}
            disabled={!hasHistory(history)}
            title={hasHistory(history) ? undefined : "No history to clear"}
            className="inline-flex h-9 items-center gap-2 rounded-sm border border-danger/50 px-3 text-sm font-medium text-danger transition-colors duration-150 hover:bg-danger-soft disabled:cursor-not-allowed disabled:border-line disabled:text-dim disabled:hover:bg-transparent"
          >
            <Trash2 className="size-4 shrink-0" aria-hidden="true" />
            <span className="whitespace-nowrap">Clear History</span>
          </button>
        }
      />

      <section
        aria-label="Lifetime statistics"
        className="mb-4 grid grid-cols-2 border-y border-line min-[990px]:grid-cols-3 min-[990px]:divide-x min-[990px]:divide-line"
      >
        <Stat label="Completed SCU" loading={state.status === "loading"}>
          {formatScu(stats.completedScuMilli)}
          <span className="ml-1.5 text-sm text-dim">SCU</span>
        </Stat>
        <Stat label="Completed contracts" loading={state.status === "loading"} className="border-l border-line min-[990px]:border-l-0">
          {stats.completedContracts.toLocaleString("en-US")}
        </Stat>
        <Stat
          label="Estimated aUEC earned"
          loading={state.status === "loading"}
          className="col-span-2 border-t border-line min-[990px]:col-span-1 min-[990px]:border-t-0"
          note="Estimate: contract rewards, not verified payouts."
        >
          {formatAuec(stats.estimatedAuec)}
          <span className="ml-1.5 text-sm text-dim">aUEC</span>
        </Stat>
      </section>

      {notice && (
        <div
          role="status"
          className={[
            "mb-3 flex items-center gap-2 border-l-2 bg-surface px-3 py-1.5 text-sm",
            notice.tone === "success" ? "border-pickup" : "border-line-strong",
          ].join(" ")}
        >
          {notice.tone === "success" ? (
            <CircleCheck className="size-4 shrink-0 text-pickup" aria-hidden="true" />
          ) : (
            <Info className="size-4 shrink-0 text-muted" aria-hidden="true" />
          )}
          <span className="min-w-0 flex-1 break-words text-fg">{notice.text}</span>
          <button
            type="button"
            onClick={() => setNotice(null)}
            aria-label="Dismiss notice"
            className="grid size-6 shrink-0 place-items-center rounded-sm text-dim hover:bg-raised hover:text-fg"
          >
            <X className="size-3.5" aria-hidden="true" />
          </button>
        </div>
      )}

      {state.status === "loading" && (
        <Panel>
          <div className="flex items-center justify-center gap-2 px-6 py-14 text-sm text-muted" role="status">
            <LoaderCircle className="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            Loading history…
          </div>
        </Panel>
      )}

      {state.status === "error" && (
        <Panel>
          <div role="alert" className="flex flex-col items-center gap-3 px-6 py-10 text-center">
            <AlertTriangle className="size-5 text-danger" aria-hidden="true" />
            <p className="max-w-lg text-sm break-words text-muted">{describeError(state.error)}</p>
            <button
              type="button"
              onClick={() => {
                setState({ status: "loading" });
                void refresh();
              }}
              className="inline-flex h-8 items-center gap-1.5 rounded-sm border border-line px-3 text-sm text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg"
            >
              <RotateCw className="size-3.5" aria-hidden="true" />
              Retry
            </button>
          </div>
        </Panel>
      )}

      {state.status === "ready" &&
        (runs.length === 0 ? (
          <Panel>
            <EmptyState icon={CalendarDays} title="No runs recorded yet">
              A run is created automatically on the first contract of each local date. Completed and abandoned contracts
              will be listed here.
            </EmptyState>
          </Panel>
        ) : (
          <div className="@container flex flex-col gap-1" role="list" aria-label="Runs">
            {runs.map((run) => (
              <RunSection
                key={run.runId}
                run={run}
                expanded={isRunOpen(run)}
                onToggle={() => toggleRun(run)}
                isRowOpen={(id) => rowOpen.has(id)}
                onToggleRow={toggleRow}
                onRestore={setRestoring}
              />
            ))}
          </div>
        ))}

      {restoring && (
        <ConfirmDialog
          id="restore-contract"
          heading="Restore to active"
          confirmLabel="Restore"
          onCancel={() => setRestoring(null)}
          onConfirm={() => runRestore(restoring)}
        >
          <p>{restoreQuestion(restoring)}</p>
          <p className="mt-1 text-xs text-dim">{restoring.title}</p>
        </ConfirmDialog>
      )}
      {clearing && (
        <ConfirmDialog
          id="clear-history"
          heading="Clear History"
          confirmLabel="Erase history"
          danger
          onCancel={() => setClearing(false)}
          onConfirm={runClear}
        >
          {clearText(counts.completed, counts.abandoned)}
        </ConfirmDialog>
      )}
    </PageContainer>
  );
}

function Stat({
  label,
  loading,
  note,
  className = "",
  children,
}: {
  label: string;
  loading: boolean;
  note?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={["min-w-0 px-4 py-3", className].join(" ")}>
      <p className="label-tech">{label}</p>
      <p className="mt-0.5 font-mono text-2xl leading-tight text-fg tabular-nums">
        {loading ? (
          <span className="text-dim">
            <span aria-hidden="true">—</span>
            <span className="sr-only">Loading</span>
          </span>
        ) : (
          children
        )}
      </p>
      {note && <p className="mt-0.5 text-[11px] text-dim">{note}</p>}
    </div>
  );
}

function RunSection({
  run,
  expanded,
  onToggle,
  isRowOpen,
  onToggleRow,
  onRestore,
}: {
  run: HistoryRun;
  expanded: boolean;
  onToggle: () => void;
  isRowOpen: (id: number) => boolean;
  onToggleRow: (id: number) => void;
  onRestore: (contract: ContractDetail) => void;
}) {
  const bodyId = useId();
  return (
    <section role="listitem" className="min-w-0 border-b border-line">
      <h3 className="m-0 min-w-0">
        <GroupHeader
          expanded={expanded}
          controls={bodyId}
          onToggle={onToggle}
          title={runHeading(run)}
          aside={<span className="font-mono text-xs text-muted">{runTotals(run)}</span>}
        />
      </h3>
      <ol id={bodyId} hidden={!expanded} aria-label={`Finished contracts of ${runHeading(run)}`} className="pb-1.5 pl-6">
        {run.contracts.map((contract) => (
          <ContractRow
            key={contract.id}
            contract={contract}
            expanded={isRowOpen(contract.id)}
            onToggle={() => onToggleRow(contract.id)}
            onRestore={() => onRestore(contract)}
          />
        ))}
      </ol>
    </section>
  );
}

function ContractRow({
  contract,
  expanded,
  onToggle,
  onRestore,
}: {
  contract: ContractDetail;
  expanded: boolean;
  onToggle: () => void;
  onRestore: () => void;
}) {
  const bodyId = useId();
  const ref = contractRef(contract.id);
  const abandoned = contract.status === "abandoned";
  const objectives = useMemo(() => (expanded ? (groupByContract([contract])[0]?.objectives ?? []) : []), [expanded, contract]);
  return (
    <li className="min-w-0 border-t border-line/70 first:border-t-0">
      <div className="flex min-w-0 items-center">
        <div className="min-w-0 flex-1">
          <GroupHeader
            expanded={expanded}
            controls={bodyId}
            onToggle={onToggle}
            className="py-1"
            leading={
              <span className="shrink-0 rounded-sm border border-line px-1.5 font-mono text-[11px] leading-5 text-muted">{ref}</span>
            }
            title={contract.title}
            meta={`Finished ${formatFinished(contract.finishedAt)}`}
            aside={
              <>
                <span className="font-mono text-sm whitespace-nowrap">
                  <span className="text-fg">{formatScu(contract.totalScuMilli)}</span> <span className="text-dim">SCU</span>
                </span>
                <span className="w-28 text-right font-mono text-sm whitespace-nowrap">
                  <span className={abandoned ? "text-dim line-through decoration-dim/60" : "text-fg"}>
                    {formatAuec(contract.rewardAuec)}
                  </span>{" "}
                  <span className="text-dim">aUEC</span>
                </span>
                <StatusBadge status={contract.status} />
              </>
            }
          />
        </div>
        <ActionMenu
          label={`Actions for ${ref} · ${contract.title}`}
          items={[{ id: "restore", label: "Restore to active", icon: RotateCcw, onSelect: onRestore }]}
          triggerClassName="grid size-8 shrink-0 place-items-center rounded-sm text-dim transition-colors duration-150 hover:bg-raised hover:text-fg aria-expanded:bg-raised aria-expanded:text-fg"
        >
          <MoreHorizontal className="size-4" aria-hidden="true" />
        </ActionMenu>
      </div>
      <div id={bodyId} hidden={!expanded} className="pb-1.5 pl-8">
        {expanded && (
          <>
            <p className="label-tech px-2 pb-1">
              Accepted {formatFinished(contract.acceptedAt)} · {plural(objectives.length, "objective")}
            </p>
            <ol className="divide-y divide-line/70 border-t border-line/70">
              {objectives.map((objective) => (
                <ObjectiveLine key={objective.key} objective={objective} />
              ))}
            </ol>
          </>
        )}
      </div>
    </li>
  );
}

function StatusBadge({ status }: { status: ContractDetail["status"] }) {
  const completed = status === "completed";
  return (
    <span
      className={[
        "inline-flex w-[5.5rem] items-center justify-center rounded-sm border px-1.5 text-[11px] leading-5 font-medium",
        completed ? "border-pickup/40 text-pickup" : "border-danger/30 text-danger/80",
      ].join(" ")}
    >
      {statusLabel(status)}
    </span>
  );
}

// Same layout as By Contract, without tick controls.
const LINE =
  "grid gap-x-3 gap-y-1 grid-cols-[4.25rem_minmax(0,1fr)] " +
  "@min-[720px]:grid-cols-[4.25rem_minmax(0,0.8fr)_minmax(0,1.2fr)_minmax(0,1.2fr)]";

function ObjectiveLine({ objective }: { objective: ObjectiveRow }) {
  const box = boxInfo(objective.boxGroups, objective.maxBoxSizeScuMilli);
  return (
    <li className={`${LINE} px-2 py-1.5 text-sm`}>
      <span className="text-right font-mono whitespace-nowrap text-fg">
        {formatScu(objective.totalScuMilli)} <span className="text-xs text-dim">SCU</span>
      </span>
      <span className="min-w-0">
        <span className="block break-words text-fg">{objective.commodityName}</span>
        {box.kind !== "none" && (
          <span className="block font-mono text-[11px] text-dim">
            <BoxText boxGroups={objective.boxGroups} maxBoxSizeScuMilli={objective.maxBoxSizeScuMilli} />
          </span>
        )}
      </span>
      <StopList operation="pickup" stops={objective.pickups} requirementScuMilli={objective.totalScuMilli} />
      <StopList operation="delivery" stops={objective.deliveries} requirementScuMilli={objective.totalScuMilli} />
    </li>
  );
}

function StopList({
  operation,
  stops,
  requirementScuMilli,
}: {
  operation: "pickup" | "delivery";
  stops: StopEntry[];
  requirementScuMilli: number;
}) {
  const meta = OPERATION[operation];
  const Icon = meta.icon;
  return (
    <div className="col-start-2 min-w-0 @min-[720px]:col-start-auto">
      <ul className="flex flex-col">
        {stops.map((stop, i) => (
          <li key={stop.stopId} className="flex min-w-0 items-center gap-2">
            <Icon className={["size-3 shrink-0", meta.tone, i === 0 ? "" : "invisible"].join(" ")} aria-hidden="true" />
            <span className="sr-only">{meta.label}: </span>
            <span className="min-w-0 flex-1 break-words text-muted">{stop.locationName}</span>
            {stop.quantity.kind !== "derived" && (
              <span className="shrink-0 font-mono text-xs">
                <StopQuantityText quantity={stop.quantity} requirementScuMilli={requirementScuMilli} />
              </span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
