// Frontend and tooling tests for Task 19: one version number in every file, set-version's input
// check (on a temporary copy, never the real files), the CHANGELOG section extraction, the About
// version line per mode, and the release workflow's rules.
import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, mkdtempSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applyVersion, cargoPackageVersion, isPrerelease, isSemver, readVersions } from "../../scripts/set-version.mjs";
import { extractSection } from "../../scripts/release-notes.mjs";
import { aboutVersionLine } from "../../src/features/data/model.ts";

const repo = join(import.meta.dirname, "..", "..");
// Line endings normalised: a Windows checkout may have CRLF (core.autocrlf).
const read = (...parts: string[]) => readFileSync(join(repo, ...parts), "utf8").replace(/\r\n/g, "\n");

test("package.json, package-lock.json, Cargo.toml, Cargo.lock and tauri.conf.json agree", () => {
  const v = readVersions(repo);
  assert.ok(isSemver(v.packageJson), v.packageJson);
  assert.equal(v.packageLock, v.packageJson);
  assert.equal(v.packageLockRoot, v.packageJson);
  assert.equal(v.cargoToml, v.packageJson);
  assert.equal(v.cargoLock, v.packageJson);
  // tauri.conf.json reads package.json; if it ever holds its own value, it must match.
  assert.ok(v.tauriConf === "../package.json" || v.tauriConf === v.packageJson, v.tauriConf);
});

test("semver check: accepts releases and pre-releases, refuses the rest", () => {
  for (const ok of ["1.0.0", "1.0.0-beta.1", "0.9.0", "10.20.30", "1.0.0-rc.1+build.5"]) assert.ok(isSemver(ok), ok);
  for (const bad of ["v1.0.0", "1.0", "1.0.0.0", "01.0.0", "1.0.0-", "1.0.0-beta..1", "", " 1.0.0", "latest", undefined]) {
    assert.ok(!isSemver(bad as string), String(bad));
  }
  assert.ok(isPrerelease("1.0.0-beta.1"));
  assert.ok(!isPrerelease("1.0.0"));
  assert.ok(!isPrerelease("1.0.0+build.5"), "build metadata alone isn't a pre-release");
});

