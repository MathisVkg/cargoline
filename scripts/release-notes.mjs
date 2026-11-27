#!/usr/bin/env node
// Extracts the CHANGELOG.md section of one version, for the GitHub Release notes.
//
//   node scripts/release-notes.mjs <version> <output file>
//
// Fails when CHANGELOG.md has no `## [<version>]` section, or when that section is empty.
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** The body of `## [version]` (heading line excluded), up to the next `## ` heading or a link-reference block. */
export function extractSection(changelog, version) {
  const lines = changelog.replace(/\r\n/g, "\n").split("\n");
  const heading = new RegExp(`^##\\s+\\[${escape(version)}\\](\\s|$)`);
  const start = lines.findIndex((l) => heading.test(l));
  if (start < 0) throw new Error(`CHANGELOG.md has no "## [${version}]" section`);
  const body = [];
  for (const line of lines.slice(start + 1)) {
    if (/^##\s/.test(line) || /^\[[^\]]+\]:\s/.test(line)) break;
    body.push(line);
  }
  const text = body.join("\n").trim();
  if (!text) throw new Error(`the "## [${version}]" section of CHANGELOG.md is empty`);
  return text + "\n";
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const [version, out] = process.argv.slice(2);
  if (!version || !out) {
    console.error("usage: node scripts/release-notes.mjs <version> <output file>");
    process.exit(2);
  }
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  try {
    writeFileSync(out, extractSection(readFileSync(join(root, "CHANGELOG.md"), "utf8"), version));
  } catch (e) {
    console.error(`release-notes: ${e.message}`);
    process.exit(1);
  }
}
