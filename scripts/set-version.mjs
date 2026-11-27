#!/usr/bin/env node
// Sets the app version everywhere it is stored. The source of truth is package.json;
// tauri.conf.json reads it ("version": "../package.json"); Cargo needs its own copy.
//
//   node scripts/set-version.mjs 1.0.0-beta.1
//
// Writes package.json, package-lock.json (root entry) and src-tauri/Cargo.toml, then runs
// `cargo metadata --offline` so Cargo.lock follows. Refuses anything that isn't valid semver.
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

// semver.org's official pattern (no leading "v", no leading zeros, optional pre-release and build).
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

export function isSemver(version) {
  return typeof version === "string" && SEMVER.test(version);
}

/** Whether a version has a pre-release part ("1.0.0-beta.1"); GitHub then marks it a pre-release. */
export function isPrerelease(version) {
  const m = SEMVER.exec(version);
  return !!m && m[4] !== undefined;
}

/** The `[package]` version of a Cargo.toml text (first `version =` line of that table). */
export function cargoPackageVersion(toml) {
  const table = /^\[package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(toml);
  const line = table && /^version\s*=\s*"([^"]*)"/m.exec(table[1]);
  return line ? line[1] : null;
}

function setCargoVersion(toml, version) {
  const start = toml.search(/^\[package\]\s*$/m);
  if (start < 0) throw new Error("Cargo.toml has no [package] table");
  const rest = toml.slice(start);
  const next = rest.slice(1).search(/^\[/m);
  const end = next < 0 ? toml.length : start + 1 + next;
  const table = toml.slice(start, end);
  if (!/^version\s*=\s*"[^"]*"/m.test(table)) throw new Error("Cargo.toml [package] has no version");
  return toml.slice(0, start) + table.replace(/^version\s*=\s*"[^"]*"/m, `version = "${version}"`) + toml.slice(end);
}

function editJson(path, edit) {
  const text = readFileSync(path, "utf8");
  const data = JSON.parse(text);
  edit(data);
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  writeFileSync(path, JSON.stringify(data, null, 2).replace(/\n/g, eol) + eol);
}

/** Writes the version into the three files under `root`. Throws on an invalid version. */
export function applyVersion(root, version) {
  if (!isSemver(version)) {
    throw new Error(`"${version}" is not a valid semver version (expected e.g. 1.0.0 or 1.0.0-beta.1, no "v")`);
  }
  editJson(join(root, "package.json"), (p) => {
    p.version = version;
  });
  editJson(join(root, "package-lock.json"), (l) => {
    l.version = version;
    if (l.packages?.[""]) l.packages[""].version = version;
  });
  const cargo = join(root, "src-tauri", "Cargo.toml");
  writeFileSync(cargo, setCargoVersion(readFileSync(cargo, "utf8"), version));
}

/** Every version the repository stores, for the consistency check. */
export function readVersions(root) {
  const json = (...p) => JSON.parse(readFileSync(join(root, ...p), "utf8"));
  const pkg = json("package.json");
  const lock = json("package-lock.json");
  const tauri = json("src-tauri", "tauri.conf.json");
  const cargoToml = readFileSync(join(root, "src-tauri", "Cargo.toml"), "utf8");
  const cargoLock = readFileSync(join(root, "src-tauri", "Cargo.lock"), "utf8");
  const lockEntry = /\[\[package\]\]\r?\nname = "cargo-line"\r?\nversion = "([^"]*)"/.exec(cargoLock);
  return {
    packageJson: pkg.version,
    packageLock: lock.version,
    packageLockRoot: lock.packages?.[""]?.version,
    // "../package.json" means it reads package.json; otherwise it holds its own value.
    tauriConf: tauri.version,
    cargoToml: cargoPackageVersion(cargoToml),
    cargoLock: lockEntry ? lockEntry[1] : null,
  };
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const version = process.argv[2];
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  try {
    applyVersion(root, version);
  } catch (e) {
    console.error(`set-version: ${e.message}`);
    process.exit(1);
  }
  // Cargo rewrites the lock file's own entry; offline, nothing is downloaded.
  const r = spawnSync("cargo", ["metadata", "--format-version", "1", "--offline", "--manifest-path", join(root, "src-tauri", "Cargo.toml")], {
    stdio: ["ignore", "ignore", "inherit"],
    shell: false,
  });
  if (r.status !== 0) {
    console.error("set-version: cargo metadata failed; run it once by hand so Cargo.lock follows.");
    process.exit(1);
  }
  const v = readVersions(root);
  console.log(`version ${version}: package.json, package-lock.json, Cargo.toml, Cargo.lock (${v.cargoLock})`);
}
