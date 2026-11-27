// Repository guards: no internal work-item references in tracked files, the maintainer folder is
// git-ignored, and the README keeps its player sections, the data attribution and the licence.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const repo = join(import.meta.dirname, "..", "..");
const git = (...args: string[]) => execFileSync("git", args, { cwd: repo, encoding: "utf8" });

// Written with character classes so this file doesn't match itself.
const REFERENCE = /[T]ask ?\d|[t]ask\d|[T]ASK_/;

test("no tracked file name or content refers to internal work items", () => {
  // Tracked files plus new, not-ignored ones; files deleted in the working tree are skipped.
  const files = git("ls-files", "--cached", "--others", "--exclude-standard").split("\n").filter((f) => f && existsSync(join(repo, f)));
  assert.ok(files.length > 50, `found ${files.length} files`);
  const found: string[] = [];
  for (const file of files) {
    if (REFERENCE.test(file)) found.push(`name: ${file}`);
    const text = readFileSync(join(repo, file), "latin1");
    const line = text.split("\n").findIndex((l) => REFERENCE.test(l));
    if (line >= 0) found.push(`${file}:${line + 1}`);
  }
  assert.deepEqual(found, []);
});

test("the maintainer folder .local/ is git-ignored", () => {
  assert.match(readFileSync(join(repo, ".gitignore"), "utf8"), /^\/\.local\/$/m);
  assert.equal(git("check-ignore", "--no-index", ".local/README.md").trim(), ".local/README.md");
});

test("the README is a player's guide with its credits", () => {
  const readme = readFileSync(join(repo, "README.md"), "utf8").replace(/\r\n/g, "\n");
  const headings = [...readme.matchAll(/^## (.+)$/gm)].map((m) => m[1]);
  assert.deepEqual(headings, ["Download and install", "How to use Cargo Line", "Updates", "Current status", "Credits and licence"]);
  const credits = readme.slice(readme.indexOf("## Credits and licence"));
  assert.ok(credits.trim().split("\n").length <= 10, "short: heading plus at most 8 lines");
  assert.match(credits, /GPL-3\.0-only/);
  assert.match(credits, /\[`LICENSE`\]\(LICENSE\)/);
  assert.match(credits, /THIRD_PARTY_NOTICES\.md/);
  assert.match(credits, /Tesseract \(Apache-2\.0\)/);
  assert.match(credits, /api\.star-citizen\.wiki/);
  assert.match(credits, /non-commercial, with attribution/);
  assert.match(credits, /Star Citizen® is a\s+registered trademark of Cloud Imperium Rights LLC/);
  assert.match(credits, /Unofficial fan project/);
  // The help uses the app's own labels.
  for (const label of ["Add Contract", "Detect", "Browse…", "Capture shortcut", "By Location", "Pick Up", "Turn In", "Restore to active", "Clear History", "Show the welcome guide"]) {
    assert.ok(readme.includes(label), label);
  }
  // No developer sections left.
  assert.doesNotMatch(readme, /^## (Prerequisites|Development|Architecture|Tests)\b/m);
});
