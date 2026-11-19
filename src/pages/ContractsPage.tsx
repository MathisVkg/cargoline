import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  CircleCheck,
  CircleX,
  ChevronsDownUp,
  ChevronsUpDown,
  ClipboardList,
  Info,
  LoaderCircle,
  MoreHorizontal,
  Plus,
  RotateCw,
  X,
} from "lucide-react";
import { EmptyState, PageContainer, PageHeader, Panel } from "../components/ui";
import { ActionMenu } from "../features/contracts/ActionMenu";
import { AddContractModal } from "../features/contracts/AddContractModal";
import { ConfirmFinalizeDialog, type ConfirmOutcome } from "../features/contracts/ConfirmFinalizeDialog";
import { ContractsSummary } from "../features/contracts/ContractsSummary";
import { ContractView } from "../features/contracts/ContractView";
import { LocationView } from "../features/contracts/LocationView";
import {
  abandonContract,
  completeContract,
  describeError,
  finalizeActiveContracts,
  getContractGrouping,
  listActiveContracts,
  setContractGrouping,
  setStopReminder,
  toCommandError,
} from "../features/contracts/api";
import {
  bulkConfirmation,
  pruneExpansion,
  singleConfirmation,
  statusFor,
  successNotice,
  validExpansionKeys,
  type Confirmation,
  type FinalAction,
} from "../features/contracts/finalize";
import { applyReminders, captureReminders, restoreReminders } from "../features/contracts/reminders";
import { contractRef } from "../features/contracts/grouping";
import {
  groupByContract,
  groupByLocation,
  isCombinedKey,
  isExpanded,
  setAllExpanded,
  toggleExpanded,
  type ExpansionState,
} from "../features/contracts/grouping";
import { summaryForList } from "../features/contracts/summary";
import type { CommandError, ContractDetail, ContractGrouping } from "../features/contracts/types";

const MODES: { id: ContractGrouping; label: string }[] = [
  { id: "by_location", label: "By Location" },
  { id: "by_contract", label: "By Contract" },
];

/** Location groups start open (operational view); contract groups start closed (review view). */
const DEFAULT_EXPANDED: Record<ContractGrouping, boolean> = { by_location: true, by_contract: false };

/** Selected ship capacity (whole SCU). No ship catalog yet, so always unknown. */
const SELECTED_SHIP_CAPACITY_SCU: number | null = null;

/** How long the success notice stays visible. */
const NOTICE_MS = 6000;

interface Notice {
  id: number;
  text: string;
  tone: "success" | "info";
}

type ListState =
  | { status: "loading" }
  | { status: "error"; error: CommandError }
  | { status: "ready"; contracts: ContractDetail[] };

