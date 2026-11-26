import { Suspense, useEffect, useReducer, useRef, useState } from "react";
import { TopBar } from "./components/TopBar";
import { UPDATES_SECTION_ID } from "./features/update/model";
import { BottomNav } from "./components/BottomNav";
import { ContractsPage } from "./pages/ContractsPage";
import { CargoPage, HistoryPage, PREFETCH_DELAY_MS, SettingsPage, prefetchLazyChunks } from "./lazyPages";
import { PageSkeleton } from "./components/PageSkeleton";
import type { PageId } from "./navigation";
import { getSettings } from "./features/capture/api";
import { onGameLogEvent, SETTINGS_CHANGED } from "./features/gamelog/api";
import { initialFlow, reduceFlow } from "./features/gamelog/flow";
import { matchesShortcut } from "./features/gamelog/shortcut";
import { GameCompletions } from "./features/gamelog/GameCompletions";
import { StartupNoticeBanner } from "./features/data/StartupNoticeBanner";
import { traceMark, traceOnce } from "./features/perf/trace";

/**
 * Application shell: fixed top bar, independently scrolling main area,
 * fixed bottom navigation. A three-row grid guarantees the bars never
 * overlap the content (no absolute/fixed positioning over <main>).
 */
export default function App() {
  const [page, setPage] = useState<PageId>("contracts");
  const mainRef = useRef<HTMLElement>(null);
  // Game.log events and the Add Contract modal (Task 10); works from any page.
  const [flow, dispatch] = useReducer(reduceFlow, initialFlow);
  const [shortcut, setShortcut] = useState<string | null>(null);

  useEffect(() => {
    traceMark("first React render");
    const timer = window.setTimeout(prefetchLazyChunks, PREFETCH_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = () =>
      getSettings()
        .then((s) => !cancelled && setShortcut(s.capture_shortcut))
        .catch(() => undefined);
    void load();
    window.addEventListener(SETTINGS_CHANGED, load);
    return () => {
      cancelled = true;
      window.removeEventListener(SETTINGS_CHANGED, load);
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    onGameLogEvent((event) => dispatch({ type: "gameLog", event }))
      .then((fn) => (disposed ? fn() : (unlisten = fn)))
      .catch(() => undefined); // outside Tauri (plain browser): no events
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // The capture shortcut, only while Cargo Line has focus (keydown never reaches us otherwise).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat || !matchesShortcut(e, shortcut)) return;
      e.preventDefault();
      dispatch({ type: "shortcut" });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [shortcut]);

  // Opening Add Contract shows the Contracts page (never brings the window forward).
  useEffect(() => {
    if (flow.modal.open && flow.modal.kind === "add") setPage("contracts");
  }, [flow.modal.open, flow.modal.kind, flow.modal.session]);

  const navigate = (next: PageId) => {
    setPage(next);
    mainRef.current?.scrollTo({ top: 0 });
  };

  // The top-bar update marker: Settings, scrolled to the Updates group (the page may still load).
  const openUpdates = () => {
    navigate("settings");
    let tries = 0;
    const scroll = () => {
      const target = document.getElementById(UPDATES_SECTION_ID);
      if (target) target.scrollIntoView({ block: "start" });
      else if (++tries < 20) window.setTimeout(scroll, 50);
    };
    window.setTimeout(scroll, 0);
  };

  return (
    <div className="grid h-full grid-rows-[auto_minmax(0,1fr)_auto] bg-canvas">
      <TopBar onOpenUpdates={openUpdates} />
      <main
        ref={mainRef}
        id="main-content"
        aria-label="Page content"
        className="min-h-0 overflow-y-auto overflow-x-hidden [scrollbar-gutter:stable]"
      >
        <StartupNoticeBanner />
        <Suspense fallback={<PageSkeleton />}>
          <PageShown page={page} />
          {page === "contracts" && <ContractsPage flow={flow} dispatch={dispatch} />}
          {page === "cargo" && <CargoPage flow={flow} />}
          {page === "history" && <HistoryPage flow={flow} dispatch={dispatch} />}
          {page === "settings" && <SettingsPage />}
        </Suspense>
      </main>
      <BottomNav current={page} onNavigate={navigate} />
      <GameCompletions flow={flow} dispatch={dispatch} />
    </div>
  );
}

/** Trace mark when a page is first shown (inside Suspense: after its chunk has loaded). */
function PageShown({ page }: { page: PageId }) {
  useEffect(() => traceOnce(`page ${page} shown`), [page]);
  return null;
}
