import { useEffect, useRef, useState } from "react";
import { AlertTriangle, LoaderCircle } from "lucide-react";
import { describeError, finalizeActiveContracts, listActiveContracts, toCommandError } from "../contracts/api";
import { contractRef } from "../contracts/grouping";
import { formatAuec, formatScu } from "../contracts/scu";
import type { ContractDetail } from "../contracts/types";
import type { Completion } from "./flow";
import { askRows } from "./completion";

/**
 * "Ask me first" (Task 11 Part B): the contracts the game reports as completed, all ticked.
 * Confirm finalises the ticked ones (one transaction); Cancel and unticked ones go back to the
 * normal "completed in game" notices. Completions arriving while it is open are added to it.
 * Escape cancels. It never brings the window forward.
 */
export function CompleteFromGameDialog({
  items,
  onDone,
}: {
  items: readonly Completion[];
  /** `completed`: the contract ids finalised (empty on Cancel). */
  onDone: (completed: number[]) => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const [active, setActive] = useState<ContractDetail[] | null>(null);
  const [unticked, setUnticked] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = () =>
    listActiveContracts()
      .then(setActive)
      .catch((e) => setError(describeError(toCommandError(e))));

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    cancelRef.current?.focus();
    void load();
    return () => dialog?.close();
  }, []);

  // New completions arrive while open: refresh the details.
  useEffect(() => {
    if (active && items.some((i) => !active.some((c) => c.id === i.contractId))) void load();
  }, [items.length]);

  const rows = active ? askRows(items, active) : [];
  const ticked = rows.filter((r) => !unticked.has(r.id)).map((r) => r.id);

  // Every reported contract is no longer active (finished by hand meanwhile): nothing to ask.
  useEffect(() => {
    if (active && rows.length === 0 && items.length > 0) onDone([]);
  }, [active, rows.length]);

  const toggle = (id: number) =>
    setUnticked((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const confirm = async () => {
    if (busy || ticked.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      await finalizeActiveContracts(ticked, "completed");
      onDone(ticked);
    } catch (thrown) {
      const failure = toCommandError(thrown);
      setBusy(false);
      if (failure.kind === "conflict" || failure.kind === "notFound" || failure.kind === "notActive") {
        setError("Some of these contracts are no longer active. The list has been updated; nothing was changed.");
        void load();
      } else setError(describeError(failure));
    }
  };

  const headingId = "complete-from-game-title";
  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={headingId}
      aria-describedby={`${headingId}-note`}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onDone([]);
      }}
      className="m-auto w-[min(30rem,calc(100%-2rem))] max-w-none max-h-[calc(100%-2rem)] flex-col overflow-hidden rounded-sm border border-line-strong bg-surface p-0 text-fg shadow-2xl backdrop:bg-black/70 open:flex"
    >
      <div className="min-h-0 flex-1 overflow-y-auto px-4 pt-3.5 pb-3">
        <h2 id={headingId} className="text-[15px] font-semibold tracking-tight text-fg">
          {rows.length === 1 ? "Contract completed in game" : "Contracts completed in game"}
        </h2>
        <p id={`${headingId}-note`} className="mt-0.5 text-sm break-words text-muted">
          The game reports these contracts as completed. Untick any you want to keep active.
        </p>

        {active === null && !error && (
          <p role="status" className="mt-3 flex items-center gap-2 text-sm text-muted">
            <LoaderCircle className="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            Loading…
          </p>
        )}

        {rows.length > 0 && (
          <ul aria-label="Contracts to mark complete" className="mt-3 flex flex-col divide-y divide-line border-y border-line">
            {rows.map((r) => {
              const checked = !unticked.has(r.id);
              return (
                <li key={r.id}>
                  <label className="flex cursor-pointer items-center gap-3 py-2">
                    <input
                      type="checkbox"
                      checked={checked}
                      disabled={busy}
                      onChange={() => toggle(r.id)}
                      className="size-4 shrink-0 accent-(--color-accent)"
                    />
                    <span className="w-9 shrink-0 font-mono text-xs text-accent">{contractRef(r.id)}</span>
                    <span className="min-w-0 flex-1 truncate text-sm text-fg" title={r.title}>
                      {r.title}
                    </span>
                    <span className="shrink-0 text-right font-mono text-xs text-muted tabular-nums">
                      {formatScu(r.totalScuMilli)} SCU
                      <span className="block text-dim">{formatAuec(r.rewardAuec)} aUEC</span>
                    </span>
                  </label>
                </li>
              );
            })}
          </ul>
        )}

        {error && (
          <div role="alert" className="mt-3 flex items-start gap-2 rounded-sm border border-danger/40 bg-danger-soft px-3 py-2 text-sm">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-danger" aria-hidden="true" />
            <span className="min-w-0 break-words">{error}</span>
          </div>
        )}
      </div>

      <div className="flex shrink-0 justify-end gap-2 border-t border-line px-4 py-2.5">
        <button
          ref={cancelRef}
          type="button"
          onClick={() => onDone([])}
          disabled={busy}
          className="h-9 rounded-sm border border-line px-4 text-sm font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg disabled:opacity-50"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={confirm}
          disabled={busy || ticked.length === 0}
          className="h-9 rounded-sm bg-accent px-4 text-sm font-semibold text-(color:--color-canvas) transition-[filter] duration-150 hover:brightness-110 disabled:opacity-50"
        >
          {busy ? "Working…" : `Mark complete (${ticked.length})`}
        </button>
      </div>
    </dialog>
  );
}
