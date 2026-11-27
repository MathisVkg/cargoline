// Frontend unit tests: the single retry, choosing between readings, max box size,
// and Detect outcomes.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  betterReading,
  captureStatus,
  initialCaptureModel,
  reduceCapture,
  shouldRetry,
  type CaptureEvent,
  type CaptureModel,
} from "../../src/features/capture/captureModel.ts";
import { clearChangedFlags, draftFromParsed, flagKeys } from "../../src/features/capture/prefill.ts";
import { choiceLabel, detectOutcome } from "../../src/features/gamelog/detect.ts";
import type { CaptureInfo, OcrResult, ParsedContract } from "../../src/features/capture/types.ts";
import type { FoundLog } from "../../src/features/gamelog/types.ts";

const shot = (id: number): CaptureInfo => ({
  id,
  contractId: null,
  capturedAt: 0,
  expiresAt: 1,
  source: "window",
  backend: "wgc",
  width: 1,
  height: 1,
});
const result: OcrResult = {
  width: 1,
  height: 1,
  anchors: { header: true, details: true, reward: true },
  screen: "unknown",
  regions: [],
  timings: { loadMs: 0, anchorMs: 0, regionsMs: 0, totalMs: 900 },
};
const reading = (over: Partial<ParsedContract> = {}): ParsedContract => ({
  screen: "unknown",
  title: { value: "Junior Rank - Direct Medium Cargo Haul", flag: "ok", reason: null },
  reward: { value: 59500, flag: "ok", reason: null },
  requirements: [
    {
      commodity: { value: "Aluminum", flag: "ok", reason: null, resolution: "exact", read: "Aluminum" },
      totalScu: { value: 93, flag: "ok", reason: null },
      pickups: [
        { location: { value: "ARC-L5 Yellow Core Station", flag: "ok", reason: null, resolution: "exact", read: "" }, scu: { value: 93, flag: "ok", reason: null } },
      ],
      deliveries: [
        { location: { value: "Port Tressler", flag: "ok", reason: null, resolution: "stripped", read: "" }, scu: { value: 93, flag: "ok", reason: null } },
      ],
    },
  ],
  problems: [],
  maxBoxScu: { value: 8, flag: "ok", reason: null },
  ...over,
});
const readModel = (parsed: ParsedContract, id = 1, base: CaptureModel = initialCaptureModel): CaptureModel => {
  const events: CaptureEvent[] = [
    { type: "configured", auto: true, delayMs: 0 },
    { type: "start" },
    { type: "succeeded", capture: shot(id) },
    { type: "ocrStart", captureId: id },
    { type: "ocrSucceeded", captureId: id, result, parsed },
  ];
  return events.reduce(reduceCapture, base);
};

test("retry once when the screen is unknown but objectives were read", () => {
  const m = readModel(reading());
  assert.equal(shouldRetry(m), true);
  const checking = reduceCapture(m, { type: "retryStart" });
  assert.equal(shouldRetry(checking), false, "never a second retry");
  assert.deepEqual(captureStatus(checking), { text: "Checking screen again…", tone: "normal", busy: true });
  // The second reading recognises the screen: it replaces the first.
  const better = reading({ screen: "accepted" });
  const replaced = reduceCapture(checking, { type: "retryFinished", better: { capture: shot(2), result, parsed: better } });
  assert.equal(replaced.capture?.id, 2);
  assert.equal(replaced.ocr.parsed, better);
  assert.equal(replaced.checkingAgain, false);
  assert.equal(shouldRetry(replaced), false);
  // The first was better: kept as is.
  const kept = reduceCapture(checking, { type: "retryFinished" });
  assert.equal(kept.capture?.id, 1);
  assert.equal(kept.checkingAgain, false);
  // A recapture by the user does not give another automatic retry.
  const again = readModel(reading(), 3, kept);
  assert.equal(shouldRetry(again), false, "only once per opening");
});

test("no retry on a recognised screen, without objectives, or after close", () => {
  assert.equal(shouldRetry(readModel(reading({ screen: "accepted" }))), false);
  assert.equal(shouldRetry(readModel(reading({ screen: "offer" }))), false);
  assert.equal(shouldRetry(readModel(reading({ requirements: [] }))), false);
  assert.equal(shouldRetry(reduceCapture(readModel(reading()), { type: "closed" })), false);
  assert.equal(shouldRetry(initialCaptureModel), false);
});

test("choosing between two readings", () => {
  assert.equal(betterReading(reading(), reading({ screen: "accepted" })), "second", "recognised screen wins");
  assert.equal(betterReading(reading({ screen: "accepted" }), reading()), "first");
  assert.equal(betterReading(reading({ problems: ["a", "b"] }), reading({ problems: ["a"] })), "second", "then fewer problems");
  assert.equal(betterReading(reading({ problems: ["a"] }), reading({ problems: ["a", "b"] })), "first");
  assert.equal(betterReading(reading(), reading()), "first", "ties keep the first");
  assert.equal(betterReading(reading({ screen: "offer", problems: ["x"] }), reading({ screen: "accepted" })), "second");
});

test("max box size fills every requirement; conflicts are flagged", () => {
  const two = reading();
  two.requirements = [two.requirements[0], { ...two.requirements[0] }];
  const { draft, flags } = draftFromParsed(two);
  assert.deepEqual(
    draft.requirements.map((r) => r.maxBoxSizeScu),
    ["8", "8"],
  );
  assert.equal(flags.size, 0);
  const none = draftFromParsed(reading({ maxBoxScu: { value: null, flag: "ok", reason: null } }));
  assert.equal(none.draft.requirements[0].maxBoxSizeScu, "");
  assert.equal(none.flags.size, 0, "none stated: empty, no flag");
  const conflict = draftFromParsed(reading({ maxBoxScu: { value: null, flag: "check", reason: "Check — several container sizes in the text" } }));
  const key = flagKeys.maxBox(conflict.draft.requirements[0].key);
  assert.equal(conflict.draft.requirements[0].maxBoxSizeScu, "");
  assert.equal(conflict.flags.get(key), "Check — several container sizes in the text");
  // Editing the field clears its hint.
  const edited = { ...conflict.draft, requirements: [{ ...conflict.draft.requirements[0], maxBoxSizeScu: "4" }] };
  assert.equal(clearChangedFlags(conflict.flags, conflict.draft, edited).has(key), false);
});

const log = (channel: string, modifiedMs: number | null = null): FoundLog => ({
  path: `C:\\Program Files\\Roberts Space Industries\\StarCitizen\\${channel}\\Game.log`,
  channel,
  modifiedMs,
  method: "launcher",
});

test("Detect outcomes are never silent", () => {
  const none = detectOutcome({ found: [], message: "Couldn't find Game.log — use Browse to select it" });
  assert.deepEqual(none, { kind: "none", message: "Couldn't find Game.log — use Browse to select it" });
  const one = detectOutcome({ found: [log("LIVE")], message: "Found Game.log (LIVE)" });
  assert.equal(one.kind, "one");
  assert.equal(one.kind === "one" && one.log.channel, "LIVE");
  const several = detectOutcome({ found: [log("LIVE"), log("PTU", Date.UTC(2026, 10, 19))], message: "Found 2 Game.log files — choose one" });
  assert.equal(several.kind, "several");
  assert.equal(several.kind === "several" && several.choices[0].channel, "LIVE", "LIVE first (ranked by the backend)");
  assert.equal(choiceLabel(log("LIVE"), true), "LIVE (recommended)");
  assert.match(choiceLabel(log("PTU", Date.UTC(2026, 10, 19)), false), /^PTU · modified /);
});
