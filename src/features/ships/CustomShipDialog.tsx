import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { AlertTriangle } from "lucide-react";
import { describeError, toCommandError } from "../contracts/api";
import { CUSTOM_NAME_MAX, customShipInput, customShipIssues, type CustomShipIssues } from "./model";
import { addCustomShip, editCustomShip, useShips } from "./store";
import type { ShipEntry } from "./types";

/**
 * Add or edit a custom ship (Task 15): Name and Cargo (SCU), Cancel and Save. Mount to open,
 * unmount to close. Validation messages are inline; the backend checks the same rules and its
 * messages are shown in the same places. After adding, the new ship is selected.
 */
export function CustomShipDialog({ editing, onClose }: { editing: ShipEntry | null; onClose: (saved: ShipEntry | null) => void }) {
  const id = useId();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const { ships } = useShips();
  const [name, setName] = useState(editing?.name ?? "");
  const [cargo, setCargo] = useState(editing ? String(editing.cargoScu) : "");
  const [issues, setIssues] = useState<CustomShipIssues>({});
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    nameRef.current?.focus();
    return () => dialog?.close();
  }, []);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    const draft = { name, cargo };
    const found = customShipIssues(draft, ships.filter((s) => s.isCustom), editing?.id ?? null);
    setIssues(found);
    setFailure(null);
    if (Object.keys(found).length > 0) {
      (found.name ? nameRef.current : dialogRef.current?.querySelector<HTMLInputElement>(`#${CSS.escape(`${id}-cargo`)}`))?.focus();
      return;
    }
    setBusy(true);
    try {
      const input = customShipInput(draft);
      const saved = editing ? await editCustomShip(editing.id, input) : await addCustomShip(input);
      onClose(saved);
    } catch (thrown) {
      const error = toCommandError(thrown);
      setBusy(false);
      if (error.kind === "validation") {
        const next: CustomShipIssues = {};
        for (const i of error.issues) if (i.path === "name" || i.path === "cargoScu") next[i.path] = i.message;
        setIssues(next);
      } else setFailure(describeError(error));
    }
  };

  const field = "h-9 w-full rounded-sm border bg-raised px-3 text-sm text-fg placeholder:text-placeholder";
  const heading = editing ? "Edit custom ship" : "Add custom ship";
  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={`${id}-title`}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose(null);
      }}
      className="m-auto w-[min(24rem,calc(100%-2rem))] max-w-none flex-col overflow-hidden rounded-sm border border-line-strong bg-surface p-0 text-fg shadow-2xl backdrop:bg-black/70 open:flex"
    >
      <form onSubmit={submit} noValidate>
        <div className="flex flex-col gap-3 px-4 pt-3.5 pb-3">
          <h2 id={`${id}-title`} className="text-[15px] font-semibold tracking-tight text-fg">
            {heading}
          </h2>
          <div className="flex flex-col gap-1">
            <label htmlFor={`${id}-name`} className="text-xs text-muted">
              Name
            </label>
            <input
              ref={nameRef}
              id={`${id}-name`}
              value={name}
              maxLength={CUSTOM_NAME_MAX + 20}
              onChange={(e) => setName(e.target.value)}
              aria-invalid={issues.name ? true : undefined}
              aria-describedby={issues.name ? `${id}-name-error` : undefined}
              placeholder="e.g. Retaliator Cargo"
              autoComplete="off"
              spellCheck={false}
              className={`${field} ${issues.name ? "border-danger" : "border-line"}`}
            />
            {issues.name && (
              <p id={`${id}-name-error`} className="text-xs text-danger">
                {issues.name}
              </p>
            )}
          </div>
          <div className="flex flex-col gap-1">
            <label htmlFor={`${id}-cargo`} className="text-xs text-muted">
              Cargo (SCU)
            </label>
            <input
              id={`${id}-cargo`}
              value={cargo}
              inputMode="numeric"
              onChange={(e) => setCargo(e.target.value)}
              aria-invalid={issues.cargoScu ? true : undefined}
              aria-describedby={issues.cargoScu ? `${id}-cargo-error` : undefined}
              placeholder="e.g. 74"
              autoComplete="off"
              className={`${field} font-mono ${issues.cargoScu ? "border-danger" : "border-line"}`}
            />
            {issues.cargoScu && (
              <p id={`${id}-cargo-error`} className="text-xs text-danger">
                {issues.cargoScu}
              </p>
            )}
          </div>
          <p className="text-xs text-dim">Shown as manufacturer “Custom”. It only sets the capacity used by the views.</p>
          {failure && (
            <div role="alert" className="flex items-start gap-2 rounded-sm border border-danger/40 bg-danger-soft px-3 py-2 text-sm">
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-danger" aria-hidden="true" />
              <span className="min-w-0 break-words">{failure}</span>
            </div>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-line px-4 py-2.5">
          <button
            type="button"
            onClick={() => onClose(null)}
            disabled={busy}
            className="h-9 rounded-sm border border-line px-4 text-sm font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={busy}
            className="h-9 rounded-sm bg-accent px-4 text-sm font-semibold text-(color:--color-canvas) transition-[filter] duration-150 hover:brightness-110 disabled:opacity-60"
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </dialog>
  );
}
