// Frontend tests: the texts for each check result, the top-bar marker, the button
// for each mode, the release notes as plain text, and no network access from the web view.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import {
  checkResultText,
  downloadText,
  installConfirmText,
  lastCheckText,
  markerVisible,
  releaseDateText,
  updateAction,
  PORTABLE_HINT,
  UPDATE_MARKER_TEXT,
  type UpdateOffer,
  type UpdateStatus,
} from "../../src/features/update/model.ts";

const repo = join(import.meta.dirname, "..", "..");
const read = (...parts: string[]) => readFileSync(join(repo, ...parts), "utf8").replace(/\r\n/g, "\n");

const offer = (over: Partial<UpdateOffer> = {}): UpdateOffer => ({
  version: "1.0.0-beta.3",
  tag: "v1.0.0-beta.3",
  htmlUrl: "https://github.com/MathisVkg/cargoline/releases/tag/v1.0.0-beta.3",
  publishedAt: "2027-01-10T12:00:00Z",
  notes: "Fixes.",
  notesTruncated: false,
  setup: { name: "Cargo-Line_1.0.0-beta.3_x64-setup.exe", size: 7_000_000, url: "https://github.com/x" },
  sums: { name: "SHA256SUMS.txt", size: 211, url: "https://github.com/y" },
  ...over,
});

const status = (over: Partial<UpdateStatus> = {}): UpdateStatus => ({
  currentVersion: "1.0.0-beta.2",
  mode: "installed",
  inAppInstall: true,
  automatic: true,
  lastCheckMs: null,
  skippedVersion: null,
  checking: false,
  result: null,
  offer: offer(),
  marker: true,
  download: { state: "idle" },
  ...over,
});

test("the texts for each check result", () => {
  assert.equal(checkResultText({ kind: "upToDate", version: "1.0.0-beta.2" }), "You're up to date (1.0.0-beta.2)");
  assert.equal(checkResultText({ kind: "available", version: "1.0.0-beta.3" }), "Version 1.0.0-beta.3 is available");
  assert.equal(checkResultText({ kind: "failed", reason: "GitHub is busy, try later" }), "Couldn't reach GitHub: GitHub is busy, try later");
  assert.equal(checkResultText(null), null);
  assert.equal(
    installConfirmText("1.0.0-beta.3"),
    "Cargo Line will close, install version 1.0.0-beta.3, and open again. Your contracts and settings are kept.",
  );
});

test("download progress and outcome texts", () => {
  assert.equal(downloadText({ state: "idle" }), null);
  assert.equal(downloadText({ state: "downloading", received: 3_670_016, total: 7_340_032 }), "Downloading… 50 % (3.5 of 7.0 MB)");
  assert.equal(downloadText({ state: "downloading", received: 1_048_576, total: null }), "Downloading… 1.0 MB");
  assert.equal(downloadText({ state: "verifying" }), "Checking the download…");
  assert.equal(downloadText({ state: "failed", message: "The download was damaged; nothing was installed." }), "The download was damaged; nothing was installed.");
  assert.equal(lastCheckText(null), "Never checked");
  assert.match(lastCheckText(Date.UTC(2026, 10, 26, 12, 0), "en-GB"), /^Last checked 26 Nov 2026, \d\d:\d\d$/);
  assert.equal(releaseDateText("not a date"), null);
});

test("the marker shows or hides", () => {
  assert.equal(markerVisible(status()), true);
  assert.equal(markerVisible(status({ marker: false })), false, "skipped version");
  assert.equal(markerVisible(status({ offer: null, marker: false })), false, "up to date");
  assert.equal(markerVisible(null), false, "not loaded yet");
  assert.equal(UPDATE_MARKER_TEXT, "Update available");
  // A quiet line in the top bar, never a dialog.
  const bar = read("src", "components", "TopBar.tsx");
  assert.match(bar, /<UpdateMarker onOpen=\{onOpenUpdates\} \/>/);
  assert.doesNotMatch(bar, /<dialog|showModal|ConfirmDialog/);
});

test("each mode shows the right button", () => {
  assert.equal(updateAction(status()), "download", "installed: Download and install");
  assert.equal(updateAction(status({ download: { state: "downloading", received: 1, total: 2 } })), "downloading", "Cancel");
  assert.equal(updateAction(status({ download: { state: "verifying" } })), "downloading");
  assert.equal(updateAction(status({ download: { state: "ready", version: "1.0.0-beta.3" } })), "installNow", "Later kept the file");
  assert.equal(updateAction(status({ download: { state: "ready", version: "1.0.0-beta.2" } })), "download", "a kept file for another version");
  assert.equal(updateAction(status({ download: { state: "failed", message: "x" } })), "download", "try again");
  assert.equal(updateAction(status({ mode: "portable", inAppInstall: false })), "openPage");
  assert.equal(updateAction(status({ mode: "dev", inAppInstall: false })), "openPage");
  assert.equal(updateAction(status({ mode: "installed", inAppInstall: false })), "openPage", "portable.txt deleted");
  assert.equal(updateAction(status({ offer: offer({ sums: null }) })), "openPage", "no SHA256SUMS.txt: notice only");
  assert.equal(updateAction(status({ offer: offer({ setup: null }) })), "openPage");
  assert.equal(updateAction(status({ offer: null })), "none");
  assert.match(PORTABLE_HINT, /download the new zip and copy your data folder into it/);
  const page = read("src", "pages", "SettingsPage.tsx");
  for (const label of ["Download and install", "Install now", "Open download page", "Skip this version", "Check now", "Cancel"]) {
    assert.ok(page.includes(label), label);
  }
});

test("release notes are rendered as plain text", () => {
  const page = read("src", "pages", "SettingsPage.tsx");
  // The notes are a text child of a whitespace-preserving element: React escapes them, so a
  // <script> in the release body stays text.
  assert.match(page, /whitespace-pre-wrap[^>]*data-release-notes>\s*\{offer\.notes\}/);
  for (const file of sourceFiles(join(repo, "src"))) {
    assert.doesNotMatch(readFileSync(file, "utf8"), /dangerouslySetInnerHTML|innerHTML\s*=/, file);
  }
});

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sourceFiles(path);
    return /\.(ts|tsx)$/.test(name) ? [path] : [];
  });
}

test("the web view makes no network requests; the CSP is unchanged", () => {
  for (const file of sourceFiles(join(repo, "src"))) {
    assert.doesNotMatch(readFileSync(file, "utf8"), /\bfetch\(|XMLHttpRequest|new WebSocket|EventSource\(|sendBeacon/, file);
  }
  const csp = JSON.parse(read("src-tauri", "tauri.conf.json")).app.security.csp as string;
  assert.equal(
    csp,
    "default-src 'self'; img-src 'self' data: capture: http://capture.localhost; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost",
  );
});
