// Shared helpers for the catalog generators: arguments, a throttled and cached JSON fetch from
// the Star Citizen Wiki API, and deterministic output. Plain Node (no dependencies).
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const TOOL_DIR = dirname(fileURLToPath(import.meta.url));
export const REPO_DIR = resolve(TOOL_DIR, "..", "..");
export const CATALOG_DIR = join(REPO_DIR, "src-tauri", "resources", "catalog");
export const CACHE_DIR = join(TOOL_DIR, ".cache");
export const API = "https://api.star-citizen.wiki/api";
export const USER_AGENT = "cargo-line-catalog-gather";
export const SOURCE = "Star Citizen Wiki API (api.star-citizen.wiki)";
export const ATTRIBUTION = "Data from api.star-citizen.wiki. Unofficial; not affiliated with Cloud Imperium Games.";
/** At most one request per second. */
const MIN_INTERVAL_MS = 1000;

/**
 * Command-line options shared by every generator:
 *   --version <code>   pin the game version (default: the API's default version)
 *   --cache-only       never use the network; fail if a response is not cached
 *   --check            do not write; exit 1 if the output would differ from the file
 *   --out <path>       write somewhere else (e.g. to compare)
 */
export function parseArgs(argv = process.argv.slice(2)) {
  const opts = { version: null, cacheOnly: false, check: false, out: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--version") opts.version = argv[++i];
    else if (a === "--cache-only") opts.cacheOnly = true;
    else if (a === "--check") opts.check = true;
    else if (a === "--out") opts.out = argv[++i];
    else if (a === "--help" || a === "-h") {
      console.log("Options: --version <code>  --cache-only  --check  --out <path>");
      process.exit(0);
    } else throw new Error(`unknown option ${a}`);
  }
  return opts;
}

let lastRequest = 0;
let requests = 0;
let cacheHits = 0;

/** Cache file for a URL: readable prefix + hash, so different query strings never collide. */
export function cacheFile(url) {
  const path = url.replace(API, "").replace(/[^a-zA-Z0-9]+/g, "_").replace(/^_|_$/g, "").slice(0, 80);
  const hash = createHash("sha256").update(url).digest("hex").slice(0, 12);
  return join(CACHE_DIR, `${path}-${hash}.json`);
}

/** GET a JSON document, from the cache if present; otherwise throttled, with retries. */
export async function getJson(url, { cacheOnly = false } = {}) {
  const file = cacheFile(url);
  if (existsSync(file)) {
    cacheHits++;
    return JSON.parse(readFileSync(file, "utf8"));
  }
  if (cacheOnly) throw new Error(`not cached (--cache-only): ${url}`);
  mkdirSync(CACHE_DIR, { recursive: true });
  for (let attempt = 1; ; attempt++) {
    const wait = lastRequest + MIN_INTERVAL_MS - Date.now();
    if (wait > 0) await new Promise((r) => setTimeout(r, wait));
    lastRequest = Date.now();
    requests++;
    try {
      const res = await fetch(url, { headers: { "User-Agent": USER_AGENT, Accept: "application/json" } });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const json = await res.json();
      writeFileSync(file, JSON.stringify(json));
      return json;
    } catch (e) {
      if (attempt >= 4) throw new Error(`${url}: ${e.message}`);
      await new Promise((r) => setTimeout(r, 2000 * attempt));
    }
  }
}

export function stats() {
  return { requests, cacheHits };
}

/** URL with query parameters (and the pinned version, when given). */
export function apiUrl(path, params = {}, version = null) {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) q.set(k, String(v));
  if (version) q.set("version", version);
  const s = q.toString();
  return `${API}${path}${s ? `?${s}` : ""}`;
}

/** Every page of a list endpoint (`page[size]` is capped at 50 on several endpoints). */
export async function getAllPages(path, params, opts, size = 50) {
  const out = [];
  for (let page = 1; ; page++) {
    const json = await getJson(apiUrl(path, { ...params, "page[size]": size, "page[number]": page }, opts.version), opts);
    out.push(...json.data);
    const last = json.meta?.last_page ?? 1;
    if (page >= last) break;
  }
  return out;
}

/** The game version to record: the pinned one, or the API's default. */
export async function gameVersion(opts) {
  if (opts.version) return opts.version;
  const json = await getJson(apiUrl("/game-versions/default"), opts);
  return json.data.code;
}

/** Sort key used by the app's list checks: lowercase, whitespace collapsed. */
export const sortKey = (name) => name.split(/\s+/).filter(Boolean).join(" ").toLowerCase();
export const byName = (a, b) => {
  const x = sortKey(a.name);
  const y = sortKey(b.name);
  return x < y ? -1 : x > y ? 1 : a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
};

const today = () => new Date().toISOString().slice(0, 10);

/**
 * Writes `{source, attribution, game_version, generated_at, items}` as pretty JSON with a final
 * newline. `generated_at` stays the existing file's date when the version and items are
 * unchanged, so a rerun without data changes gives a byte-identical file (no diff).
 */
export function writeCatalog(name, version, items, opts) {
  const target = opts.out ? resolve(opts.out) : join(CATALOG_DIR, name);
  const existingPath = join(CATALOG_DIR, name);
  let generatedAt = today();
  if (existsSync(existingPath)) {
    const existing = JSON.parse(readFileSync(existingPath, "utf8"));
    if (existing.game_version === version && JSON.stringify(existing.items) === JSON.stringify(items)) generatedAt = existing.generated_at;
  }
  const text = `${JSON.stringify({ source: SOURCE, attribution: ATTRIBUTION, game_version: version, generated_at: generatedAt, items }, null, 2)}\n`;
  const current = existsSync(existingPath) ? readFileSync(existingPath, "utf8") : null;
  const same = current === text;
  if (opts.check) {
    console.log(same ? `${name}: identical (${items.length} items)` : `${name}: DIFFERS from the committed file`);
    if (!same) process.exitCode = 1;
    return { same, text };
  }
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, text);
  console.log(`${name}: ${items.length} items → ${target}${same ? " (unchanged)" : ""}`);
  return { same, text };
}
