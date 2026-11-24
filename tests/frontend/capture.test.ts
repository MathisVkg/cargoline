// Frontend unit tests for the Add Contract screenshot preview.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  canCapture,
  captureFailureMessage,
  captureHeader,
  captureStatus,
  captureToDiscardOnClose,
  ocrFailureMessage,
  ocrToCancel,
  ocrToStart,
  discardLateResult,
  formatClock,
  initialCaptureModel,
  reduceCapture,
  shouldAutoCapture,
  type CaptureEvent,
  type CaptureModel,
} from "../../src/features/capture/captureModel.ts";
import type { CaptureInfo, OcrResult } from "../../src/features/capture/types.ts";

const shot = (id: number, capturedAt = new Date(2026, 10, 19, 16, 20).getTime()): CaptureInfo => ({
  id,
  contractId: null,
  capturedAt,
  expiresAt: capturedAt + 7 * 86_400_000,
  source: "window",
  backend: "wgc",
  width: 3840,
  height: 2160,
});

const run = (...events: CaptureEvent[]): CaptureModel => events.reduce(reduceCapture, initialCaptureModel);

test("edit mode never captures; add mode follows the setting", () => {
  assert.equal(shouldAutoCapture("edit", true), false);
  assert.equal(shouldAutoCapture("edit", false), false);
  assert.equal(shouldAutoCapture("add", true), true);
  assert.equal(shouldAutoCapture("add", false), false);
});

test("waiting → capturing → captured", () => {
  let m = run({ type: "configured", auto: true, delayMs: 1000 });
  assert.equal(m.phase, "waiting");
  assert.equal(captureHeader(m), "Capturing in 1.0 s…");
  assert.equal(canCapture(m), false, "no Recapture during the countdown");
  m = reduceCapture(m, { type: "tick", remainingMs: 340 });
  assert.equal(captureHeader(m), "Capturing in 0.4 s…");
  m = reduceCapture(m, { type: "start" });
  assert.equal(m.phase, "capturing");
  assert.equal(canCapture(m), false);
  m = reduceCapture(m, { type: "succeeded", capture: shot(5) });
  assert.equal(m.phase, "captured");
  assert.equal(m.capture?.id, 5);
  assert.equal(captureHeader(m), "Screenshot captured 16:20 · 3840×2160");
  assert.equal(canCapture(m), true, "Recapture once captured");
});

test("zero delay and the setting off", () => {
  assert.equal(captureHeader(run({ type: "configured", auto: true, delayMs: 0 })), "Capturing in 0.0 s…");
  const off = run({ type: "configured", auto: false, delayMs: 1000 });
  assert.equal(off.phase, "idle");
  assert.equal(captureHeader(off), "No screenshot");
  assert.equal(canCapture(off), true, "the Capture button is offered");
});

test("errors show a short message and allow Retry", () => {
  const m = run(
    { type: "configured", auto: true, delayMs: 0 },
    { type: "start" },
    { type: "failed", reason: "minimized", message: captureFailureMessage("minimized", "") },
  );
  assert.equal(m.phase, "error");
  assert.equal(captureHeader(m), "Star Citizen is minimised");
  assert.equal(canCapture(m), true);
  assert.equal(captureFailureMessage("windowNotFound", "x"), "Star Citizen window not found");
  assert.equal(captureFailureMessage("other", "Database down"), "Database down");
  assert.equal(captureFailureMessage("failed", ""), "Capture failed");
});

test("recapture replaces the capture; a failed recapture keeps the previous one", () => {
  let m = run({ type: "configured", auto: true, delayMs: 0 }, { type: "start" }, { type: "succeeded", capture: shot(1) });
  m = reduceCapture(m, { type: "start" });
  assert.equal(m.capture?.id, 1, "still linked while recapturing");
  m = reduceCapture(m, { type: "succeeded", capture: shot(2) });
  assert.equal(m.capture?.id, 2);
  m = reduceCapture(reduceCapture(m, { type: "start" }), { type: "failed", reason: "timeout", message: "slow" });
  assert.equal(m.phase, "error");
  assert.equal(m.capture?.id, 2, "the previous capture is kept and still saved with the contract");
});

test("only one capture at a time; stray results are ignored", () => {
  const capturing = run({ type: "configured", auto: false, delayMs: 0 }, { type: "start" });
  assert.equal(reduceCapture(capturing, { type: "start" }), capturing);
  const idle = run({ type: "configured", auto: false, delayMs: 0 });
  assert.equal(reduceCapture(idle, { type: "succeeded", capture: shot(9) }), idle);
  assert.equal(reduceCapture(idle, { type: "tick", remainingMs: 5 }), idle);
  // Settings arriving twice (StrictMode remount) do not restart anything.
  const waiting = run({ type: "configured", auto: true, delayMs: 800 }, { type: "tick", remainingMs: 300 });
  assert.equal(reduceCapture(waiting, { type: "configured", auto: true, delayMs: 800 }).remainingMs, 300);
});

