import { useRef, useState } from "react";
import { TopBar } from "./components/TopBar";
import { BottomNav } from "./components/BottomNav";
import { ContractsPage } from "./pages/ContractsPage";
import { CargoPage } from "./pages/CargoPage";
import { HistoryPage } from "./pages/HistoryPage";
import { SettingsPage } from "./pages/SettingsPage";
import type { PageId } from "./navigation";

/**
 * Application shell: fixed top bar, independently scrolling main area,
 * fixed bottom navigation. A three-row grid guarantees the bars never
 * overlap the content (no absolute/fixed positioning over <main>).
 */
export default function App() {
  const [page, setPage] = useState<PageId>("contracts");
  const mainRef = useRef<HTMLElement>(null);

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
        {page === "contracts" && <ContractsPage />}
        {page === "cargo" && <CargoPage />}
        {page === "history" && <HistoryPage />}
        {page === "settings" && <SettingsPage />}
      </main>
      <BottomNav current={page} onNavigate={navigate} />
    </div>
  );
}
