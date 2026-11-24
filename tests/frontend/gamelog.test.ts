// Frontend unit tests for Game.log events, the modal flow, notices, the shortcut and the
// cross-check (Task 10).
import { test } from "node:test";
import assert from "node:assert/strict";
import { initialFlow, queueSummary, reduceFlow, type FlowAction, type FlowState } from "../../src/features/gamelog/flow.ts";
import { noticeView, stateLabel } from "../../src/features/gamelog/notices.ts";
import { matchesShortcut, shortcutFromKey, shortcutProblem, type KeyLike } from "../../src/features/gamelog/shortcut.ts";
import { crossCheckFlags, draftFromParsed, flagKeys, GAME_LOG_MISMATCH } from "../../src/features/capture/prefill.ts";
import type { AcceptedEvent, GameLogEvent } from "../../src/features/gamelog/types.ts";
import type { ContractDetail } from "../../src/features/contracts/types.ts";
import type { ParsedContract } from "../../src/features/capture/types.ts";

const accepted = (n: number, autoCapture = true): AcceptedEvent => ({
  kind: "accepted",
  missionId: `0000000${n}-0000-4000-8000-000000000000`,
  title: `Junior Rank - Haul ${n}`,
  at: null,
  objective: null,
  autoCapture,
});
const run = (...actions: FlowAction[]): FlowState => actions.reduce(reduceFlow, initialFlow);
const log = (event: GameLogEvent): FlowAction => ({ type: "gameLog", event });

test("auto capture with the modal closed opens Add Contract attached to the event", () => {
  const s = run(log(accepted(1)));
  assert.equal(s.modal.open, true);
  assert.equal(s.modal.kind, "add");
  assert.equal(s.modal.event?.missionId, accepted(1).missionId);
  assert.equal(s.modal.auto, true, "saved as auto_capture");
  assert.equal(s.modal.forceCapture, true, "captures even if the open-capture setting is off");
});

test("events arriving while the modal is open queue in order, and the next opens on close", () => {
  let s = run({ type: "openAdd" }, log(accepted(1)), log(accepted(2)), log(accepted(3)));
  assert.equal(s.modal.event, null, "the user's form is not interrupted");
  assert.deepEqual(s.queue.map((e) => e.title), ["Junior Rank - Haul 1", "Junior Rank - Haul 2", "Junior Rank - Haul 3"]);
  assert.deepEqual(
    (({ first, more }) => [first.title, more])(queueSummary(s.queue)!),
    ["Junior Rank - Haul 1", 2],
    "Accepted in game: <title> · +2 more waiting",
  );
  const session = s.modal.session;
  s = reduceFlow(s, { type: "closed" });
  assert.equal(s.modal.open, true, "the next queued event opens");
  assert.equal(s.modal.event?.title, "Junior Rank - Haul 1");
  assert.ok(s.modal.session > session, "a fresh form");
  assert.equal(s.queue.length, 2);
  s = reduceFlow(reduceFlow(reduceFlow(s, { type: "closed" }), { type: "closed" }), { type: "closed" });
  assert.equal(s.modal.open, false);
  assert.equal(s.queue.length, 0);
  // The edit modal also holds accepts back.
  let e = run({ type: "editOpened" }, log(accepted(4)));
  assert.equal(e.modal.kind, "edit");
  assert.equal(e.queue.length, 1);
  e = reduceFlow(e, { type: "closed" });
  assert.equal(e.modal.kind, "add");
});

test("Capture now attaches a queued event to the open form and recaptures", () => {
  const s = run({ type: "openAdd" }, log(accepted(1)), log(accepted(2)), { type: "captureNow", missionId: accepted(2).missionId });
  assert.equal(s.modal.event?.title, "Junior Rank - Haul 2");
  assert.equal(s.modal.auto, true);
  assert.equal(s.recapture, 1);
  assert.deepEqual(s.queue.map((q) => q.title), ["Junior Rank - Haul 1"]);
});

test("automatic capture off: a notice, which opens the modal with the normal capture", () => {
  let s = run(log(accepted(5, false)));
  assert.equal(s.modal.open, false);
  assert.equal(s.notices.length, 1);
  assert.deepEqual(noticeView(s.notices[0], []), { text: "Accepted in game: Junior Rank - Haul 5", action: "add", label: "Add contract" });
  s = reduceFlow(s, { type: "openFromNotice", noticeId: s.notices[0].id });
  assert.equal(s.modal.event?.missionId, accepted(5).missionId, "MissionId attached");
  assert.equal(s.modal.auto, false, "user-initiated: not auto_capture");
  assert.equal(s.modal.forceCapture, false, "normal capture setting");
  assert.equal(s.notices.length, 0);
});

test("dedupe, objective pairing and saved events", () => {
  let s = run(log(accepted(1, false)), log(accepted(1, false)), log(accepted(1, true)));
  assert.equal(s.notices.length, 1);
  assert.equal(s.modal.open, false);
  const objective = { scu: 84, commodity: "Aluminum", location: "Baijini Point" };
  s = reduceFlow(s, log({ kind: "objective", missionId: accepted(1).missionId, objective }));
  assert.deepEqual(s.notices[0].kind === "accepted" && s.notices[0].event.objective, objective);
  const open = run(log(accepted(2)), log({ kind: "objective", missionId: accepted(2).missionId, objective }));
  assert.deepEqual(open.modal.event?.objective, objective, "the open form receives a late objective");
  s = reduceFlow(s, { type: "saved", missionId: accepted(1).missionId });
  assert.equal(s.notices.length, 0);
});

const contract = (id: number) => ({ id, status: "active" }) as unknown as ContractDetail;