export function ContractsPage() {
  const [list, setList] = useState<ListState>({ status: "loading" });
  const [mode, setMode] = useState<ContractGrouping>("by_location");
  const [modeLoaded, setModeLoaded] = useState(false);
  const [modeSaveFailed, setModeSaveFailed] = useState(false);
  const [expansion, setExpansion] = useState<Record<ContractGrouping, ExpansionState>>({
    by_location: new Map(),
    by_contract: new Map(),
  });
  const [adding, setAdding] = useState(false);
  const [pending, setPending] = useState<Confirmation | null>(null);
  const [editing, setEditing] = useState<ContractDetail | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);

  const refresh = useCallback(async () => {
    try {
      setList({ status: "ready", contracts: await listActiveContracts() });
    } catch (error) {
      setList({ status: "error", error: toCommandError(error) });
    }
  }, []);

  useEffect(() => {
    void refresh();
    let cancelled = false;
    getContractGrouping()
      .then((saved) => !cancelled && setMode(saved))
      .catch(() => undefined) // keep the default; the list shows any database problem
      .finally(() => !cancelled && setModeLoaded(true));
    return () => {
      cancelled = true;
    };
  }, [refresh]);

  const chooseMode = (next: ContractGrouping) => {
    if (next === mode) return;
    setMode(next);
    setModeSaveFailed(false);
    setContractGrouping(next).catch(() => setModeSaveFailed(true));
  };

  const contracts = list.status === "ready" ? list.contracts : [];
  const locationGroups = useMemo(() => groupByLocation(contracts), [contracts]);
  const contractGroups = useMemo(() => groupByContract(contracts), [contracts]);
  const groupKeys = mode === "by_location" ? locationGroups.map((g) => g.key) : contractGroups.map((g) => g.key);

  // Combined manifest lines start collapsed; groups use the mode default.
  const defaultFor = (key: string) => (isCombinedKey(key) ? false : DEFAULT_EXPANDED[mode]);
  const expandedFor = (key: string) => isExpanded(expansion[mode], key, defaultFor(key));
  const toggle = (key: string) =>
    setExpansion((current) => ({ ...current, [mode]: toggleExpanded(current[mode], key, defaultFor(key)) }));
  const setAll = (expanded: boolean) => setExpansion((current) => ({ ...current, [mode]: setAllExpanded(groupKeys, expanded) }));

  // Drop expansion choices for groups / lines that no longer exist (e.g. finished contracts).
  useEffect(() => {
    if (list.status !== "ready") return;
    const valid = validExpansionKeys(locationGroups, contractGroups);
    setExpansion((current) => {
      const byLocation = pruneExpansion(current.by_location, valid);
      const byContract = pruneExpansion(current.by_contract, valid);
      return byLocation === current.by_location && byContract === current.by_contract
        ? current
        : { by_location: byLocation, by_contract: byContract };
    });
  }, [list.status, locationGroups, contractGroups]);

  // The notice disappears on its own.
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice((n) => (n?.id === notice.id ? null : n)), NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const showNotice = (text: string, tone: Notice["tone"]) => setNotice({ id: Date.now(), text, tone });

  const requestFinalize = (contractId: number, action: FinalAction) => {
    const contract = contracts.find((c) => c.id === contractId);
    if (contract) setPending(singleConfirmation(contract, action));
  };

  const requestBulk = (action: FinalAction) => {
    // The dialog shows, and later sends, exactly the contracts active right now.
    if (contracts.length > 0) setPending(bulkConfirmation(contracts, action));
  };

  const runFinalize = async (confirmation: Confirmation): Promise<ConfirmOutcome> => {
    try {
      if (confirmation.count === 1 && confirmation.title !== null) {
        const id = confirmation.ids[0];
        await (confirmation.action === "complete" ? completeContract(id) : abandonContract(id));
      } else {
        await finalizeActiveContracts(confirmation.ids, statusFor(confirmation.action));
      }
      setPending(null);
      showNotice(successNotice(confirmation), "success");
      void refresh();
      return { ok: true };
    } catch (thrown) {
      const error = toCommandError(thrown);
      const message = describeError(error);
      if (error.kind === "conflict" || error.kind === "notFound" || error.kind === "notActive") {
        // The list on screen is out of date: explain, reload and close.
        setPending(null);
        showNotice(message, "info");
        void refresh();
        return { ok: false, message, close: true };
      }
      return { ok: false, message, close: false };
    }
  };

  const requestEdit = (contractId: number) => {
    const contract = contracts.find((c) => c.id === contractId);
    if (contract) setEditing(contract);
  };

  const handleUpdated = (updated: ContractDetail) => {
    setEditing(null);
    setList((current) =>
      current.status === "ready"
        ? { status: "ready", contracts: current.contracts.map((c) => (c.id === updated.id ? updated : c)) }
        : current,
    );
    showNotice(`${contractRef(updated.id)} updated`, "success");
    void refresh();
  };

  const handleStaleEdit = (message: string) => {
    setEditing(null);
    showNotice(message, "info");
    void refresh();
  };

  /** Optimistic Pick Up / Turn In tick: shown at once, rolled back if saving fails. */
  const toggleReminder = async (stopIds: number[], done: boolean) => {
    if (list.status !== "ready" || stopIds.length === 0) return;
    const snapshot = captureReminders(list.contracts, stopIds);
    const update = (change: (contracts: ContractDetail[]) => ContractDetail[]) =>
      setList((current) => (current.status === "ready" ? { status: "ready", contracts: change(current.contracts) } : current));
    update((cs) => applyReminders(cs, stopIds, done ? Date.now() : null));
    try {
      const saved = await setStopReminder(stopIds, done);
      update((cs) => restoreReminders(cs, new Map(saved.map((s) => [s.id, s.reminderDoneAt]))));
    } catch (thrown) {
      const error = toCommandError(thrown);
      if (error.kind === "notFound" || error.kind === "notActive") {
        showNotice(`${describeError(error)} The list has been reloaded.`, "info");
        void refresh();
      } else {
        update((cs) => restoreReminders(cs, snapshot));
        showNotice(`The tick could not be saved and was undone. ${describeError(error)}`, "info");
      }
    }
  };

  const handleSaved = (saved: ContractDetail) => {
    setAdding(false);
    // Show the new contract immediately, then reload the authoritative list.
    setList((current) =>
      current.status === "ready" ? { status: "ready", contracts: [saved, ...current.contracts] } : current,
    );
    void refresh();
  };

  const count = contracts.length;
  const ready = list.status === "ready" && modeLoaded;
  // Recomputed on every list (re)load; null while loading or on error.
  const summary = useMemo(() => summaryForList(list), [list]);

  return (
    <PageContainer>
      <PageHeader
        index="01"
        title="Contracts"
        description="Active hauling contracts, grouped by location or by contract."
        actions={
          <button
            type="button"
            onClick={() => setAdding(true)}
            className="inline-flex h-9 items-center gap-2 rounded-sm bg-accent px-3 text-sm font-semibold text-(color:--color-canvas) transition-[filter] duration-150 hover:brightness-110"
          >
            <Plus className="size-4 shrink-0" aria-hidden="true" />
            <span className="whitespace-nowrap">Add Contract</span>
          </button>
        }
      />

      <ContractsSummary summary={summary} capacityScu={SELECTED_SHIP_CAPACITY_SCU} />

      <div className="mb-3 flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <div className="flex flex-wrap items-center gap-2">
          <div role="group" aria-label="Group contracts" className="inline-flex rounded-sm border border-line bg-surface p-0.5">
            {MODES.map(({ id, label }) => {
              const active = id === mode;
              return (
                <button
                  key={id}
                  type="button"
                  aria-pressed={active}
                  onClick={() => chooseMode(id)}
                  className={[
                    "h-8 rounded-[2px] px-3 text-sm whitespace-nowrap transition-colors duration-150 focus-visible:outline-offset-0",
                    active ? "bg-raised font-medium text-fg shadow-[inset_0_-2px_0_var(--color-accent)]" : "text-dim hover:text-muted",
                  ].join(" ")}
                >
                  {label}
                </button>
              );
            })}
          </div>
          <div className="flex items-center">
            {ready && count > 0 && (
              <>
                <ToolbarButton icon={ChevronsUpDown} label="Expand all" onClick={() => setAll(true)} />
                <ToolbarButton icon={ChevronsDownUp} label="Collapse all" onClick={() => setAll(false)} />
              </>
            )}
            <ActionMenu
              label="Actions for all active contracts"
              disabled={!ready || count === 0}
              items={[
                {
                  id: "complete-all",
                  label: `Complete all active (${count})`,
                  icon: CircleCheck,
                  onSelect: () => requestBulk("complete"),
                },
                {
                  id: "abandon-all",
                  label: `Abandon all active (${count})`,
                  icon: CircleX,
                  danger: true,
                  onSelect: () => requestBulk("abandon"),
                },
              ]}
              triggerClassName="grid size-8 place-items-center rounded-sm text-dim transition-colors duration-150 hover:bg-raised hover:text-fg disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent aria-expanded:bg-raised aria-expanded:text-fg"
            >
              <MoreHorizontal className="size-4" aria-hidden="true" />
            </ActionMenu>
          </div>
        </div>
        <div className="flex flex-col items-end">
          {ready && mode === "by_location" && count > 0 && (
            <span className="label-tech">
              {locationGroups.length} location{locationGroups.length === 1 ? "" : "s"}
            </span>
          )}
          {modeSaveFailed && (
            <span role="status" className="text-xs text-danger">
              The view preference could not be saved.
            </span>
          )}
        </div>
      </div>

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

      {(list.status === "loading" || (list.status === "ready" && !modeLoaded)) && (
        <Panel>
          <div className="flex items-center justify-center gap-2 px-6 py-14 text-sm text-muted" role="status">
            <LoaderCircle className="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            Loading contracts…
          </div>
        </Panel>
      )}

      {list.status === "error" && (
        <Panel>
          <div role="alert" className="flex flex-col items-center gap-3 px-6 py-10 text-center">
            <AlertTriangle className="size-5 text-danger" aria-hidden="true" />
            <p className="max-w-lg text-sm break-words text-muted">{describeError(list.error)}</p>
            <button
              type="button"
              onClick={() => {
                setList({ status: "loading" });
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

      {ready &&
        (count === 0 ? (
          <Panel>
            <EmptyState icon={ClipboardList} title="No active contracts">
              Use Add Contract to enter a contract and its cargo. Pickups and turn-ins will be listed here by location
              or by contract.
            </EmptyState>
          </Panel>
        ) : mode === "by_location" ? (
          <LocationView
            groups={locationGroups}
            isExpanded={expandedFor}
            onToggle={toggle}
            onRequestFinalize={requestFinalize}
            onEditContract={requestEdit}
            onToggleReminder={toggleReminder}
          />
        ) : (
          <ContractView
            groups={contractGroups}
            isExpanded={expandedFor}
            onToggle={toggle}
            onRequestFinalize={requestFinalize}
            onEditContract={requestEdit}
            onToggleReminder={toggleReminder}
          />
        ))}

      {ready && count > 0 && (
        <p className="mt-2 text-xs text-dim">
          “?” marks a location whose share of the cargo is unknown.
        </p>
      )}

      {adding && <AddContractModal onClose={() => setAdding(false)} onSaved={handleSaved} />}
      {editing && (
        <AddContractModal
          key={`edit:${editing.id}`}
          editing={editing}
          onClose={() => setEditing(null)}
          onSaved={handleUpdated}
          onStale={handleStaleEdit}
        />
      )}
      {pending && (
        <ConfirmFinalizeDialog
          key={`${pending.action}:${pending.ids.join(",")}`}
          confirmation={pending}
          onCancel={() => setPending(null)}
          onConfirm={() => runFinalize(pending)}
        />
      )}
    </PageContainer>
  );
}

function ToolbarButton({ icon: Icon, label, onClick }: { icon: typeof Plus; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="grid size-8 place-items-center rounded-sm text-dim transition-colors duration-150 hover:bg-raised hover:text-fg"
    >
      <Icon className="size-4" aria-hidden="true" />
    </button>
  );
}
