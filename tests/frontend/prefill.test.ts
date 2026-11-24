// Frontend unit tests for filling Add Contract from a parsed screenshot (Task 09).
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  applyNeedsConfirm,
  clearChangedFlags,
  draftFromParsed,
  duplicateHint,
  findDuplicate,
  flagKeys,
  readingAction,
  screenBanner,
} from "../../src/features/capture/prefill.ts";
import { initialCaptureModel, reduceCapture } from "../../src/features/capture/captureModel.ts";
import { emptyDraft, toInput } from "../../src/features/contracts/draft.ts";
import type { ParsedContract, ParsedField, ParsedName, ParsedStop } from "../../src/features/capture/types.ts";
import type { ContractDetail } from "../../src/features/contracts/types.ts";

const ok = <T>(value: T): ParsedField<T> => ({ value, flag: "ok", reason: null });
const name = (value: string, flag: ParsedName["flag"] = "ok", reason: string | null = null): ParsedName => ({
  value,
  flag,
  reason,
  resolution: flag === "ok" ? "exact" : "unresolved",
  read: value,
});
const stop = (location: ParsedName, scu: ParsedField<number>): ParsedStop => ({ location, scu });

const parsed = (over: Partial<ParsedContract> = {}): ParsedContract => ({
  screen: "accepted",
  title: ok("Junior Rank - Medium Cargo Haul"),
  reward: ok(64250),
  requirements: [
    {
      commodity: name("Aluminum"),
      totalScu: ok(45),
      pickups: [stop(name("ARC-L5 Yellow Core Station"), ok(45))],
      deliveries: [stop(name("Everus Harbor"), ok(45))],
    },
    {
      commodity: name("Agricultural Supplies"),
      totalScu: { value: 11, flag: "check", reason: "Check — read with low confidence" },
      pickups: [stop(name("Port Tressler"), { value: null, flag: "ok", reason: null }), stop(name("Nowhere Outpost", "check", "Not in list"), { value: null, flag: "ok", reason: null })],
      deliveries: [stop(name("Everus Harbor"), ok(11))],
    },
  ],
  problems: [],
  maxBoxScu: { value: null, flag: "ok", reason: null },
  ...over,
});

test("a parsed contract becomes a draft with hints on flagged fields", () => {
  const { draft, flags } = draftFromParsed(parsed({ reward: { value: null, flag: "missing", reason: "Couldn't read reward" } }));
  assert.deepEqual(toInput(draft), {
    title: "Junior Rank - Medium Cargo Haul",
    rewardAuec: "",
    requirements: [
      {
        commodityName: "Aluminum",
        quantityScu: "45",
        maxBoxSizeScu: null,
        pickups: [{ locationName: "ARC-L5 Yellow Core Station", quantityScu: "45" }],
        deliveries: [{ locationName: "Everus Harbor", quantityScu: "45" }],
      },
      {
        commodityName: "Agricultural Supplies",
        quantityScu: "11",
        maxBoxSizeScu: null,
        // Several pickups: SCU unknown, left empty, no hint.
        pickups: [
          { locationName: "Port Tressler", quantityScu: null },
          { locationName: "Nowhere Outpost", quantityScu: null },
        ],
        deliveries: [{ locationName: "Everus Harbor", quantityScu: "11" }],
      },
    ],
  });
  const second = draft.requirements[1];
  assert.equal(flags.get(flagKeys.reward), "Couldn't read reward");
  assert.equal(flags.get(flagKeys.total(second.key)), "Check — read with low confidence");
  assert.equal(flags.get(flagKeys.location(second.pickups[1].key)), "Not in list");
  assert.equal(flags.has(flagKeys.stopScu(second.pickups[0].key)), false, "unknown split is not a problem");
  assert.equal(flags.has(flagKeys.title), false);
  assert.equal(flags.size, 3);
});

test("missing pieces stay empty and flagged; no objectives leaves one empty objective", () => {
  const noPickup = draftFromParsed(
    parsed({
      requirements: [{ commodity: name("Aluminum"), totalScu: ok(10), pickups: [], deliveries: [stop(name("Baijini Point"), ok(10))] }],
    }),
  );
  const pickup = noPickup.draft.requirements[0].pickups[0];
  assert.equal(pickup.locationName, "");
  assert.equal(noPickup.flags.get(flagKeys.location(pickup.key)), "No pickup line was read");
  const none = draftFromParsed(parsed({ requirements: [], title: { value: null, flag: "missing", reason: "Couldn't read title" } }));
  assert.equal(none.draft.requirements.length, 1);
  assert.equal(none.draft.title, "");
  assert.equal(none.flags.get(flagKeys.title), "Couldn't read title");
});