test("ended in game: completion hint or abandon, once, only for active contracts", () => {
  const ended = (state: string, contractId = 6): GameLogEvent => ({
    kind: "ended",
    missionId: "0000000a-0000-4000-8000-000000000000",
    state,
    at: null,
    contractId,
  });
  let s = run(log(ended("MISSION_STATE_COMPLETED")), log(ended("MISSION_STATE_COMPLETED")));
  assert.equal(s.notices.length, 1, "never repeated for the same MissionId");
  assert.deepEqual(noticeView(s.notices[0], [contract(6)]), { text: "C06 completed in game", action: "complete", label: "Mark complete" });
  assert.equal(noticeView(s.notices[0], [contract(7)]), null, "contract no longer active");
  s = reduceFlow(s, { type: "dismiss", noticeId: s.notices[0].id });
  s = reduceFlow(s, log(ended("MISSION_STATE_COMPLETED")));
  assert.equal(s.notices.length, 0, "dismissed notices do not come back");
  const failed = run(log({ ...ended("MISSION_STATE_FAILED"), missionId: "0000000b-0000-4000-8000-000000000000" }));
  assert.deepEqual(noticeView(failed.notices[0], [contract(6)]), {
    text: "C06 ended in game (failed)",
    action: "abandon",
    label: "Abandon contract",
  });
  assert.equal(stateLabel("MISSION_STATE_SOMETHING_NEW"), "something new");
});

test("the shortcut: open with capture, recapture in Add, nothing in Edit", () => {
  const opened = run({ type: "shortcut" });
  assert.equal(opened.modal.kind, "add");
  assert.equal(opened.modal.forceCapture, true);
  assert.equal(reduceFlow(opened, { type: "shortcut" }).recapture, 1);
  const edit = run({ type: "editOpened" });
  assert.equal(reduceFlow(edit, { type: "shortcut" }), edit);
});

const key = (over: Partial<KeyLike>): KeyLike => ({ key: "c", code: "KeyC", ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...over });

test("shortcut recording rules", () => {
  assert.equal(shortcutFromKey(key({ ctrlKey: true, shiftKey: true, key: "C" })), "Ctrl+Shift+C");
  assert.equal(shortcutFromKey(key({ key: "a", code: "KeyQ", altKey: true })), "Alt+A", "AZERTY: the letter typed");
  assert.equal(shortcutFromKey(key({ key: "&", code: "Digit1", ctrlKey: true })), "Ctrl+1", "AZERTY digits without Shift");
  assert.equal(shortcutFromKey(key({ key: "F5", code: "F5", ctrlKey: true, altKey: true })), "Ctrl+Alt+F5");
  assert.equal(shortcutFromKey(key({ key: "Control", code: "ControlLeft", ctrlKey: true })), null, "modifier only");
  assert.equal(shortcutFromKey(key({ metaKey: true, ctrlKey: true })), null, "Windows key not supported");
  assert.equal(shortcutProblem("Ctrl+Shift+C"), null);
  assert.match(shortcutProblem("C")!, /Ctrl or Alt/);
  assert.match(shortcutProblem("Shift+C")!, /Ctrl or Alt/);
  assert.match(shortcutProblem("F5")!, /Ctrl or Alt/);
  assert.match(shortcutProblem("Ctrl+C")!, /already used/);
  assert.match(shortcutProblem("Ctrl+R")!, /already used/);
  assert.equal(matchesShortcut(key({ ctrlKey: true, shiftKey: true, key: "C" }), "Ctrl+Shift+C"), true);
  assert.equal(matchesShortcut(key({ ctrlKey: true, key: "c" }), "Ctrl+Shift+C"), false);
  assert.equal(matchesShortcut(key({ ctrlKey: true, shiftKey: true, key: "C" }), null), false, "no shortcut by default");
});

test("cross-check with the Game.log first objective flags differences, never overwrites", () => {
  const parsed: ParsedContract = {
    screen: "accepted",
    title: { value: "Junior Rank", flag: "ok", reason: null },
    reward: { value: 59500, flag: "ok", reason: null },
    requirements: [
      {
        commodity: { value: "Aluminum", flag: "ok", reason: null, resolution: "exact", read: "Aluminum" },
        totalScu: { value: 89, flag: "ok", reason: null },
        pickups: [{ location: { value: "ARC-L2 Lively Pathway Station", flag: "ok", reason: null, resolution: "exact", read: "" }, scu: { value: 89, flag: "ok", reason: null } }],
        deliveries: [{ location: { value: "Seraphim Station", flag: "ok", reason: null, resolution: "stripped", read: "" }, scu: { value: 89, flag: "ok", reason: null } }],
      },
    ],
    problems: [],
    maxBoxScu: { value: null, flag: "ok", reason: null },
  };
  const { draft } = draftFromParsed(parsed);
  const first = draft.requirements[0];
  assert.equal(crossCheckFlags(draft, { scu: 89, commodity: "aluminum", location: "Seraphim  Station" }).size, 0, "same after resolution");
  const differ = crossCheckFlags(draft, { scu: 84, commodity: "Tungsten", location: "Baijini Point" });
  assert.equal(differ.get(flagKeys.total(first.key)), GAME_LOG_MISMATCH);
  assert.equal(differ.get(flagKeys.commodity(first.key)), GAME_LOG_MISMATCH);
  assert.equal(differ.get(flagKeys.location(first.deliveries[0].key)), GAME_LOG_MISMATCH);
  assert.equal(first.quantityScu, "89", "the form keeps the screenshot's value");
  assert.equal(crossCheckFlags(draft, null).size, 0);
});
