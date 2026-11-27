// Frontend unit tests: the narrow-window banner, the Game.log cross-check with an
// interstellar objective (system name), and the abandon end state through the notice flow.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  NARROW_WINDOW_BANNER,
  crossCheckFlags,
  draftFromParsed,
  flagKeys,
  GAME_LOG_MISMATCH,
  narrowWindowBanner,
} from "../../src/features/capture/prefill.ts";
import type { ParsedContract } from "../../src/features/capture/types.ts";
import { initialFlow, reduceFlow, type FlowAction } from "../../src/features/gamelog/flow.ts";
import { noticeView } from "../../src/features/gamelog/notices.ts";
import type { ContractDetail } from "../../src/features/contracts/types.ts";

test("narrow-window banner: below 16:10 only", () => {
  assert.equal(NARROW_WINDOW_BANNER, "Your game window is narrower than 16:9; part of the contract screen may be cut off. Check every field.");
  const at = (width: number, height: number) => narrowWindowBanner({ width, height });
  assert.equal(at(3840, 2160), null, "16:9");
  assert.equal(at(1924, 1081), null, "16:9 window with borders");
  assert.equal(at(2560, 1600), null, "16:10 is the limit and passes");
  assert.equal(at(3440, 1440), null, "21:9");
  assert.equal(at(1600, 1067), NARROW_WINDOW_BANNER, "3:2");
  assert.equal(at(1600, 1200), NARROW_WINDOW_BANNER, "4:3");
  assert.equal(at(1280, 1024), NARROW_WINDOW_BANNER, "5:4");
  assert.equal(narrowWindowBanner(null), null, "no capture");
  assert.equal(narrowWindowBanner({ width: null, height: null }), null, "unknown size");
  assert.equal(narrowWindowBanner({ width: 0, height: 0 }), null);
});

const parsed: ParsedContract = {
  screen: "accepted",
  title: { value: "Ling Family Rookie Cargo Haul - Small Scale", flag: "ok", reason: null },
  reward: { value: 476500, flag: "ok", reason: null },
  requirements: [
    {
      commodity: { value: "Souvenirs", flag: "ok", reason: null, resolution: "exact", read: "Souvenirs" },
      totalScu: { value: 2, flag: "ok", reason: null },
      pickups: [{ location: { value: "HDMS-Thedus", flag: "ok", reason: null, resolution: "exact", read: "" }, scu: { value: 2, flag: "ok", reason: null } }],
      deliveries: [{ location: { value: "Shepherd's Rest", flag: "ok", reason: null, resolution: "exact", read: "" }, scu: { value: 2, flag: "ok", reason: null } }],
    },
  ],
  problems: [],
  maxBoxScu: { value: null, flag: "ok", reason: null },
};

test("cross-check: a system name in the log is not compared with the destination", () => {
  const { draft } = draftFromParsed(parsed);
  const first = draft.requirements[0];
  const location = flagKeys.location(first.deliveries[0].key);
  // The false "Differs from game log" on Shepherd's Rest.
  const interstellar = { scu: 2, commodity: "Souvenirs", location: "Pyro System", locationIsSystem: true };
  assert.equal(crossCheckFlags(draft, interstellar).size, 0);
  // SCU and commodity are still compared.
  const differ = crossCheckFlags(draft, { ...interstellar, scu: 3, commodity: "Stims" });
  assert.equal(differ.get(flagKeys.total(first.key)), GAME_LOG_MISMATCH);
  assert.equal(differ.get(flagKeys.commodity(first.key)), GAME_LOG_MISMATCH);
  assert.equal(differ.has(location), false);
  // A real destination is still compared (older events have no flag at all).
  assert.equal(crossCheckFlags(draft, { scu: 2, commodity: "Souvenirs", location: "Ruin Station" }).get(location), GAME_LOG_MISMATCH);
  assert.equal(crossCheckFlags(draft, { scu: 2, commodity: "Souvenirs", location: "Ruin Station", locationIsSystem: false }).get(location), GAME_LOG_MISMATCH);
});

const contract = (id: number) => ({ id }) as ContractDetail;
const ended = (state: string, mode: "off" | "ask" | "automatic"): FlowAction => ({
  type: "gameLog",
  event: { kind: "ended", missionId: "6a1b2c3d-2222-4aaa-8bbb-0123456789ab", state, at: null, contractId: 6, completeFromGame: mode },
});

test("an abandon from EndMission is the 'ended in game (abandoned)' notice, never automatic", () => {
  for (const mode of ["off", "ask", "automatic"] as const) {
    let s = reduceFlow(initialFlow, ended("MISSION_STATE_ABANDONED", mode));
    s = reduceFlow(s, { type: "completionDue" });
    assert.equal(s.completion.running, null, `${mode}: nothing finalised automatically`);
    assert.equal(s.completion.dialog, null, `${mode}: no Ask dialog`);
    assert.equal(s.notices.length, 1, mode);
    const view = noticeView(s.notices[0], [contract(6)]);
    assert.deepEqual(view, { text: "C06 ended in game (abandoned)", action: "abandon", label: "Abandon contract" }, mode);
  }
  // Another completion type passes through by name.
  const other = reduceFlow(initialFlow, ended("Fail", "automatic"));
  assert.equal(noticeView(other.notices[0], [contract(6)])?.text, "C06 ended in game (fail)");
});
