// Pages and modules not needed for the first screen are separate chunks. They are
// fetched in the background shortly after startup, so the first visit is still instant.
import { lazy } from "react";

export const loadCargoPage = () => import("./pages/CargoPage");
export const loadHistoryPage = () => import("./pages/HistoryPage");
export const loadSettingsPage = () => import("./pages/SettingsPage");
export const loadAddContractModal = () => import("./features/contracts/AddContractModal");
/** Only needed once (first launch) or on request: never prefetched. */
export const loadWelcomeGuide = () => import("./features/welcome/WelcomeGuide");

export const CargoPage = lazy(() => loadCargoPage().then((m) => ({ default: m.CargoPage })));
export const HistoryPage = lazy(() => loadHistoryPage().then((m) => ({ default: m.HistoryPage })));
export const SettingsPage = lazy(() => loadSettingsPage().then((m) => ({ default: m.SettingsPage })));
export const AddContractModal = lazy(() => loadAddContractModal().then((m) => ({ default: m.AddContractModal })));
export const WelcomeGuide = lazy(() => loadWelcomeGuide().then((m) => ({ default: m.WelcomeGuide })));

/** How long after the first screen the other chunks are fetched. */
export const PREFETCH_DELAY_MS = 1500;

/** Fetches the other chunks once the first screen is up (errors are ignored: a visit retries). */
export function prefetchLazyChunks(): void {
  for (const load of [loadAddContractModal, loadCargoPage, loadHistoryPage, loadSettingsPage]) {
    load().catch(() => undefined);
  }
}
