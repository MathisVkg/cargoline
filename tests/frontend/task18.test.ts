// Frontend unit tests for Task 18: reference lists loaded lazily and cached for the session, the
// startup trace recorder (off = nothing sent after the first answer), code splitting, and the
// static dark shell.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { sessionCache } from "../../src/features/perf/sessionCache.ts";
import { createTracer, type TraceMark } from "../../src/features/perf/trace.ts";

const repo = join(import.meta.dirname, "..", "..");
const read = (...parts: string[]) => readFileSync(join(repo, ...parts), "utf8");

test("session cache loads once and retries after a failure", async () => {
  let calls = 0;
  let fail = true;
  const load = sessionCache(async () => {
    calls += 1;
    if (fail) throw new Error("not ready");
    return ["Lorville"];
  });
  await assert.rejects(load(), /not ready/);
  assert.equal(calls, 1);
  fail = false;
  const [a, b] = await Promise.all([load(), load()]);
  assert.deepEqual(a, ["Lorville"]);
  assert.equal(a, b, "the same cached list");
  await load();
  assert.equal(calls, 2, "loaded again only after the failure");
});

test("Add Contract loads its lists when it opens, the bundled ones from the session cache", () => {
  const modal = read("src", "features", "contracts", "AddContractModal.tsx");
  assert.match(modal, /suggestions\.referenceLocations\(\)/);
  assert.match(modal, /suggestions\.referenceCommodities\(\)/);
  assert.match(modal, /suggestions\.userLocations\(\)/);
  assert.match(modal, /suggestions\.usedCommodities\(\)/);
  const cache = read("src", "features", "contracts", "referenceCache.ts");
  assert.match(cache, /referenceLocations[^\n]*= sessionCache\(listReferenceLocations\)/);
  assert.match(cache, /referenceCommodities[^\n]*= sessionCache\(listReferenceCommodities\)/);
  // The lists from saved contracts change when contracts are saved: asked for at every open.
  assert.doesNotMatch(cache, /sessionCache\(listLocations\)|sessionCache\(listUsedCommodities\)/);
  // Nothing outside the modal asks for them at startup.
  for (const file of [["src", "App.tsx"], ["src", "pages", "ContractsPage.tsx"]]) {
    assert.doesNotMatch(read(...file), /listReference|referenceCache/, file.join("/"));
  }
});

test("trace recorder: off after the first answer, batched when on, names once", async () => {
  const flushers: (() => void)[] = [];
  const schedule = (fn: () => void) => flushers.push(fn);
  const flush = async () => {
    flushers.splice(0).forEach((fn) => fn());
    await new Promise((r) => setTimeout(r, 0));
  };

  // Off: one call, then nothing is recorded or sent.
  const sentOff: TraceMark[][] = [];
  const off = createTracer(async (m) => (sentOff.push(m), false), () => 1, schedule);
  off.mark("script start");
  off.mark("first React render");
  await flush();
  assert.equal(sentOff.length, 1);
  assert.equal(off.state(), false);
  off.mark("later");
  off.markOnce("page cargo shown");
  await flush();
  assert.equal(sentOff.length, 1, "no further call");

  // On: batched, markOnce deduplicated.
  const sentOn: TraceMark[][] = [];
  let t = 100;
  const on = createTracer(async (m) => (sentOn.push(m), true), () => t++, schedule);
  on.mark("script start");
  on.markOnce("page history shown");
  on.markOnce("page history shown");
  await flush();
  assert.deepEqual(sentOn, [[{ name: "script start", at: 100 }, { name: "page history shown", at: 101 }]]);
  on.markOnce("page history shown");
  await flush();
  assert.equal(sentOn.length, 1);

  // A failing call (outside Tauri) switches it off.
  const broken = createTracer(() => Promise.reject(new Error("no backend")), () => 1, schedule);
  broken.mark("x");
  await flush();
  assert.equal(broken.state(), false);
});

test("pages and Add Contract are separate chunks, prefetched after the first screen", () => {
  const app = read("src", "App.tsx");
  for (const page of ["CargoPage", "HistoryPage", "SettingsPage"]) {
    assert.doesNotMatch(app, new RegExp(`from "\\./pages/${page}"`), `${page} not imported statically`);
  }
  assert.match(app, /from "\.\/lazyPages"/);
  assert.match(app, /<Suspense fallback=\{<PageSkeleton \/>\}>/);
  assert.match(app, /setTimeout\(prefetchLazyChunks, PREFETCH_DELAY_MS\)/);
  const contracts = read("src", "pages", "ContractsPage.tsx");
  assert.doesNotMatch(contracts, /from "\.\.\/features\/contracts\/AddContractModal"/);
  const lazyPages = read("src", "lazyPages.ts");
  for (const path of ["./pages/CargoPage", "./pages/HistoryPage", "./pages/SettingsPage", "./features/contracts/AddContractModal"]) {
    assert.ok(lazyPages.includes(`import("${path}")`), path);
  }
});

test("dark background before any script: window colour and static shell", () => {
  const conf = JSON.parse(read("src-tauri", "tauri.conf.json"));
  assert.equal(conf.app.windows[0].backgroundColor, "#0a0c0f");
  const css = read("src", "styles.css");
  assert.match(css, /--color-canvas: #0a0c0f;/, "same colour as the theme");
  const html = read("index.html");
  assert.match(html, /<html[^>]*style="background: #0a0c0f"/);
  assert.match(html, /<div id="root"[^>]*>\s*<!--|<div id="root"[^>]*>\s*<div/, "the shell is inside #root (replaced by React)");
  assert.match(html, /grid-template-rows: 56px minmax\(0, 1fr\) 56px/, "top bar, page and bottom nav");
  assert.doesNotMatch(html, /<script(?![^>]*type="module")/, "no inline script (CSP)");
});
