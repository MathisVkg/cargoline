// Frontend unit tests: the ship selector model, the capacity label, and
// "Complete contracts finished in game" (batching, Ask dialog, automatic + Undo).
import { test } from "node:test";
import assert from "node:assert/strict";
import { catalogLine, fold, matchesQuery, shipLabel, shipOptions, withFavorite } from "../../src/features/ships/model.ts";
import type { ShipEntry } from "../../src/features/ships/types.ts";
import { capacityState, formatPercent } from "../../src/features/contracts/summary.ts";
import {
  completionText,
  initialFlow,
  reduceFlow,
  type FlowAction,
  type FlowState,
} from "../../src/features/gamelog/flow.ts";
import { askRows, autoIds, bannerText } from "../../src/features/gamelog/completion.ts";
import { noticeView } from "../../src/features/gamelog/notices.ts";
import type { AcceptedEvent, CompleteFromGame, GameLogEvent } from "../../src/features/gamelog/types.ts";
import type { ContractDetail } from "../../src/features/contracts/types.ts";

/* ------------------------------------------------------------------ ships */

const ship = (id: string, name: string, manufacturer: string, code: string, cargoScu: number, isFavorite = false): ShipEntry => ({
  id,
  name,
  manufacturer,
  manufacturerCode: code,
  cargoScu,
  maxContainerScu: 32,
  isFavorite,
});
const SHIPS: ShipEntry[] = [
  ship("CRUS_Starlifter_C2", "C2 Hercules Starlifter", "Crusader Industries", "CRUS", 696),
  ship("CRUS_Starlifter_M2", "M2 Hercules Starlifter", "Crusader Industries", "CRUS", 522, true),
  ship("DRAK_Caterpillar", "Caterpillar", "Drake Interplanetary", "DRAK", 576),
  ship("MISC_Hull_C", "Hull C", "Musashi Industrial & Starflight Concern", "MISC", 4608),
  ship("ANVL_Carrack", "Carrack", "Anvil Aerospace", "ANVL", 456, true),
];

test("search matches name, manufacturer and code, every word, case and accents ignored", () => {
  assert.equal(fold("  Crusäder   INDUSTRIES "), "crusader industries");
  assert.deepEqual(shipOptions(SHIPS, "hercules").others.map((s) => s.id), ["CRUS_Starlifter_C2"]);
  assert.deepEqual(shipOptions(SHIPS, "hercules").favorites.map((s) => s.id), ["CRUS_Starlifter_M2"]);
  assert.ok(matchesQuery(SHIPS[2], "drake"));
  assert.ok(matchesQuery(SHIPS[2], "DRAK cat"));
  assert.ok(matchesQuery(SHIPS[3], "misc hull"));
  assert.ok(!matchesQuery(SHIPS[3], "misc carrack"));
  assert.equal(shipOptions(SHIPS, "zzz").favorites.length + shipOptions(SHIPS, "zzz").others.length, 0);
});

test("favourites come first, each group alphabetical; an empty query lists everything", () => {
  const { favorites, others } = shipOptions(SHIPS, "");
  assert.deepEqual(favorites.map((s) => s.name), ["Carrack", "M2 Hercules Starlifter"]);
  assert.deepEqual(others.map((s) => s.name), ["C2 Hercules Starlifter", "Caterpillar", "Hull C"]);
  // Starring moves a ship to Favourites without selecting anything (the list is all that changes).
  const starred = withFavorite(SHIPS, "MISC_Hull_C", true);
  assert.deepEqual(shipOptions(starred, "").favorites.map((s) => s.name), ["Carrack", "Hull C", "M2 Hercules Starlifter"]);
  assert.equal(SHIPS[3].isFavorite, false, "not mutated");
});

test("capacity label and amber over 100 %", () => {
  assert.equal(shipLabel(SHIPS[0]), "C2 Hercules Starlifter · 696 SCU");
  assert.equal(shipLabel(SHIPS[3]), "Hull C · 4,608 SCU");
  const half = capacityState(348_000, 696);
  assert.deepEqual(half, { kind: "known", percent: 50, fill: 0.5, over: false });
  const over = capacityState(800_000, 696);
  assert.equal(over.kind === "known" && over.over, true, "over 100 % is flagged (shown amber), never blocked");
  assert.equal(over.kind === "known" && over.fill, 1);
  assert.equal(formatPercent(over.kind === "known" ? over.percent : 0), "115 %");
  assert.deepEqual(capacityState(1000, null), { kind: "unknown" }, "No ship: neutral");
});

