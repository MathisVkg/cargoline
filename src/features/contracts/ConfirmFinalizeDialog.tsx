import { useEffect, useRef, useState } from "react";
import { AlertTriangle } from "lucide-react";
import type { Confirmation } from "./finalize";
import { formatAuec, formatScu } from "./scu";

/** What the page reports back after trying the request. */
export type ConfirmOutcome =
  | { ok: true }
  /** `close`: the page reloads and closes the dialog (e.g. a conflict). */
  | { ok: false; message: string; close: boolean };

interface ConfirmFinalizeDialogProps {
  confirmation: Confirmation;
  onCancel: () => void;
  onConfirm: () => Promise<ConfirmOutcome>;
}

/**
 * Compact confirmation for completing / abandoning one or several contracts.
 * Mount to open, unmount to close. Focus starts on Cancel; Escape cancels;
 * both buttons are disabled while the request runs.
 */
export function ConfirmFinalizeDialog({ confirmation: c, onCancel, onConfirm }: ConfirmFinalizeDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    cancelRef.current?.focus(); // showModal would focus the first control; start on Cancel instead
    return () => dialog?.close();
  }, []);

  const confirm = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    const outcome = await onConfirm();
    if (!outcome.ok) {
      setBusy(false);
      if (!outcome.close) setError(outcome.message);
    }
  };

  const headingId = `finalize-${c.action}-title`;
  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={headingId}
      aria-describedby={`${headingId}-note`}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onCancel();
      }}
      className="m-auto w-[min(26rem,calc(100%-2rem))] max-w-none max-h-[calc(100%-2rem)] flex-col overflow-hidden rounded-sm border border-line-strong bg-surface p-0 text-fg shadow-2xl backdrop:bg-black/70 open:flex"
    >
      <div className="min-h-0 flex-1 overflow-y-auto px-4 pt-3.5 pb-3">
        <h2 id={headingId} className="text-[15px] font-semibold tracking-tight text-fg">
          {c.heading}
        </h2>
        {c.title && <p className="mt-0.5 text-sm break-words text-muted">{c.title}</p>}

        <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 border-y border-line py-2 text-sm">
          {c.title === null && (
            <>
              <dt className="label-tech self-center">Contracts</dt>
              <dd className="text-right font-mono text-fg tabular-nums">{c.count}</dd>
            </>
          )}
          <dt className="label-tech self-center">Total SCU</dt>
          <dd className="text-right font-mono text-fg tabular-nums">{formatScu(c.totalScuMilli)}</dd>
          <dt className="label-tech self-center">Reward</dt>
          <dd className="text-right font-mono text-fg tabular-nums">
            {formatAuec(c.rewardAuec)} <span className="text-xs text-dim">aUEC</span>
          </dd>
        </dl>

        {c.title === null && (
          <p className="mt-2 font-mono text-xs break-words text-muted" aria-label="Contracts included">
            {c.refs.join(" · ")}
          </p>
        )}

        <p id={`${headingId}-note`} className="mt-2 text-xs break-words text-dim">
          {c.note}
        </p>

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
          onClick={onCancel}
          disabled={busy}
          className="h-9 rounded-sm border border-line px-4 text-sm font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg disabled:opacity-50"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={confirm}
          disabled={busy}
          className={[
            "h-9 rounded-sm px-4 text-sm font-semibold transition-[filter] duration-150 hover:brightness-110 disabled:opacity-60",
            c.danger ? "bg-danger text-(color:--color-canvas)" : "bg-accent text-(color:--color-canvas)",
          ].join(" ")}
        >
          {busy ? "Working…" : c.confirmLabel}
        </button>
      </div>
    </dialog>
  );
}
