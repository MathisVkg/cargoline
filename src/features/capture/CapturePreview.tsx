import { useCallback, useEffect, useId, useReducer, useRef, useState } from "react";
import { AlertTriangle, Camera, ChevronRight, Loader2, RefreshCw } from "lucide-react";
import { describeError, toCommandError } from "../contracts/api";
import {
  cancelRecognition,
  captureImageUrl,
  captureScreenshot,
  discardCapture,
  getSettings,
  recognizeCapture,
} from "./api";
import {
  canCapture,
  captureFailureMessage,
  captureStatus,
  initialCaptureModel,
  ocrFailureMessage,
  ocrToStart,
  reduceCapture,
  shouldAutoCapture,
  type CaptureModel,
} from "./captureModel";
import type { OcrRegion } from "./types";

export interface ContractCapture {
  model: CaptureModel;
  /** The unsaved capture to link when the contract is saved. */
  captureId: () => number | null;
  /** Capture now (Capture / Recapture / Retry). Replaces the previous unsaved capture. */
  capture: () => void;
  /** Read the current capture's text again (after an OCR error). */
  retryOcr: () => void;
  /** Call right before the form closes after a successful save: the capture is kept. */
  markSaved: () => void;
}

/**
 * Screenshot flow of the Add Contract form. In Edit mode it does nothing.
 * - Setting on: waits the configured delay, then captures (the form stays usable).
 * - Every new capture is read (OCR) in the background; a recapture restarts it.
 * - Closing cancels a pending countdown and a running OCR; a capture already running
 *   finishes and is deleted. Closing without saving deletes the unsaved capture.
 */
export function useContractCapture(mode: "add" | "edit"): ContractCapture {
  const [model, dispatch] = useReducer(reduceCapture, initialCaptureModel);
  const closedRef = useRef(false);
  const savedRef = useRef(false);
  const captureIdRef = useRef<number | null>(null);
  const inFlightRef = useRef(false);
  /** The capture whose OCR is running (cancelled on recapture / close). */
  const readingRef = useRef<number | null>(null);

  const cancelReading = () => {
    const reading = readingRef.current;
    readingRef.current = null;
    if (reading !== null) void cancelRecognition(reading).catch(() => undefined);
  };

  const readText = useCallback((captureId: number) => {
    if (closedRef.current || readingRef.current === captureId) return;
    readingRef.current = captureId;
    dispatch({ type: "ocrStart", captureId });
    recognizeCapture(captureId)
      .then((result) => {
        if (!closedRef.current) dispatch({ type: "ocrSucceeded", captureId, result });
      })
      .catch((thrown: unknown) => {
        if (closedRef.current) return;
        const error = toCommandError(thrown);
        const reason = error.kind === "ocr" ? error.reason : "other";
        // The reducer ignores this if a recapture replaced the capture meanwhile.
        dispatch({ type: "ocrFailed", captureId, reason, message: ocrFailureMessage(reason, describeError(error)) });
      })
      .finally(() => {
        if (readingRef.current === captureId) readingRef.current = null;
      });
  }, []);

  const capture = useCallback(() => {
    if (inFlightRef.current || closedRef.current) return;
    inFlightRef.current = true;
    cancelReading(); // the previous capture is about to be replaced
    dispatch({ type: "start" });
    captureScreenshot(captureIdRef.current)
      .then((stored) => {
        if (closedRef.current) {
          void discardCapture(stored.id).catch(() => undefined); // the form closed meanwhile
          return;
        }
        captureIdRef.current = stored.id;
        dispatch({ type: "succeeded", capture: stored });
      })
      .catch((thrown: unknown) => {
        if (closedRef.current) return;
        const error = toCommandError(thrown);
        const reason = error.kind === "capture" ? error.reason : "other";
        dispatch({ type: "failed", reason, message: captureFailureMessage(reason, describeError(error)) });
      })
      .finally(() => {
        inFlightRef.current = false;
      });
  }, []);

  // Start OCR for each new capture.
  const pendingOcr = ocrToStart(model);
  useEffect(() => {
    if (pendingOcr !== null) readText(pendingOcr);
  }, [pendingOcr, readText]);

  useEffect(() => {
    if (mode !== "add") return;
    closedRef.current = false;
    let cancelled = false;
    let timer: number | undefined;
    getSettings()
      .then((settings) => {
        if (cancelled) return;
        const auto = shouldAutoCapture(mode, settings.capture_on_add_contract);
        dispatch({ type: "configured", auto, delayMs: settings.capture_delay_ms });
        if (!auto) return;
        const due = Date.now() + settings.capture_delay_ms;
        const step = () => {
          const left = due - Date.now();
          if (left <= 0) {
            timer = undefined;
            capture();
          } else {
            dispatch({ type: "tick", remainingMs: left });
            timer = window.setTimeout(step, Math.min(100, left));
          }
        };
        step();
      })
      .catch(() => {
        // Settings unavailable: no automatic capture; the Capture button still works.
        if (!cancelled) dispatch({ type: "configured", auto: false, delayMs: 0 });
      });
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
      closedRef.current = true;
      cancelReading(); // before deleting the file it reads
      const unsaved = savedRef.current ? null : captureIdRef.current;
      captureIdRef.current = null;
      if (unsaved !== null) void discardCapture(unsaved).catch(() => undefined);
    };
  }, [mode, capture]);

  return {
    model,
    captureId: () => captureIdRef.current,
    capture,
    retryOcr: () => {
      if (model.capture && model.ocr.status === "error") readText(model.capture.id);
    },
    markSaved: () => {
      savedRef.current = true;
    },
  };
}