test("Settings catalog line", () => {
  assert.equal(
    catalogLine({ count: 100, gameVersion: "4.10.3-LIVE.13165946", generatedAt: "2026-11-20", source: "Star Citizen Wiki API (api.star-citizen.wiki)" }),
    "100 ships · game 4.10.3-LIVE.13165946 · Star Citizen Wiki",
  );
  assert.equal(catalogLine({ count: 1, gameVersion: "v", generatedAt: "d", source: "Other" }), "1 ship · game v · Other");
});

/* -------------------------------------------------- complete from game */

const mission = (n: number) => `0000000${n}-0000-4000-8000-00000000000${n}`;
const ended = (n: number, mode: CompleteFromGame, state = "MISSION_STATE_COMPLETED"): FlowAction => ({
  type: "gameLog",
  event: { kind: "ended", missionId: mission(n), state, at: null, contractId: n, completeFromGame: mode },
});
const accepted = (n: number): FlowAction => ({
  type: "gameLog",
  event: { kind: "accepted", missionId: mission(n), title: `Haul ${n}`, at: null, objective: null, autoCapture: true } as AcceptedEvent,
});
const run = (...actions: FlowAction[]): FlowState => actions.reduce(reduceFlow, initialFlow);
const due: FlowAction = { type: "completionDue" };
const contract = (id: number): ContractDetail =>
  ({ id, title: `Haul ${id}`, rewardAuec: 1000 * id, totalScuMilli: 10_000 * id, status: "active" }) as ContractDetail;

test("setting off (or a payload without the field): the accepted-contract notice, nothing automatic", () => {
  const s = run(ended(1, "off"));
  assert.equal(s.notices.length, 1);
  assert.equal(s.completion.waiting.length, 0);
  const legacy = run({
    type: "gameLog",
    event: { kind: "ended", missionId: mission(2), state: "MISSION_STATE_COMPLETED", at: null, contractId: 2 } as GameLogEvent,
  });
  assert.equal(legacy.notices.length, 1, "missing completeFromGame is treated as off");
});

test("only MISSION_STATE_COMPLETED acts; other end states stay notices (never abandoned automatically)", () => {
  const s = run(ended(1, "automatic", "MISSION_STATE_ABANDONED"), ended(2, "ask", "MISSION_STATE_FAILED"), due);
  assert.equal(s.completion.running, null);
  assert.equal(s.completion.dialog, null);
  assert.deepEqual(s.notices.map((n) => n.kind === "ended" && n.state), ["MISSION_STATE_ABANDONED", "MISSION_STATE_FAILED"]);
  assert.equal(noticeView(s.notices[0], [contract(1)])?.action, "abandon", "the user decides");
});

test("Ask: completions within the window batch into one dialog; each MissionId acts once", () => {
  let s = run(ended(1, "ask"), ended(2, "ask"), ended(2, "ask"), ended(3, "ask"));
  assert.equal(s.completion.dialog, null, "nothing opens before the window ends");
  assert.deepEqual(s.completion.waiting.map((w) => w.contractId), [1, 2, 3]);
  s = reduceFlow(s, due);
  assert.deepEqual(s.completion.dialog?.map((d) => d.contractId), [1, 2, 3]);
  assert.equal(s.completion.waiting.length, 0);
  // A later completion is added to the open dialog; a repeat of an acted MissionId is ignored.
  s = reduceFlow(reduceFlow(s, ended(4, "ask")), ended(1, "ask"));
  assert.deepEqual(s.completion.dialog?.map((d) => d.contractId), [1, 2, 3, 4]);
  // Confirm with 3 unticked: 1, 2, 4 completed (banner, no Undo); 3 becomes the normal notice.
  s = reduceFlow(s, { type: "completionDialogDone", completed: [1, 2, 4], now: 10 });
  assert.equal(s.completion.dialog, null);
  assert.deepEqual(s.banner, { id: 10, contractIds: [1, 2, 4], undo: false });
  assert.deepEqual(s.notices.map((n) => n.id), [`ended:${mission(3)}`]);
  assert.equal(s.listVersion, 1);
  assert.equal(bannerText(s.banner!), "3 contracts completed from game");
});

