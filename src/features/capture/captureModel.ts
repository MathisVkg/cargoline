// Pure state machine of the Add Contract screenshot preview (no React, no IPC).
import type { CaptureFailure, OcrFailure } from "../contracts/types.ts";
import type { CaptureInfo, OcrResult, ParsedContract } from "./types.ts";

export type CapturePhase =
  /** Settings not loaded yet (Add mode). */
  | "loading"
  /** Nothing automatic: "Capture when opening Add Contract" is off, or nothing ran yet. */
  | "idle"
  | "waiting"
  | "capturing"
  | "captured"
  | "error";

export interface CaptureModel {
  phase: CapturePhase;
  /** Countdown while waiting, in ms. */
  remainingMs: number;
  /** Latest stored, unsaved capture: linked on save, deleted on close or recapture. */
  capture: CaptureInfo | null;
  error: { reason: CaptureFailure | "other"; message: string } | null;
  /** The form was closed: a late result must be deleted, never shown. */
  closed: boolean;
  /** Text recognition of `capture` (restarted by every new capture). */
  ocr: OcrView;
  /** The single "screen unknown" retry was used (once per opening, never a loop). */
  retried: boolean;
  /** The retry capture is running: the header says "Checking screen again…". */
  checkingAgain: boolean;
}

export interface OcrView {
  status: "idle" | "reading" | "done" | "error";
  result: OcrResult | null;
  /** The contract read from `result`. */
  parsed: ParsedContract | null;
  error: { reason: OcrFailure | "other"; message: string } | null;
}

const OCR_IDLE: OcrView = { status: "idle", result: null, parsed: null, error: null };

export type CaptureEvent =
  | { type: "configured"; auto: boolean; delayMs: number }
  | { type: "tick"; remainingMs: number }
  | { type: "start" }
  /** The automatic retry started / finished; `better` replaces the reading if it won. */
  | { type: "retryStart" }
  | { type: "retryFinished"; better?: { capture: CaptureInfo; result: OcrResult; parsed: ParsedContract } }
  | { type: "succeeded"; capture: CaptureInfo }
  | { type: "failed"; reason: CaptureFailure | "other"; message: string }
  | { type: "closed" }
  /** OCR events name their capture: a result for an older capture is ignored. */
  | { type: "ocrStart"; captureId: number }
  | { type: "ocrSucceeded"; captureId: number; result: OcrResult; parsed?: ParsedContract | null }
  | { type: "ocrFailed"; captureId: number; reason: OcrFailure | "other"; message: string };

export const initialCaptureModel: CaptureModel = {
  phase: "loading",
  remainingMs: 0,
  capture: null,
  error: null,
  closed: false,
  ocr: OCR_IDLE,
  retried: false,
  checkingAgain: false,
};

/** Edit mode never captures; Add mode captures automatically only when the setting is on. */
export function shouldAutoCapture(mode: "add" | "edit", captureOnAddContract: boolean): boolean {
  return mode === "add" && captureOnAddContract;
}

export function reduceCapture(model: CaptureModel, event: CaptureEvent): CaptureModel {
  if (model.closed) return model; // nothing changes after close (late results are deleted by the caller)
  switch (event.type) {
    case "configured":
      if (model.phase !== "loading") return model;
      return event.auto
        ? { ...model, phase: "waiting", remainingMs: Math.max(0, event.delayMs) }
        : { ...model, phase: "idle" };
    case "tick":
      return model.phase === "waiting" ? { ...model, remainingMs: Math.max(0, event.remainingMs) } : model;
    case "start":
      if (model.phase === "capturing") return model; // one at a time
      // A recapture restarts OCR (the caller cancels the running one).
      return { ...model, phase: "capturing", remainingMs: 0, error: null, ocr: OCR_IDLE };
    case "succeeded":
      return model.phase === "capturing"
        ? { ...model, phase: "captured", capture: event.capture, error: null, ocr: OCR_IDLE }
        : model;
    case "failed":
      // A failed recapture keeps the previous capture (still linked on save).
      return model.phase === "capturing"
        ? { ...model, phase: "error", error: { reason: event.reason, message: event.message } }
        : model;
    case "closed":
      return { ...model, closed: true };
    case "ocrStart":
      if (model.phase !== "captured" || model.capture?.id !== event.captureId) return model;
      return { ...model, ocr: { status: "reading", result: null, parsed: null, error: null } };
    case "ocrSucceeded":
      if (model.capture?.id !== event.captureId || model.ocr.status !== "reading") return model;
      return { ...model, ocr: { status: "done", result: event.result, parsed: event.parsed ?? null, error: null } };
    case "ocrFailed":
      if (model.capture?.id !== event.captureId || model.ocr.status !== "reading") return model;
      return {
        ...model,
        ocr: { status: "error", result: null, parsed: null, error: { reason: event.reason, message: event.message } },
      };
    case "retryStart":
      return model.retried ? model : { ...model, retried: true, checkingAgain: true };
    case "retryFinished":
      if (!model.checkingAgain) return model;
      if (!event.better) return { ...model, checkingAgain: false };
      return {
        ...model,
        checkingAgain: false,
        capture: event.better.capture,
        ocr: { status: "done", result: event.better.result, parsed: event.better.parsed, error: null },
      };
  }
}

