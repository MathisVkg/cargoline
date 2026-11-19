// Pure state machine of the Add Contract screenshot preview (no React, no IPC).
import type { CaptureFailure } from "../contracts/types.ts";
import type { CaptureInfo } from "./types.ts";

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
}

export type CaptureEvent =
  | { type: "configured"; auto: boolean; delayMs: number }
  | { type: "tick"; remainingMs: number }
  | { type: "start" }
  | { type: "succeeded"; capture: CaptureInfo }
  | { type: "failed"; reason: CaptureFailure | "other"; message: string }
  | { type: "closed" };

export const initialCaptureModel: CaptureModel = {
  phase: "loading",
  remainingMs: 0,
  capture: null,
  error: null,
  closed: false,
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
      return { ...model, phase: "capturing", remainingMs: 0, error: null };
    case "succeeded":
      return model.phase === "capturing" ? { ...model, phase: "captured", capture: event.capture, error: null } : model;
    case "failed":
      // A failed recapture keeps the previous capture (still linked on save).
      return model.phase === "capturing"
        ? { ...model, phase: "error", error: { reason: event.reason, message: event.message } }
        : model;
    case "closed":
      return { ...model, closed: true };
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
