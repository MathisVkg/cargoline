import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { AlertTriangle, X } from "lucide-react";
import { ContractForm } from "./ContractForm";
import {
  createContract,
  describeError,
  listLocations,
  listReferenceCommodities,
  listReferenceLocations,
  listUsedCommodities,
  toCommandError,
} from "./api";
import { emptyDraft, isDraftEmpty, issuesByPath, toInput, validateDraft, type ContractDraft } from "./draft";
import { formatScu, parseScuMilli } from "./scu";
import type { CommandError, ContractDetail, LocationEntry, ReferenceCommodity, ReferenceLocation, ValidationIssue } from "./types";

interface AddContractModalProps {
  onClose: () => void;
  onSaved: (contract: ContractDetail) => void;
  /** Pre-filled values (e.g. future OCR results). Defaults to an empty form. */
  initialDraft?: ContractDraft;
}

/**
 * Centered modal for adding a contract. Mount it to open; unmount to close.
 * Uses the native <dialog> element for focus containment and an inert backdrop.
 */
export function AddContractModal({ onClose, onSaved, initialDraft }: AddContractModalProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [draft, setDraft] = useState<ContractDraft>(() => initialDraft ?? emptyDraft());
  const [attempted, setAttempted] = useState(false);
  const [serverIssues, setServerIssues] = useState<ValidationIssue[]>([]);
  const [error, setError] = useState<CommandError | null>(null);
  const [saving, setSaving] = useState(false);
  const [referenceLocations, setReferenceLocations] = useState<ReferenceLocation[]>([]);
  const [referenceCommodities, setReferenceCommodities] = useState<ReferenceCommodity[]>([]);
  const [userLocations, setUserLocations] = useState<LocationEntry[]>([]);
  const [usedCommodities, setUsedCommodities] = useState<string[]>([]);

  useEffect(() => {
    let cancelled = false;
    // Suggestions only: if any list fails to load, typing still works.
    function load<T>(request: Promise<T>, apply: (value: T) => void) {
      request.then((value) => !cancelled && apply(value)).catch(() => undefined);
    }
    load(listReferenceLocations(), setReferenceLocations);
    load(listReferenceCommodities(), setReferenceCommodities);
    load(listLocations(), setUserLocations);
    load(listUsedCommodities(), setUsedCommodities);
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    return () => dialog?.close();
  }, []);

  const issues = useMemo(
    () => issuesByPath([...(attempted ? validateDraft(draft) : []), ...serverIssues]),
    [attempted, draft, serverIssues],
  );

  const totalMilli = draft.requirements.reduce((sum, r) => {
    const q = parseScuMilli(r.quantityScu);
    return q.ok ? sum + q.value : sum;
  }, 0);

  const change = (next: ContractDraft) => {
    setDraft(next);
    setServerIssues([]);
  };

  const focusFirstInvalid = () =>
    requestAnimationFrame(() => dialogRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus());

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (saving) return;
    setAttempted(true);
    setError(null);
    if (validateDraft(draft).length > 0) {
      setServerIssues([]);
      focusFirstInvalid();
      return;
    }
    setSaving(true);
    try {
      onSaved(await createContract(toInput(draft)));
    } catch (thrown) {
      const failure = toCommandError(thrown);
      if (failure.kind === "validation") {
        setServerIssues(failure.issues);
        focusFirstInvalid();
      } else {
        setError(failure);
      }
    } finally {
      setSaving(false);
    }
  };

  const requestClose = () => {
    if (!saving) onClose();
  };

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby="add-contract-title"
      aria-describedby="add-contract-description"
      onCancel={(e) => {
        // Escape only closes an untouched form, so typed data is never lost by accident.
        e.preventDefault();
        if (isDraftEmpty(draft)) requestClose();
      }}
      className={[
        "m-auto w-[min(56rem,calc(100%-2rem))] max-w-none max-h-[calc(100%-2rem)] flex-col overflow-hidden p-0 open:flex",
        "rounded-sm border border-line-strong bg-surface text-fg shadow-2xl backdrop:bg-black/70",
      ].join(" ")}
    >
      <form onSubmit={submit} noValidate className="flex min-h-0 flex-1 flex-col">
        <header className="flex shrink-0 items-center justify-between gap-4 border-b border-line px-4 py-2.5">
          <div className="min-w-0">
            <h2 id="add-contract-title" className="text-base font-semibold tracking-tight">
              Add Contract
            </h2>
            <p id="add-contract-description" className="text-xs break-words text-dim">
              Nothing is saved until you confirm.
            </p>
          </div>
          <button
            type="button"
            onClick={requestClose}
            disabled={saving}
            aria-label="Close"
            className="grid size-9 shrink-0 place-items-center rounded-sm text-dim transition-colors duration-150 hover:bg-raised hover:text-fg"
          >
            <X className="size-5" aria-hidden="true" />
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-3">
          <ContractForm draft={draft} onChange={change} issues={issues} referenceLocations={referenceLocations}
            referenceCommodities={referenceCommodities}
            userLocations={userLocations}
            usedCommodities={usedCommodities}
            disabled={saving} />
        </div>

        <footer className="shrink-0 border-t border-line bg-surface px-4 py-2.5">
          {error && (
            <div
              role="alert"
              className="mb-3 flex items-start gap-2 rounded-sm border border-danger/40 bg-danger-soft px-3 py-2 text-sm text-fg"
            >
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-danger" aria-hidden="true" />
              <span className="min-w-0 break-words">{describeError(error)}</span>
            </div>
          )}
          {attempted && issues.size > 0 && !error && (
            <p role="status" className="mb-3 text-sm text-danger">
              Some fields need attention. Your entries are kept.
            </p>
          )}
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="label-tech" aria-live="polite">
              Total <span className="font-mono text-fg">{formatScu(totalMilli)}</span> SCU ·{" "}
              {draft.requirements.length} objective{draft.requirements.length === 1 ? "" : "s"}
            </p>
            <div className="ml-auto flex items-center gap-2">
              <button
                type="button"
                onClick={requestClose}
                disabled={saving}
                className="h-9 rounded-sm border border-line px-4 text-sm font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={saving}
                className="h-9 rounded-sm bg-accent px-4 text-sm font-semibold text-(color:--color-base) transition-[filter] duration-150 hover:brightness-110 disabled:opacity-60"
              >
                {saving ? "Saving…" : "Add Contract"}
              </button>
            </div>
          </div>
        </footer>
      </form>
    </dialog>
  );
}
