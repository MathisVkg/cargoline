// Frontend unit tests: Settings group order, Data texts (backup name, export notice,
// restore confirmation with counts, refusals), About texts and data credits, Updates hint.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  ABOUT_DESCRIPTION,
  BACKUP_FILTER,
  DATA_CREDIT,
  DATA_TERMS,
  LICENCE_TEXT,
  REFUSALS,
  REPOSITORY_LABEL,
  RESTORE_QUESTION,
  SCREENSHOTS_NOT_INCLUDED,
  SETTINGS_DESCRIPTION,
  SETTINGS_GROUPS,
  UNOFFICIAL,
  backupContents,
  backupFileName,
  catalogCreditLine,
  exportNotice,
  formatBytes,
  noticeBlocks,
  refusalMessage,
  shortenPath,
} from "../../src/features/data/model.ts";

const repo = join(import.meta.dirname, "..", "..");

test("Settings groups and description", () => {
  assert.deepEqual(
    SETTINGS_GROUPS.map((g) => `${g.index} ${g.title}`),
    ["01 Game", "02 OCR Capture", "03 Updates", "04 Data", "05 About"],
  );
  assert.ok(!/placeholder/i.test(SETTINGS_DESCRIPTION), "no longer says settings are placeholders");
  // The page source uses the same order. (Updates replaced its disabled placeholder.)
  const page = readFileSync(join(repo, "src", "pages", "SettingsPage.tsx"), "utf8");
  const order = ["<GameSection", "<OcrSection", "<UpdatesSection", "<DataSection", "<AboutSection"].map((t) => page.indexOf(t));
  assert.ok(order.every((i, k) => i > 0 && (k === 0 || i > order[k - 1])), `section order ${order}`);
  assert.ok(!page.includes("<Tag>Pending</Tag>"), "no placeholder leftovers");
});

test("export texts", () => {
  assert.equal(backupFileName(new Date(2026, 10, 26, 23, 59)), "cargo-line-backup-2026-11-26.sqlite3", "local date");
  assert.equal(backupFileName(new Date(2027, 0, 5)), "cargo-line-backup-2027-01-05.sqlite3");
  assert.deepEqual(BACKUP_FILTER, { name: "Cargo Line backup (*.sqlite3)", extensions: ["sqlite3"] });
  assert.equal(SCREENSHOTS_NOT_INCLUDED, "Screenshots are not included.");
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(348_160), "340 KB");
  assert.equal(formatBytes(1_258_291), "1.2 MB");
  assert.equal(exportNotice("cargo-line-backup-2026-11-26.sqlite3", 348_160), "Backup saved: cargo-line-backup-2026-11-26.sqlite3 (340 KB).");
});

test("data folder path is shortened, full path kept for the tooltip", () => {
  const full = "C:\\Users\\Someone\\AppData\\Roaming\\io.github.mathisvkg.sccargoapp";
  assert.equal(shortenPath(full), "C:\\…\\io.github.mathisvkg.sccargoapp");
  assert.equal(shortenPath("C:\\Short\\Path"), "C:\\Short\\Path");
});

test("restore confirmation with counts, and refusals", () => {
  assert.equal(
    RESTORE_QUESTION,
    "Replace all current data with this backup? Your current data is saved as a safety copy first. Cargo Line restarts to finish.",
  );
  assert.equal(
    backupContents({ schemaVersion: 6, activeContracts: 3, finishedContracts: 41, runs: 12, bytes: 1 }),
    "This backup contains 3 active contracts, 41 finished contracts and 12 runs.",
  );
  assert.equal(
    backupContents({ schemaVersion: 4, activeContracts: 1, finishedContracts: 0, runs: 1, bytes: 1 }),
    "This backup contains 1 active contract, 0 finished contracts and 1 run.",
  );
  assert.equal(refusalMessage({ kind: "backup", reason: "tooNew", message: "x" }), "This backup was made by a newer version of Cargo Line.");
  assert.equal(refusalMessage({ kind: "backup", reason: "notSqlite" }), REFUSALS.notSqlite);
  assert.equal(refusalMessage({ kind: "backup", reason: "notCargoLine" }), "This file is not a Cargo Line backup (it belongs to another program).");
  assert.equal(refusalMessage({ kind: "backup", reason: "corrupted", message: "This backup is damaged and cannot be used (page 3)." }), "This backup is damaged and cannot be used (page 3).");
  assert.equal(refusalMessage({ kind: "backup", reason: "sameAsLive" }), "Choose a file other than the database Cargo Line is using.");
  assert.equal(refusalMessage({ kind: "database", message: "disk full" }), "disk full");
});

test("About texts and data credits from the catalog headers", () => {
  assert.equal(LICENCE_TEXT, "Free software under the GNU General Public License v3.0");
  assert.equal(REPOSITORY_LABEL, "github.com/MathisVkg/cargoline");
  assert.ok(ABOUT_DESCRIPTION.length > 10 && !ABOUT_DESCRIPTION.includes("\n"), "one line");
  assert.equal(DATA_CREDIT, "Ship, location and commodity data from the Star Citizen Wiki API (api.star-citizen.wiki).");
  assert.equal(DATA_TERMS, "Used under its terms; non-commercial.");
  assert.equal(
    UNOFFICIAL,
    "Cargo Line is an unofficial Star Citizen fan tool, not affiliated with the Cloud Imperium group of companies. Star Citizen® is a registered trademark of Cloud Imperium Rights LLC. All game content belongs to its respective owners.",
  );
  for (const [name, file] of [["Ships", "ships.json"], ["Locations", "locations.json"], ["Commodities", "commodities.json"]] as const) {
    const header = JSON.parse(readFileSync(join(repo, "src-tauri", "resources", "catalog", file), "utf8"));
    assert.equal(
      catalogCreditLine({ name, gameVersion: header.game_version, generatedAt: header.generated_at }),
      `${name}: game ${header.game_version}, generated ${header.generated_at}`,
    );
  }
});

test("third-party notices render as headings, rules and text blocks", () => {
  const blocks = noticeBlocks(readFileSync(join(repo, "THIRD_PARTY_NOTICES.md"), "utf8"));
  assert.ok(blocks.some((b) => b.kind === "heading" && b.text === "Third-party notices"));
  assert.ok(blocks.some((b) => b.kind === "heading" && /Tesseract OCR/.test(b.text)));
  assert.ok(blocks.some((b) => b.kind === "text" && /Tesseract OCR\s+·\s+5\.5\.3\s+·\s+Apache-2\.0/.test(b.text)), "table rows become plain lines");
  assert.ok(blocks.some((b) => b.kind === "text" && /Apache License/.test(b.text)));
  assert.ok(blocks.some((b) => b.kind === "rule"));
  assert.deepEqual(noticeBlocks("# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\ntext"), [
    { kind: "heading", level: 1, text: "T" },
    { kind: "text", text: "a  ·  b\n1  ·  2" },
    { kind: "rule" },
    { kind: "text", text: "text" },
  ]);
});