/**
 * Retry once (after 1.0 s) when the screen is unknown but objectives were read: the panel may
 * still have been animating. Never on a recognised screen, never twice per opening.
 */
export function shouldRetry(model: CaptureModel): boolean {
  const parsed = model.ocr.parsed;
  return (
    !model.closed &&
    !model.retried &&
    model.phase === "captured" &&
    model.ocr.status === "done" &&
    parsed !== null &&
    parsed.screen === "unknown" &&
    parsed.requirements.length >= 1
  );
}

/** The better of two readings: a recognised screen first, then fewer problems; ties keep the first. */
export function betterReading(first: ParsedContract, second: ParsedContract): "first" | "second" {
  const recognised = (p: ParsedContract) => (p.screen === "unknown" ? 0 : 1);
  if (recognised(second) !== recognised(first)) return recognised(second) > recognised(first) ? "second" : "first";
  return second.problems.length < first.problems.length ? "second" : "first";
}

/** The capture whose OCR should start now (a new capture that has not been read yet). */
export function ocrToStart(model: CaptureModel): number | null {
  return !model.closed && model.phase === "captured" && model.capture && model.ocr.status === "idle"
    ? model.capture.id
    : null;
}

/** The capture whose OCR must be cancelled on close or recapture. */
export function ocrToCancel(model: CaptureModel): number | null {
  return model.ocr.status === "reading" ? (model.capture?.id ?? null) : null;
}

export type StatusTone = "normal" | "warning" | "error";

/** The header line: capture state first, then the OCR state once a capture exists. */
export function captureStatus(model: CaptureModel): { text: string; tone: StatusTone; busy: boolean } {
  if (model.phase === "captured" || (model.phase === "idle" && model.capture)) {
    const { ocr } = model;
    const seconds = (ms: number) => `${(ms / 1000).toFixed(1)} s`;
    if (model.checkingAgain) return { text: "Checking screen again…", tone: "normal", busy: true };
    switch (ocr.status) {
      case "reading":
        return { text: "Reading text…", tone: "normal", busy: true };
      case "done": {
        const r = ocr.result!;
        if (r.screen === "accepted")
          return { text: `Text read · Accepted contract (${seconds(r.timings.totalMs)})`, tone: "normal", busy: false };
        if (r.screen === "offer")
          return { text: "Text read · Offer screen — amounts may change after accepting", tone: "warning", busy: false };
        return { text: `Text read · Contract screen not recognised (${seconds(r.timings.totalMs)})`, tone: "warning", busy: false };
      }
      case "error":
        return { text: ocr.error?.message ?? "Text could not be read", tone: "error", busy: false };
      case "idle":
        break;
    }
  }
  return {
    text: captureHeader(model),
    tone: model.phase === "error" ? "error" : "normal",
    busy: model.phase === "capturing",
  };
}

/** Short, user-facing message for a typed OCR failure. */
export function ocrFailureMessage(reason: OcrFailure | "other", fallback: string): string {
  switch (reason) {
    case "engineMissing":
      return "OCR engine not installed — enter the contract manually";
    case "timeout":
      return "Reading the text took too long";
    case "noPanel":
      return "No contract panel found in the screenshot";
    case "cancelled":
      return "Text reading was cancelled";
    default:
      return fallback || "Text could not be read";
  }
}

/** Whether a finished capture must be deleted instead of shown (the form closed meanwhile). */
export function discardLateResult(model: CaptureModel): boolean {
  return model.closed;
}

/** Capture / Recapture / Retry are offered once the countdown is over and nothing is running. */
export function canCapture(model: CaptureModel): boolean {
  return !model.closed && (model.phase === "idle" || model.phase === "captured" || model.phase === "error");
}

/** The unsaved capture to delete when the form closes without saving. */
export function captureToDiscardOnClose(model: CaptureModel, saved: boolean): number | null {
  return saved ? null : (model.capture?.id ?? null);
}

/** "16:20" in the user's local time. */
export function formatClock(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** The header text for each state (the spinner and buttons are added by the component). */
export function captureHeader(model: CaptureModel): string {
  switch (model.phase) {
    case "loading":
      return "Preparing capture…";
    case "idle":
      return model.capture ? capturedText(model.capture) : "No screenshot";
    case "waiting":
      return `Capturing in ${(Math.ceil(model.remainingMs / 100) / 10).toFixed(1)} s…`;
    case "capturing":
      return "Capturing…";
    case "captured":
      return model.capture ? capturedText(model.capture) : "Screenshot captured";
    case "error":
      return model.error?.message ?? "Capture failed";
  }
}

function capturedText(capture: CaptureInfo): string {
  const size = capture.width && capture.height ? ` · ${capture.width}×${capture.height}` : "";
  return `Screenshot captured ${formatClock(capture.capturedAt)}${size}`;
}

/** Short, user-facing message for a typed capture failure. */
export function captureFailureMessage(reason: CaptureFailure | "other", fallback: string): string {
  switch (reason) {
    case "windowNotFound":
      return "Star Citizen window not found";
    case "minimized":
      return "Star Citizen is minimised";
    case "busy":
      return "Another capture is running";
    case "timeout":
      return "The capture took too long";
    case "unsupported":
      return "Screen capture is not supported here";
    default:
      return fallback || "Capture failed";
  }
}