test("fill only when the form is untouched; otherwise offer Apply", () => {
  const filled = draftFromParsed(parsed()).draft;
  assert.equal(readingAction({ draft: emptyDraft(), userEdited: false }), "fill");
  assert.equal(readingAction({ draft: filled, userEdited: false }), "fill", "a recapture replaces an earlier reading");
  assert.equal(readingAction({ draft: { ...filled, title: "Mine" }, userEdited: true }), "offer");
  assert.equal(readingAction({ draft: emptyDraft(), userEdited: true }), "fill", "typed then cleared");
  assert.equal(applyNeedsConfirm(emptyDraft()), false);
  assert.equal(applyNeedsConfirm({ ...emptyDraft(), rewardAuec: "5" }), true);
});

test("editing a field clears its hint, and only its hint", () => {
  const { draft, flags } = draftFromParsed(parsed({ title: { value: "Junlor", flag: "check", reason: "Check" } }));
  const second = draft.requirements[1];
  assert.equal(clearChangedFlags(flags, draft, draft), flags, "no change keeps the same map");
  const retitled = { ...draft, title: "Junior Rank" };
  const afterTitle = clearChangedFlags(flags, draft, retitled);
  assert.equal(afterTitle.has(flagKeys.title), false);
  assert.equal(afterTitle.has(flagKeys.total(second.key)), true);
  // Fixing the location name clears that stop's hint.
  const fixedStop = {
    ...retitled,
    requirements: [
      retitled.requirements[0],
      { ...second, pickups: [second.pickups[0], { ...second.pickups[1], locationName: "Seraphim Station" }] },
    ],
  };
  const afterStop = clearChangedFlags(afterTitle, retitled, fixedStop);
  assert.equal(afterStop.has(flagKeys.location(second.pickups[1].key)), false);
  // Removing the objective removes its hints; the first objective's rows are untouched.
  const removed = { ...fixedStop, requirements: [fixedStop.requirements[0]] };
  assert.equal(clearChangedFlags(afterStop, fixedStop, removed).size, 0);
});

const contract = (id: number, title: string, reward: number, totals: number[], status = "active") =>
  ({ id, title, rewardAuec: reward, status, requirements: totals.map((t) => ({ quantityScuMilli: t * 1000 })) }) as unknown as ContractDetail;

test("possible duplicate: same title, reward and total SCU among active contracts", () => {
  const draft = draftFromParsed(parsed()).draft; // 45 + 11 = 56 SCU, 64250
  const active = [contract(2, "Other", 64250, [56]), contract(3, "junior rank -  medium cargo haul", 64250, [40, 16])];
  assert.equal(findDuplicate(draft, active)?.id, 3);
  assert.equal(duplicateHint(active[1]), "Looks like C03 — already added?");
  assert.equal(findDuplicate({ ...draft, rewardAuec: "64000" }, active), null);
  assert.equal(findDuplicate(draft, [contract(3, "Junior Rank - Medium Cargo Haul", 64250, [55])]), null);
  assert.equal(findDuplicate(draft, [contract(4, "Junior Rank - Medium Cargo Haul", 64250, [56], "completed")]), null);
  assert.equal(findDuplicate({ ...draft, rewardAuec: "" }, active), null, "incomplete forms never match");
});

test("banners per screen", () => {
  assert.equal(
    screenBanner(parsed({ screen: "offer" })),
    "Read from the Offers screen — amounts can change after accepting. Recapture from the Accepted tab.",
  );
  assert.equal(
    screenBanner(parsed({ screen: "unknown" })),
    "Couldn't confirm this is the Accepted tab — check the amounts",
    "clean reading: mild",
  );
  assert.equal(
    screenBanner(parsed({ screen: "unknown", problems: ["No objectives were read"] })),
    "Contract screen not recognised — check every field.",
    "problems: strong",
  );
  assert.equal(
    screenBanner(parsed({ screen: "unknown", reward: { value: null, flag: "missing", reason: "Couldn't read reward" } })),
    "Contract screen not recognised — check every field.",
    "reward not read: strong",
  );
  assert.equal(screenBanner(parsed({ screen: "unknown", requirements: [] })), "Contract screen not recognised — check every field.");
  assert.equal(screenBanner(parsed()), null);
});

test("the capture model carries the parsed contract with its OCR result", () => {
  const shot = { id: 5, contractId: null, capturedAt: 0, expiresAt: 1, source: "window", backend: "wgc", width: 1, height: 1 } as const;
  let m = reduceCapture(initialCaptureModel, { type: "configured", auto: false, delayMs: 0 });
  m = reduceCapture(m, { type: "start" });
  m = reduceCapture(m, { type: "succeeded", capture: shot });
  m = reduceCapture(m, { type: "ocrStart", captureId: 5 });
  const result = { width: 1, height: 1, anchors: { header: true, details: true, reward: true }, screen: "accepted", regions: [], timings: { loadMs: 0, anchorMs: 0, regionsMs: 0, totalMs: 0 } } as const;
  const reading = parsed();
  m = reduceCapture(m, { type: "ocrSucceeded", captureId: 5, result, parsed: reading });
  assert.equal(m.ocr.parsed, reading);
  // A recapture resets it.
  assert.equal(reduceCapture(m, { type: "start" }).ocr.parsed, null);
});