/** Collapsed screenshot section at the bottom of the Add Contract form. */
export function CapturePreview({ flow }: { flow: ContractCapture }) {
  const [open, setOpen] = useState(false);
  const bodyId = useId();
  const { model } = flow;
  const status = captureStatus(model);
  const counting = model.phase === "loading" || model.phase === "waiting" || model.phase === "capturing";
  const ocrError = model.ocr.status === "error";
  const toneClass = status.tone === "error" ? "text-danger" : status.tone === "warning" ? "text-accent" : "text-dim";

  return (
    <section aria-label="Screenshot" className="mt-4 rounded-sm border border-line bg-raised/40">
      <div className="flex h-10 items-center gap-2 pr-2">
        <button
          type="button"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={() => setOpen((v) => !v)}
          className="flex h-full min-w-0 flex-1 items-center gap-2 rounded-sm px-3 text-left transition-colors duration-150 hover:bg-raised"
        >
          <ChevronRight
            className={`size-4 shrink-0 text-dim transition-transform duration-150 ${open ? "rotate-90" : ""}`}
            aria-hidden="true"
          />
          <span className="label-tech shrink-0">Screenshot</span>
          {status.busy && <Loader2 className="size-3.5 shrink-0 animate-spin text-accent" aria-hidden="true" />}
          {status.tone !== "normal" && (
            <AlertTriangle
              className={`size-3.5 shrink-0 ${status.tone === "error" ? "text-danger" : "text-accent"}`}
              aria-hidden="true"
            />
          )}
          <span aria-live="polite" className={`min-w-0 truncate text-xs ${toneClass}`} title={status.text}>
            {status.text}
          </span>
        </button>
        {ocrError && (
          <HeaderButton onClick={flow.retryOcr} icon={RefreshCw}>
            Retry
          </HeaderButton>
        )}
        {!counting && (
          <HeaderButton onClick={flow.capture} disabled={!canCapture(model)} icon={model.capture || model.phase === "error" ? RefreshCw : Camera}>
            {model.phase === "error" ? "Retry" : model.capture ? "Recapture" : "Capture"}
          </HeaderButton>
        )}
      </div>
      {open && (
        <div id={bodyId} className="space-y-2 border-t border-line p-2">
          {model.capture ? (
            <img
              src={captureImageUrl(model.capture.id)}
              alt="Screenshot of Star Citizen"
              className="block max-h-[min(60vh,32rem)] w-full rounded-sm bg-black object-contain"
            />
          ) : (
            <p className="px-1 py-2 text-xs text-dim">No screenshot yet.</p>
          )}
          {model.ocr.status === "done" && model.ocr.result && <RecognisedText regions={model.ocr.result.regions} />}
        </div>
      )}
    </section>
  );
}

function HeaderButton({
  onClick,
  disabled,
  icon: Icon,
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  icon: typeof Camera;
  children: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="flex h-7 shrink-0 items-center gap-1.5 rounded-sm border border-line px-2.5 text-xs font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg disabled:opacity-50"
    >
      <Icon className="size-3.5" aria-hidden="true" />
      {children}
    </button>
  );
}

const REGION_LABEL = { title: "Title", reward: "Reward", objectives: "Objectives" } as const;

/** Temporary check view (Task 09 replaces it with form filling): raw lines and confidences. */
function RecognisedText({ regions }: { regions: OcrRegion[] }) {
  return (
    <details className="rounded-sm border border-line bg-surface">
      <summary className="cursor-pointer px-3 py-2 text-xs font-medium text-muted hover:text-fg">Recognised text</summary>
      <div className="space-y-3 border-t border-line px-3 py-2">
        {regions.map((region) => (
          <div key={region.kind}>
            <p className="label-tech mb-1">
              {REGION_LABEL[region.kind]}
              {region.source === "fallback" && <span className="ml-2 normal-case text-accent">(panel not found, fixed area)</span>}
            </p>
            {region.lines.length === 0 ? (
              <p className="text-xs text-dim">No text.</p>
            ) : (
              <ul className="space-y-0.5">
                {region.lines.map((line, i) => (
                  <li key={i} className="flex items-baseline gap-2 text-xs">
                    <span
                      className={`w-8 shrink-0 text-right font-mono ${line.confidence >= 0 && line.confidence < 60 ? "text-accent" : "text-dim"}`}
                      title="Confidence (0–100)"
                    >
                      {line.confidence < 0 ? "–" : Math.round(line.confidence)}
                    </span>
                    <span className="min-w-0 break-words text-fg">{line.text}</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ))}
      </div>
    </details>
  );
}