test("closing cancels the countdown and discards late or unsaved captures", () => {
  // Closed while waiting: nothing more happens.
  const waiting = run({ type: "configured", auto: true, delayMs: 1000 });
  const closedWaiting = reduceCapture(waiting, { type: "closed" });
  assert.equal(reduceCapture(closedWaiting, { type: "start" }), closedWaiting);
  assert.equal(canCapture(closedWaiting), false);
  // Closed while capturing: the result is deleted, not shown.
  const closedCapturing = run({ type: "configured", auto: true, delayMs: 0 }, { type: "start" }, { type: "closed" });
  assert.equal(discardLateResult(closedCapturing), true);
  assert.equal(reduceCapture(closedCapturing, { type: "succeeded", capture: shot(3) }).capture, null);
  // Closed with a capture: deleted unless the contract was saved.
  const captured = run({ type: "configured", auto: true, delayMs: 0 }, { type: "start" }, { type: "succeeded", capture: shot(4) });
  assert.equal(captureToDiscardOnClose(captured, false), 4);
  assert.equal(captureToDiscardOnClose(captured, true), null);
  assert.equal(captureToDiscardOnClose(run({ type: "configured", auto: false, delayMs: 0 }), false), null);
  assert.equal(discardLateResult(captured), false);
});

test("clock formatting is local HH:MM", () => {
  assert.equal(formatClock(new Date(2026, 0, 2, 3, 4).getTime()), "03:04");
});

/* ---------- OCR status (Task 08) ---------- */

const ocrResult = (screen: OcrResult["screen"], totalMs = 940): OcrResult => ({
  width: 3840,
  height: 2160,
  anchors: { header: true, details: true, reward: true },
  screen,
  regions: [],
  timings: { loadMs: 40, anchorMs: 600, regionsMs: 300, totalMs },
});

const captured = (id = 1) =>
  run({ type: "configured", auto: true, delayMs: 0 }, { type: "start" }, { type: "succeeded", capture: shot(id) });

test("each new capture is read once, in the background", () => {
  let m = captured(7);
  assert.equal(ocrToStart(m), 7);
  assert.equal(captureStatus(m).text, "Screenshot captured 16:20 · 3840×2160");
  m = reduceCapture(m, { type: "ocrStart", captureId: 7 });
  assert.equal(ocrToStart(m), null, "not started twice");
  assert.deepEqual(captureStatus(m), { text: "Reading text…", tone: "normal", busy: true });
  assert.equal(ocrToCancel(m), 7);
  assert.equal(canCapture(m), true, "Recapture stays available while reading");
});

test("OCR results: accepted, offer, unknown", () => {
  const read = (button: OcrResult["screen"]) =>
    reduceCapture(reduceCapture(captured(), { type: "ocrStart", captureId: 1 }), {
      type: "ocrSucceeded",
      captureId: 1,
      result: ocrResult(button),
    });
  assert.deepEqual(captureStatus(read("accepted")), { text: "Text read · Accepted contract (0.9 s)", tone: "normal", busy: false });
  assert.deepEqual(captureStatus(read("offer")), {
    text: "Text read · Offer screen — amounts may change after accepting",
    tone: "warning",
    busy: false,
  });
  assert.equal(captureStatus(read("unknown")).tone, "warning");
  assert.equal(ocrToCancel(read("accepted")), null);
  assert.equal(ocrToStart(read("accepted")), null);
});

test("OCR errors allow Retry, which reads the same capture again", () => {
  let m = reduceCapture(reduceCapture(captured(), { type: "ocrStart", captureId: 1 }), {
    type: "ocrFailed",
    captureId: 1,
    reason: "engineMissing",
    message: ocrFailureMessage("engineMissing", ""),
  });
  assert.deepEqual(captureStatus(m), { text: "OCR engine not installed — enter the contract manually", tone: "error", busy: false });
  m = reduceCapture(m, { type: "ocrStart", captureId: 1 });
  assert.equal(m.ocr.status, "reading");
  assert.equal(ocrFailureMessage("noPanel", ""), "No contract panel found in the screenshot");
  assert.equal(ocrFailureMessage("timeout", ""), "Reading the text took too long");
  assert.equal(ocrFailureMessage("other", "Database down"), "Database down");
});

test("recapture restarts OCR and ignores the old capture's late result", () => {
  let m = reduceCapture(captured(1), { type: "ocrStart", captureId: 1 });
  m = reduceCapture(m, { type: "start" });
  assert.equal(m.ocr.status, "idle", "OCR reset when recapturing");
  m = reduceCapture(m, { type: "succeeded", capture: shot(2) });
  assert.equal(ocrToStart(m), 2);
  // The cancelled read of capture 1 reports late: ignored.
  assert.equal(reduceCapture(m, { type: "ocrFailed", captureId: 1, reason: "cancelled", message: "x" }), m);
  assert.equal(reduceCapture(m, { type: "ocrSucceeded", captureId: 1, result: ocrResult("accepted") }), m);
  // A failed recapture keeps the previous capture but no OCR for it is running any more.
  const failed = reduceCapture(reduceCapture(m, { type: "start" }), { type: "failed", reason: "minimized", message: "Star Citizen is minimised" });
  assert.equal(failed.capture?.id, 2);
  assert.equal(ocrToStart(failed), null);
});

test("closing stops OCR updates", () => {
  const reading = reduceCapture(captured(3), { type: "ocrStart", captureId: 3 });
  assert.equal(ocrToCancel(reading), 3, "the hook cancels this on close");
  const closed = reduceCapture(reading, { type: "closed" });
  assert.equal(reduceCapture(closed, { type: "ocrSucceeded", captureId: 3, result: ocrResult("accepted") }), closed);
  assert.equal(ocrToStart(reduceCapture(captured(4), { type: "closed" })), null);
});