test("Ask: Cancel returns every item to the normal hints", () => {
  let s = run(ended(1, "ask"), ended(2, "ask"), due);
  s = reduceFlow(s, { type: "completionDialogDone", completed: [], now: 1 });
  assert.equal(s.banner, null);
  assert.equal(s.listVersion, 0);
  assert.deepEqual(s.notices.map((n) => n.kind === "ended" && n.contractId), [1, 2]);
  assert.equal(noticeView(s.notices[0], [contract(1)])?.label, "Mark complete");
  // A notice the user dismissed earlier is not brought back.
  let d = run(ended(5, "off"), { type: "dismiss", noticeId: `ended:${mission(5)}` }, ended(6, "ask"), due);
  d = reduceFlow(d, { type: "completionDialogDone", completed: [], now: 1 });
  assert.deepEqual(d.notices.map((n) => n.kind === "ended" && n.contractId), [6]);
});

test("Ask waits while another modal is open (Add Contract or a confirmation)", () => {
  let s = run({ type: "openAdd" }, ended(1, "ask"), due);
  assert.equal(s.completion.dialog, null);
  assert.equal(s.completion.waiting.length, 1, "kept until the modal closes");
  s = reduceFlow(reduceFlow(s, { type: "closed" }), due);
  assert.deepEqual(s.completion.dialog?.map((d) => d.contractId), [1]);

  let b = run({ type: "blocking", open: true }, ended(2, "ask"), due);
  assert.equal(b.completion.dialog, null);
  b = reduceFlow(reduceFlow(b, { type: "blocking", open: false }), due);
  assert.deepEqual(b.completion.dialog?.map((d) => d.contractId), [2]);
});

test("the Ask dialog and Add Contract never stack: accepts queue, then open after the dialog", () => {
  let s = run(ended(1, "ask"), due, accepted(7));
  assert.equal(s.modal.open, false);
  assert.equal(s.queue.length, 1);
  assert.equal(reduceFlow(s, { type: "shortcut" }).modal.open, false, "the shortcut waits too");
  s = reduceFlow(s, { type: "completionDialogDone", completed: [1], now: 2 });
  assert.equal(s.modal.open, true);
  assert.equal(s.modal.event?.missionId, mission(7));
});

test("Automatic: batch, finalise, banner with Undo; a stale result is ignored; failures become hints", () => {
  let s = run(ended(1, "automatic"), ended(2, "automatic"), ended(3, "automatic"), due);
  const batch = s.completion.running!.batch;
  assert.deepEqual(s.completion.running?.items.map((i) => i.contractId), [1, 2, 3]);
  // More completions while running wait for the next run.
  s = reduceFlow(reduceFlow(s, ended(4, "automatic")), due);
  assert.equal(s.completion.running?.batch, batch);
  assert.deepEqual(s.completion.waiting.map((w) => w.contractId), [4]);
  assert.equal(reduceFlow(s, { type: "autoCompleted", batch: batch + 99, completed: [1], failed: [], now: 5 }), s, "stale batch");
  s = reduceFlow(s, { type: "autoCompleted", batch, completed: [1, 2, 3], failed: [], now: 5 });
  assert.deepEqual(s.banner, { id: 5, contractIds: [1, 2, 3], undo: true });
  assert.equal(s.completion.running, null);
  assert.equal(s.listVersion, 1);
  // Undo succeeded: the banner closes and the list reloads.
  const undone = reduceFlow(s, { type: "undone", id: 5 });
  assert.equal(undone.banner, null);
  assert.equal(undone.listVersion, 2);
  assert.equal(reduceFlow(s, { type: "undone", id: 4 }), s, "another banner's Undo does nothing");
  // The next run starts with what waited; a failure leaves the user the normal notice.
  s = reduceFlow(s, due);
  const next = s.completion.running!;
  s = reduceFlow(s, { type: "autoCompleted", batch: next.batch, completed: [], failed: next.items, now: 6 });
  assert.deepEqual(s.notices.map((n) => n.kind === "ended" && n.contractId), [4]);
  assert.equal(s.banner?.id, 5, "the earlier banner stays");
  assert.equal(reduceFlow(s, { type: "bannerDismiss", id: 5 }).banner, null);
});

test("texts and the rows the dialog / the automatic run use (active contracts only, once)", () => {
  assert.equal(completionText(["C06"]), "C06 completed from game");
  assert.equal(completionText(["C01", "C02", "C03"]), "3 contracts completed from game");
  assert.equal(bannerText({ id: 1, contractIds: [6], undo: true }), "C06 completed from game");
  const items = [1, 2, 2, 9].map((id) => ({ missionId: mission(id), contractId: id, mode: "ask" as const }));
  const active = [contract(2), contract(1), contract(3)];
  assert.deepEqual(askRows(items, active).map((c) => c.id), [1, 2], "9 is no longer active; 2 once");
  assert.deepEqual(autoIds(items, active), [1, 2]);
});
