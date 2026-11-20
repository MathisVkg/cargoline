import { useEffect, useRef, useState, type ReactNode } from "react";
import { AlertTriangle } from "lucide-react";

/** What the caller reports back after trying the action. */
export type DialogOutcome = { ok: true } | { ok: false; message: string };

/**
 * Small modal confirmation. Mount to open, unmount to close. Focus starts on Cancel; Escape
 * cancels; both buttons are disabled while the request runs; a failure shows inline and
 * leaves the dialog open (nothing changed).
 */
export function ConfirmDialog({
  id,
  heading,
  children,
  confirmLabel,
  danger = false,
  onCancel,
  onConfirm,
}: {
  id: string;
  heading: string;
  children: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onCancel: () => void;
  onConfirm: () => Promise<DialogOutcome>;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    cancelRef.current?.focus();
    return () => dialog?.close();
  }, []);

  const confirm = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    const outcome = await onConfirm();
    if (!outcome.ok) {
      setBusy(false);
      setError(outcome.message);
    }
  };

  const headingId = `${id}-title`;
  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={headingId}
      aria-describedby={`${id}-body`}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onCancel();
      }}
      className="m-auto w-[min(28rem,calc(100%-2rem))] max-w-none max-h-[calc(100%-2rem)] flex-col overflow-hidden rounded-sm border border-line-strong bg-surface p-0 text-fg shadow-2xl backdrop:bg-black/70 open:flex"
    >
      <div className="min-h-0 flex-1 overflow-y-auto px-4 pt-3.5 pb-3">
        <h2 id={headingId} className="flex items-center gap-2 text-[15px] font-semibold tracking-tight text-fg">
          {danger && <AlertTriangle className="size-4 shrink-0 text-danger" aria-hidden="true" />}
          {heading}
        </h2>
        <div id={`${id}-body`} className="mt-2 text-sm break-words text-muted">
          {children}
        </div>
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
            "h-9 rounded-sm px-4 text-sm font-semibold text-(color:--color-canvas) transition-[filter] duration-150 hover:brightness-110 disabled:opacity-60",
            danger ? "bg-danger" : "bg-accent",
          ].join(" ")}
        >
          {busy ? "Working…" : confirmLabel}
        </button>
      </div>
    </dialog>
  );
}
