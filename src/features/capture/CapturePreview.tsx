import { useCallback, useEffect, useId, useReducer, useRef, useState } from "react";
import { AlertTriangle, Camera, ChevronRight, Loader2, RefreshCw } from "lucide-react";
import { describeError, toCommandError } from "../contracts/api";
import { captureImageUrl, captureScreenshot, discardCapture, getSettings } from "./api";
import {
  canCapture,
  captureFailureMessage,
  captureHeader,
  initialCaptureModel,
  reduceCapture,
  shouldAutoCapture,
  type CaptureModel,
} from "./captureModel";

export interface ContractCapture {
  model: CaptureModel;
  /** The unsaved capture to link when the contract is saved. */
  captureId: () => number | null;
  /** Capture now (Capture / Recapture / Retry). Replaces the previous unsaved capture. */
  capture: () => void;
  /** Call right before the form closes after a successful save: the capture is kept. */
  markSaved: () => void;
}

/**
 * Screenshot flow of the Add Contract form. In Edit mode it does nothing.
 * - Setting on: waits the configured delay, then captures (the form stays usable).
 * - Closing cancels a pending countdown; a capture already running finishes and is deleted.
 * - Closing without saving deletes the unsaved capture.
 */
export function useContractCapture(mode: "add" | "edit"): ContractCapture {
  const [model, dispatch] = useReducer(reduceCapture, initialCaptureModel);
  const closedRef = useRef(false);
  const savedRef = useRef(false);
  const captureIdRef = useRef<number | null>(null);
  const inFlightRef = useRef(false);

  const capture = useCallback(() => {
    if (inFlightRef.current || closedRef.current) return;
    inFlightRef.current = true;
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
      const unsaved = savedRef.current ? null : captureIdRef.current;
      captureIdRef.current = null;
      if (unsaved !== null) void discardCapture(unsaved).catch(() => undefined);
    };
  }, [mode, capture]);

  return {
    model,
    captureId: () => captureIdRef.current,
    capture,
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
  const running = model.phase === "loading" || model.phase === "waiting" || model.phase === "capturing";
  const action = model.phase === "error" ? "Retry" : model.capture ? "Recapture" : "Capture";
  const ActionIcon = model.capture || model.phase === "error" ? RefreshCw : Camera;

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
          {model.phase === "capturing" && <Loader2 className="size-3.5 shrink-0 animate-spin text-accent" aria-hidden="true" />}
          {model.phase === "error" && <AlertTriangle className="size-3.5 shrink-0 text-danger" aria-hidden="true" />}
          <span
            aria-live="polite"
            className={`min-w-0 truncate text-xs ${model.phase === "error" ? "text-danger" : "text-dim"}`}
          >
            {captureHeader(model)}
          </span>
        </button>
        {!running && (
          <button
            type="button"
            onClick={flow.capture}
            disabled={!canCapture(model)}
            className="flex h-7 shrink-0 items-center gap-1.5 rounded-sm border border-line px-2.5 text-xs font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg disabled:opacity-50"
          >
            <ActionIcon className="size-3.5" aria-hidden="true" />
            {action}
          </button>
        )}
      </div>
      {open && (
        <div id={bodyId} className="border-t border-line p-2">
          {model.capture ? (
            <img
              src={captureImageUrl(model.capture.id)}
              alt="Screenshot of Star Citizen"
              className="block max-h-[min(60vh,32rem)] w-full rounded-sm bg-black object-contain"
            />
          ) : (
            <p className="px-1 py-2 text-xs text-dim">No screenshot yet.</p>
          )}
        </div>
      )}
    </section>
  );
}
