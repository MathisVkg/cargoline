import { useEffect, useReducer, useRef, useState } from "react";
import { TopBar } from "./components/TopBar";
import { BottomNav } from "./components/BottomNav";
import { ContractsPage } from "./pages/ContractsPage";
import { CargoPage } from "./pages/CargoPage";
import { HistoryPage } from "./pages/HistoryPage";
import { SettingsPage } from "./pages/SettingsPage";
import type { PageId } from "./navigation";
import { getSettings } from "./features/capture/api";
import { onGameLogEvent, SETTINGS_CHANGED } from "./features/gamelog/api";
import { initialFlow, reduceFlow } from "./features/gamelog/flow";
import { matchesShortcut } from "./features/gamelog/shortcut";
import { GameCompletions } from "./features/gamelog/GameCompletions";

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

  return (
    <div className="grid h-full grid-rows-[auto_minmax(0,1fr)_auto] bg-canvas">
      <TopBar />
      <main
        ref={mainRef}
        id="main-content"
        aria-label="Page content"
        className="min-h-0 overflow-y-auto overflow-x-hidden [scrollbar-gutter:stable]"
      >
        {page === "contracts" && <ContractsPage flow={flow} dispatch={dispatch} />}
        {page === "cargo" && <CargoPage />}
        {page === "history" && <HistoryPage />}
        {page === "settings" && <SettingsPage />}
      </main>
      <BottomNav current={page} onNavigate={navigate} />
      <GameCompletions flow={flow} dispatch={dispatch} />
    </div>
  );
}
