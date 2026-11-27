// Frontend tests for the welcome guide: when it opens, how every way of closing marks it as seen,
// "Show the welcome guide", and that it never stacks with another modal.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { initialFlow, reduceFlow, type FlowAction, type FlowState } from "../../src/features/gamelog/flow.ts";
import type { AcceptedEvent, CompleteFromGame } from "../../src/features/gamelog/types.ts";
import {
  SHOW_WELCOME_LABEL,
  WELCOME_INTRO,
  WELCOME_PRIMARY,
  WELCOME_SECONDARY,
  WELCOME_STEPS,
  WELCOME_TITLE,
  welcomeOnStartup,
} from "../../src/features/welcome/model.ts";

const repo = join(import.meta.dirname, "..", "..");
const read = (...parts: string[]) => readFileSync(join(repo, ...parts), "utf8").replace(/\r\n/g, "\n");

const mission = (n: number) => `0000000${n}-0000-4000-8000-00000000000${n}`;
const accepted = (n: number): FlowAction => ({
  type: "gameLog",
  event: { kind: "accepted", missionId: mission(n), title: `Haul ${n}`, at: null, objective: null, autoCapture: true } as AcceptedEvent,
});
const ended = (n: number, mode: CompleteFromGame): FlowAction => ({
  type: "gameLog",
  event: { kind: "ended", missionId: mission(n), state: "MISSION_STATE_COMPLETED", at: null, contractId: n, completeFromGame: mode },
});
const run = (...actions: FlowAction[]): FlowState => actions.reduce(reduceFlow, initialFlow);
const welcome: FlowAction = { type: "welcome" };
const closed: FlowAction = { type: "closed" };

test("the guide's text: a title, one line, five short steps, two buttons", () => {
  assert.equal(WELCOME_TITLE, "Welcome to Cargo Line");
  assert.match(WELCOME_INTRO, /hauling contracts, offline, on your PC/);
  assert.equal(WELCOME_STEPS.length, 5);
  assert.ok(WELCOME_STEPS.every((s) => s.length <= 80), "each a short line");
  assert.match(WELCOME_STEPS[0], /ship/);
  assert.match(WELCOME_STEPS[1], /Game\.log.*capture shortcut/);
  assert.match(WELCOME_STEPS[2], /screenshot.*by hand/);
  assert.match(WELCOME_STEPS[3], /Pick Up and Turn In.*Cargo/);
  assert.match(WELCOME_STEPS[4], /History/);
  assert.equal(WELCOME_PRIMARY, "Got it");
  assert.equal(WELCOME_SECONDARY, "Open Settings");
});

test("it opens at startup while welcome_seen is false, and never again once seen", () => {
  assert.equal(welcomeOnStartup({ welcome_seen: false }), true);
  assert.equal(welcomeOnStartup({}), true, "a backend without the key: show it");
  assert.equal(welcomeOnStartup({ welcome_seen: true }), false, "after a restart it stays closed");
  assert.equal(welcomeOnStartup(null), false, "settings not loaded: nothing opens");
  const s = run(welcome);
  assert.equal(s.modal.open, true);
  assert.equal(s.modal.kind, "welcome");
});

test("every way of closing counts as seen: it is marked when it opens", () => {
  const app = read("src", "App.tsx");
  // The startup settings call opens it and marks it seen right away (no new startup call).
  assert.match(app, /if \(first && welcomeOnStartup\(s\)\) \{\s*dispatch\(\{ type: "welcome" \}\);\s*markWelcomeSeen\(\)/);
  assert.equal((app.match(/getSettings\(\)/g) ?? []).length, 1, "the existing settings call only");
  const guide = read("src", "features", "welcome", "WelcomeGuide.tsx");
  // Esc (the dialog's cancel) and both buttons close it.
  assert.match(guide, /onCancel=\{\(e\) => \{\s*e\.preventDefault\(\);\s*onClose\(\);/);
  assert.match(guide, /onClick=\{onClose\}/);
  assert.match(guide, /onClick=\{onOpenSettings\}/);
  // Focus starts on "Got it".
  assert.match(guide, /ref=\{primaryRef\}[\s\S]*\{WELCOME_PRIMARY\}/);
  assert.match(guide, /primaryRef\.current\?\.focus\(\)/);
  // Open Settings closes it and goes to Settings.
  assert.match(app, /onOpenSettings=\{\(\) => \{\s*dispatch\(\{ type: "closed" \}\);\s*navigate\("settings"\);/);
  assert.equal(run(welcome, closed).modal.open, false);
});

test("Show the welcome guide opens it without changing the setting", () => {
  assert.equal(SHOW_WELCOME_LABEL, "Show the welcome guide");
  const page = read("src", "pages", "SettingsPage.tsx");
  assert.match(page, /<SmallButton onClick=\{requestWelcome\}>\{SHOW_WELCOME_LABEL\}<\/SmallButton>/);
  const app = read("src", "App.tsx");
  const listener = app.slice(app.indexOf("const show = () =>"), app.indexOf("removeEventListener(SHOW_WELCOME_EVENT"));
  assert.match(listener, /dispatch\(\{ type: "welcome" \}\)/);
  assert.doesNotMatch(listener, /markWelcomeSeen/, "the setting is left alone");
});

test("a Game.log accept waits for the guide, then opens", () => {
  let s = run(welcome, accepted(1));
  assert.equal(s.modal.kind, "welcome", "never two modals");
  assert.equal(s.queue.length, 1);
  s = reduceFlow(s, closed);
  assert.equal(s.modal.kind, "add");
  assert.equal(s.modal.event?.missionId, mission(1));
});

test("the guide waits for an open modal, and opens before queued accepts", () => {
  let s = run({ type: "openAdd" }, welcome, accepted(1));
  assert.equal(s.modal.kind, "add", "the open form isn't replaced");
  assert.equal(s.welcomePending, true);
  s = reduceFlow(s, closed);
  assert.equal(s.modal.kind, "welcome", "the guide first");
  assert.equal(s.welcomePending, false);
  s = reduceFlow(s, closed);
  assert.equal(s.modal.kind, "add", "then the queued accept");
});

test("the Ask dialog waits for the guide", () => {
  let s = run(welcome, ended(1, "ask"), { type: "completionDue" });
  assert.equal(s.completion.dialog, null, "no Ask dialog over the guide");
  assert.equal(s.completion.waiting.length, 1);
  s = reduceFlow(reduceFlow(s, closed), { type: "completionDue" });
  assert.equal(s.completion.dialog?.length, 1, "it opens once the guide is closed");
  // And the guide waits for the Ask dialog.
  const t = reduceFlow(s, welcome);
  assert.equal(t.modal.open, false);
  assert.equal(t.welcomePending, true);
});

test("the capture shortcut closes the guide, then does its normal action", () => {
  const s = run(welcome, { type: "shortcut" });
  assert.equal(s.modal.kind, "add", "Add Contract opens");
  assert.equal(s.modal.forceCapture, true, "and captures, as the shortcut always does");
});