test("set-version writes the three files on a copy and refuses bad input", () => {
  const dir = mkdtempSync(join(tmpdir(), "cl-version-"));
  try {
    mkdirSync(join(dir, "src-tauri"));
    for (const f of ["package.json", "package-lock.json"]) cpSync(join(repo, f), join(dir, f));
    for (const f of ["Cargo.toml", "Cargo.lock", "tauri.conf.json"]) cpSync(join(repo, "src-tauri", f), join(dir, "src-tauri", f));
    const before = read("src-tauri", "Cargo.toml");

    assert.throws(() => applyVersion(dir, "v2.0.0"), /not a valid semver/);
    assert.throws(() => applyVersion(dir, "2.0"), /not a valid semver/);
    assert.equal(readVersions(dir).packageJson, readVersions(repo).packageJson, "nothing written on refusal");

    applyVersion(dir, "2.3.4-rc.1");
    const v = readVersions(dir);
    assert.equal(v.packageJson, "2.3.4-rc.1");
    assert.equal(v.packageLock, "2.3.4-rc.1");
    assert.equal(v.packageLockRoot, "2.3.4-rc.1");
    assert.equal(v.cargoToml, "2.3.4-rc.1");
    // Only the [package] version line changed: dependency versions are untouched.
    const after = readFileSync(join(dir, "src-tauri", "Cargo.toml"), "utf8").replace(/\r\n/g, "\n");
    const changed = after.split("\n").filter((l, i) => l !== before.split("\n")[i]);
    assert.deepEqual(changed, ['version = "2.3.4-rc.1"']);
    // The real files are untouched.
    assert.equal(readVersions(repo).cargoToml, readVersions(repo).packageJson);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("Cargo.toml version: the [package] table only", () => {
  const toml = '[dependencies]\nversion = "9.9.9"\n\n[package]\nname = "x"\nversion = "1.2.3"\n\n[lib]\nversion = "0"\n';
  assert.equal(cargoPackageVersion(toml), "1.2.3");
  assert.equal(cargoPackageVersion('[lib]\nversion = "1"\n'), null);
});

const CHANGELOG = `# Changelog

## [1.1.0] - 2027-01-02

### Fixed

- Something.

## [1.0.0-beta.1] - Unreleased

### Added

- **Contracts:** add them.

## [0.9.0]

[1.1.0]: https://example.invalid/compare
`;

test("changelog extraction finds the right section", () => {
  assert.equal(extractSection(CHANGELOG, "1.0.0-beta.1"), "### Added\n\n- **Contracts:** add them.\n");
  assert.equal(extractSection(CHANGELOG, "1.1.0"), "### Fixed\n\n- Something.\n");
  assert.equal(extractSection(CHANGELOG.replace(/\n/g, "\r\n"), "1.1.0"), "### Fixed\n\n- Something.\n");
});

test("changelog extraction fails on a missing or empty section", () => {
  assert.throws(() => extractSection(CHANGELOG, "2.0.0"), /no "## \[2\.0\.0\]" section/);
  assert.throws(() => extractSection(CHANGELOG, "1.0.0"), /no "## \[1\.0\.0\]"/, "1.0.0 isn't 1.0.0-beta.1");
  assert.throws(() => extractSection(CHANGELOG, "0.9.0"), /is empty/);
});

test("the real CHANGELOG has a section for the current version", () => {
  const version = readVersions(repo).packageJson;
  const section = extractSection(read("CHANGELOG.md"), version);
  assert.match(section, /^### (Added|Changed|Fixed|Removed)/m, "a Keep a Changelog subsection");
  assert.doesNotMatch(section, /Task \d+/i, "no internal task numbers");
});

test("About shows the version and the kind of copy", () => {
  assert.equal(aboutVersionLine("1.0.0-beta.1", "installed"), "Version 1.0.0-beta.1 · Installed");
  assert.equal(aboutVersionLine("1.0.0-beta.1", "portable"), "Version 1.0.0-beta.1 · Portable");
  assert.equal(aboutVersionLine("1.0.0-beta.1", "dev"), "Version 1.0.0-beta.1 · Development build");
  assert.equal(aboutVersionLine("1.0.0", undefined), "Version 1.0.0");
});

test("release workflow: pinned actions, read-only by default, draft only, never overwrites", () => {
  const yml = read(".github", "workflows", "release.yml");
  for (const m of yml.matchAll(/uses:\s*(\S+)/g)) assert.match(m[1], /@[0-9a-f]{40}$/, `${m[1]} is pinned to a commit SHA`);
  assert.match(yml, /^permissions:\n  contents: read$/m);
  assert.equal(yml.match(/contents: write/g)?.length, 1, "write access in one place only");
  assert.match(yml, /tags: \["v\*\.\*\.\*"\]/);
  assert.match(yml, /workflow_dispatch:/);
  assert.match(yml, /--draft/);
  assert.match(yml, /--prerelease/);
  assert.match(yml, /already exists/);
  assert.match(yml, /retention-days: 7/);
  assert.match(yml, /timeout-minutes: 60/);
  assert.doesNotMatch(yml, /secrets\./, "no secrets beyond the automatic token");
  assert.match(yml, /build-release\.ps1 -Tag/);
});

test("the portable files are tracked and say how to use them", () => {
  assert.match(read("scripts", "portable", "portable.txt"), /data\\ folder next to cargo-line\.exe/);
  assert.match(read("scripts", "portable", "README-portable.txt"), /Extract/);
  const conf = JSON.parse(read("src-tauri", "tauri.conf.json"));
  assert.equal(conf.bundle.windows.nsis.installMode, "currentUser");
  assert.deepEqual(conf.bundle.targets, ["nsis"]);
});
