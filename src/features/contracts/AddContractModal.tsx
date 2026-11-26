import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { AlertTriangle, X } from "lucide-react";
import { ContractForm } from "./ContractForm";
import { CapturePreview, useContractCapture } from "../capture/CapturePreview";
import { ApplyReadingBar, ReadingBanners } from "../capture/ReadingNotices";
import { ScrollText } from "lucide-react";
import type { AcceptedEvent } from "../gamelog/types";
import { queueSummary } from "../gamelog/flow";
import {
  applyNeedsConfirm,
  clearChangedFlags,
  crossCheckFlags,
  draftFromParsed,
  findDuplicate,
  readingAction,
  type FieldFlags,
} from "../capture/prefill";
import type { ParsedContract } from "../capture/types";
import {
  createContract,
  describeError,
  listLocations,
  listReferenceCommodities,
  listReferenceLocations,
  listUsedCommodities,
  toCommandError,
  updateContract,
} from "./api";
import {
  draftFromContract,
  emptyDraft,
  isDraftDirty,
  issuesByPath,
  toInput,
  validateDraft,
  type ContractDraft,
} from "./draft";
import { contractRef } from "./grouping";
import { formatScu, parseScuMilli } from "./scu";
import type { CommandError, ContractDetail, LocationEntry, ReferenceCommodity, ReferenceLocation, ValidationIssue } from "./types";

interface AddContractModalProps {
  onClose: () => void;
  onSaved: (contract: ContractDetail) => void;
  /** Edit mode: the stored contract to change (prefills the same form). */
  editing?: ContractDetail;
  /** Edit mode: the contract is gone or no longer active (page closes, reloads, explains). */
  onStale?: (message: string) => void;
  /** Pre-filled values for a new contract. Defaults to an empty form. */
  initialDraft?: ContractDraft;
  /** Active contracts, for the "already added?" hint (Add mode). */
  activeContracts?: readonly ContractDetail[];
  /** The in-game contract (Game.log accept) this form is for; its MissionId is saved with it. */
  gameEvent?: AcceptedEvent | null;
  /** Opened automatically by a Game.log accept (saved with source "auto_capture"). */
  fromGameLog?: boolean;
  /** Capture even when "Capture when opening Add Contract" is off (Game.log, shortcut). */
  forceCapture?: boolean;
  /** Changes to ask for a recapture (shortcut, "Capture now"). */
  recaptureSignal?: number;
  /** Game.log accepts waiting while this form is open. */
  queue?: readonly AcceptedEvent[];
  onCaptureNow?: (missionId: string) => void;
}

const NO_QUEUE: readonly AcceptedEvent[] = [];

const NO_FLAGS: FieldFlags = new Map();
const NO_CONTRACTS: readonly ContractDetail[] = [];

/**
 * Centered modal for adding or editing a contract, using the one shared
 * ContractForm. Mount it to open; unmount to close. Uses the native <dialog>
 * element for focus containment and an inert backdrop. Closing with unsaved
 * changes (Esc, Cancel or ✕) asks for confirmation first.
 */
export function AddContractModal({
  onClose,
  onSaved,
  editing,
  onStale,
  initialDraft,
  activeContracts = NO_CONTRACTS,
  gameEvent = null,
  fromGameLog = false,
  forceCapture = false,
  recaptureSignal = 0,
  queue = NO_QUEUE,
  onCaptureNow,
}: AddContractModalProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const keepEditingRef = useRef<HTMLButtonElement>(null);
  // What the form started with; "dirty" means the payload would differ from it.
  const [baseline] = useState<ContractDraft>(() => (editing ? draftFromContract(editing) : (initialDraft ?? emptyDraft())));
  const [draft, setDraft] = useState<ContractDraft>(baseline);
  const [attempted, setAttempted] = useState(false);
  const [serverIssues, setServerIssues] = useState<ValidationIssue[]>([]);
  const [error, setError] = useState<CommandError | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const [referenceLocations, setReferenceLocations] = useState<ReferenceLocation[]>([]);
  const [referenceCommodities, setReferenceCommodities] = useState<ReferenceCommodity[]>([]);
  const [userLocations, setUserLocations] = useState<LocationEntry[]>([]);
  const [usedCommodities, setUsedCommodities] = useState<string[]>([]);
  // Filling from the screenshot (Task 09).
  const [flags, setFlags] = useState<FieldFlags>(NO_FLAGS);
  /** The user typed since the form was last filled (or since it opened). */
  const [userEdited, setUserEdited] = useState(false);
  /** The form was filled from a screenshot: saved with source "ocr", even if edited afterwards. */
  const [filledFromCapture, setFilledFromCapture] = useState(false);
  /** The reading that filled the form (for its banners). */
  const [applied, setApplied] = useState<ParsedContract | null>(null);
  /** A reading not applied because the user had typed: offered in the "Apply to form" bar. */
  const [pending, setPending] = useState<ParsedContract | null>(null);
  const [confirmApply, setConfirmApply] = useState(false);
  const handledReading = useRef<ParsedContract | null>(null);

  const ref = editing ? contractRef(editing.id) : null;
  // Add mode only: background screenshot after the configured delay (never in Edit mode).
  const captureFlow = useContractCapture(editing ? "edit" : "add", { force: forceCapture });

  // Shortcut / "Capture now": recapture in this form (Add mode only).
  const lastSignal = useRef(recaptureSignal);
  useEffect(() => {
    if (lastSignal.current === recaptureSignal) return;
    lastSignal.current = recaptureSignal;
    if (!editing) captureFlow.capture();
  }, [recaptureSignal, editing, captureFlow]);
  const dirty = isDraftDirty(draft, baseline);

  const applyReading = (parsed: ParsedContract) => {
    const filled = draftFromParsed(parsed);
    setDraft(filled.draft);
    setFlags(filled.flags);
    setUserEdited(false);
    setFilledFromCapture(true);
    setApplied(parsed);
    setPending(null);
    setConfirmApply(false);
    setServerIssues([]);
    setAttempted(false);
  };

  // A new reading fills the form if the user has not typed; otherwise it is offered.
  const reading = editing ? null : captureFlow.model.ocr.parsed;
  useEffect(() => {
    if (!reading || handledReading.current === reading) return;
    handledReading.current = reading;
    if (readingAction({ draft, userEdited }) === "fill") applyReading(reading);
    else {
      setPending(reading);
      setConfirmApply(false);
    }
    // Only a new reading triggers this; the form state is read at that moment.
  }, [reading]);

  // Cross-check with the Game.log first objective: differing fields are flagged, never overwritten.
  const objective = gameEvent?.objective ?? null;
  const checked = useRef<{ reading: ParsedContract | null; objective: unknown }>({ reading: null, objective: null });
  useEffect(() => {
    if (!applied || !objective) return;
    if (checked.current.reading === applied && checked.current.objective === objective) return;
    checked.current = { reading: applied, objective };
    const mismatches = crossCheckFlags(draft, objective);
    if (mismatches.size > 0) setFlags((current) => new Map([...current, ...mismatches]));
    // Runs when a reading is applied or the objective arrives; the draft is read at that moment.
  }, [applied, objective]);

  const duplicate = useMemo(() => (editing ? null : findDuplicate(draft, activeContracts)), [editing, draft, activeContracts]);

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
    // showModal() focuses the first control (the close button); start in the title field instead.
    dialog?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
    return () => dialog?.close();
  }, []);

  useEffect(() => {
    if (confirmDiscard) keepEditingRef.current?.focus();
  }, [confirmDiscard]);

  const issues = useMemo(
    () => issuesByPath([...(attempted ? validateDraft(draft) : []), ...serverIssues]),
    [attempted, draft, serverIssues],
  );

  const totalMilli = draft.requirements.reduce((sum, r) => {
    const q = parseScuMilli(r.quantityScu);
    return q.ok ? sum + q.value : sum;
  }, 0);

  const change = (next: ContractDraft) => {
    setFlags((current) => clearChangedFlags(current, draft, next));
    setDraft(next);
    setUserEdited(true);
    setServerIssues([]);
  };

  const focusFirstInvalid = () =>
    requestAnimationFrame(() => dialogRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus());

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (saving) return;
    setConfirmDiscard(false);
    setAttempted(true);
    setError(null);
    if (validateDraft(draft).length > 0) {
      setServerIssues([]);
      focusFirstInvalid();
      return;
    }
    setSaving(true);
    try {
      const input = toInput(draft);
      if (editing) {
        onSaved(await updateContract(editing.id, input));
      } else {
        const saved = await createContract(
          input,
          captureFlow.captureId(),
          filledFromCapture,
          gameEvent?.missionId ?? null,
          fromGameLog && gameEvent !== null,
        );
        captureFlow.markSaved(); // linked to the contract: keep it when the form closes
        onSaved(saved);
      }
    } catch (thrown) {
      const failure = toCommandError(thrown);
      if (failure.kind === "validation") {
        setServerIssues(failure.issues);
        focusFirstInvalid();
      } else if (editing && onStale && (failure.kind === "notFound" || failure.kind === "notActive")) {
        onStale(describeError(failure));
      } else {
        setError(failure);
      }
    } finally {
      setSaving(false);
    }
  };

  /** Esc, Cancel and ✕: close right away when nothing changed, otherwise ask first. */
  const requestClose = () => {
    if (saving) return;
    if (dirty) setConfirmDiscard(true);
    else onClose();
  };

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby="add-contract-title"
      aria-describedby="add-contract-description"
      onCancel={(e) => {
        e.preventDefault();
        if (confirmDiscard) setConfirmDiscard(false);
        else requestClose();
      }}
      className={[
        "m-auto w-[min(56rem,calc(100%-2rem))] max-w-none max-h-[calc(100%-2rem)] flex-col overflow-hidden p-0 open:flex",
        "rounded-sm border border-line-strong bg-surface text-fg shadow-2xl backdrop:bg-black/70",
      ].join(" ")}
    >
      <form onSubmit={submit} noValidate className="flex min-h-0 flex-1 flex-col">
        <header className="flex shrink-0 items-center justify-between gap-4 border-b border-line px-4 py-2.5">
          <div className="min-w-0">
            <h2 id="add-contract-title" className="text-[15px] font-semibold tracking-tight text-fg">
              {ref ? `Edit ${ref}` : "Add Contract"}
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
          {!editing && gameEvent && (
            <p className="mb-2 flex items-center gap-1.5 text-xs text-muted">
              <ScrollText className="size-3.5 shrink-0 text-accent" aria-hidden="true" />
              <span className="min-w-0 break-words">In-game contract: {gameEvent.title}</span>
            </p>
          )}
          {!editing && <QueueBar queue={queue} onCaptureNow={onCaptureNow} />}
          {!editing && <ReadingBanners applied={applied} duplicate={duplicate} frame={captureFlow.model.capture} />}
          <ContractForm
            draft={draft}
            onChange={change}
            issues={issues}
            referenceLocations={referenceLocations}
            referenceCommodities={referenceCommodities}
            userLocations={userLocations}
            usedCommodities={usedCommodities}
            disabled={saving}
            flags={flags}
            notice={
              pending && (
                <ApplyReadingBar
                  confirming={confirmApply}
                  onApply={() => (applyNeedsConfirm(draft) ? setConfirmApply(true) : applyReading(pending))}
                  onConfirm={() => applyReading(pending)}
                  onCancel={() => setConfirmApply(false)}
                />
              )
            }
          />
          {!editing && (
            <CapturePreview flow={captureFlow} prefix={fromGameLog && gameEvent ? "Contract accepted in game" : undefined} />
          )}
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
          {confirmDiscard ? (
            <div role="alertdialog" aria-label="Discard unsaved changes?" className="flex flex-wrap items-center justify-between gap-3">
              <p className="text-sm text-fg">{ref ? "Discard your changes to this contract?" : "Discard this new contract?"}</p>
              <div className="ml-auto flex items-center gap-2">
                <button
                  ref={keepEditingRef}
                  type="button"
                  onClick={() => setConfirmDiscard(false)}
                  className="h-9 rounded-sm border border-line px-4 text-sm font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg"
                >
                  Keep editing
                </button>
                <button
                  type="button"
                  onClick={onClose}
                  className="h-9 rounded-sm bg-danger px-4 text-sm font-semibold text-(color:--color-canvas) transition-[filter] duration-150 hover:brightness-110"
                >
                  Discard
                </button>
              </div>
            </div>
          ) : (
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
                  className="h-9 rounded-sm bg-accent px-4 text-sm font-semibold text-(color:--color-canvas) transition-[filter] duration-150 hover:brightness-110 disabled:opacity-60"
                >
                  {saving ? "Saving…" : ref ? "Save changes" : "Add Contract"}
                </button>
              </div>
            </div>
          )}
        </footer>
      </form>
    </dialog>
  );
}

/** "Accepted in game: <title> · Capture now", for Game.log accepts that arrived while this form is open. */
function QueueBar({ queue, onCaptureNow }: { queue: readonly AcceptedEvent[]; onCaptureNow?: (missionId: string) => void }) {
  const summary = queueSummary(queue);
  if (!summary) return null;
  return (
    <div role="status" className="mb-3 flex flex-wrap items-center gap-2 rounded-sm border border-line bg-raised px-3 py-2 text-sm">
      <ScrollText className="size-4 shrink-0 text-accent" aria-hidden="true" />
      <span className="min-w-0 flex-1 break-words text-fg">
        Accepted in game: {summary.first.title}
        {summary.more > 0 && <span className="ml-2 text-xs text-dim">+{summary.more} more waiting</span>}
      </span>
      {onCaptureNow && (
        <button
          type="button"
          onClick={() => onCaptureNow(summary.first.missionId)}
          className="h-7 rounded-sm bg-accent px-3 text-xs font-semibold text-(color:--color-canvas) transition-[filter] duration-150 hover:brightness-110"
        >
          Capture now
        </button>
      )}
    </div>
  );
}
