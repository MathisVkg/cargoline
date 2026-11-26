// Update check (Task 20): the status from Rust (`update::UpdateStatus`) and the texts the app
// shows. Pure functions, tested in tests/frontend/task20.test.ts.
import type { AppMode } from "../data/model.ts";

export type CheckResult =
  | { kind: "upToDate"; version: string }
  | { kind: "available"; version: string }
  | { kind: "failed"; reason: string };

export type DownloadState =
  | { state: "idle" }
  | { state: "downloading"; received: number; total: number | null }
  | { state: "verifying" }
  | { state: "ready"; version: string }
  | { state: "failed"; message: string };

export interface UpdateAsset {
  name: string;
  size: number;
  url: string;
}

export interface UpdateOffer {
  version: string;
  tag: string;
  htmlUrl: string;
  publishedAt: string | null;
  /** Plain text (never rendered as Markdown or HTML). */
  notes: string;
  notesTruncated: boolean;
  setup: UpdateAsset | null;
  sums: UpdateAsset | null;
}

export interface UpdateStatus {
  currentVersion: string;
  mode: AppMode;
  inAppInstall: boolean;
  automatic: boolean;
  lastCheckMs: number | null;
  skippedVersion: string | null;
  checking: boolean;
  result: CheckResult | null;
  offer: UpdateOffer | null;
  marker: boolean;
  download: DownloadState;
}

export const UPDATE_MARKER_TEXT = "Update available";
/** The element id of Settings → Updates, which the marker scrolls to. */
export const UPDATES_SECTION_ID = "settings-updates";
export const AUTOMATIC_LABEL = "Check for updates automatically";
export const AUTOMATIC_HINT = "About 10 seconds after start, at most once a day. Only GitHub is contacted.";
export const PORTABLE_HINT = "Portable copies are updated by hand: download the new zip and copy your data folder into it.";
export const MANUAL_HINT = "This copy is updated by hand from the download page.";
export const FULL_NOTES_TEXT = "See the full notes on GitHub";
export const INSTALL_HEADING = "Install the update";

/** The result of a check, in words. */
export function checkResultText(result: CheckResult | null): string | null {
  if (!result) return null;
  switch (result.kind) {
    case "upToDate":
      return `You're up to date (${result.version})`;
    case "available":
      return `Version ${result.version} is available`;
    case "failed":
      return `Couldn't reach GitHub: ${result.reason}`;
  }
}

/** "Cargo Line will close, install version X, and open again. …" */
export function installConfirmText(version: string): string {
  return `Cargo Line will close, install version ${version}, and open again. Your contracts and settings are kept.`;
}

const MB = 1024 * 1024;
const mb = (bytes: number) => (bytes / MB).toFixed(1);

/** Progress or outcome of the download, or null when idle. */
export function downloadText(d: DownloadState): string | null {
  switch (d.state) {
    case "idle":
      return null;
    case "downloading":
      return d.total && d.total > 0
        ? `Downloading… ${Math.min(100, Math.floor((d.received / d.total) * 100))} % (${mb(d.received)} of ${mb(d.total)} MB)`
        : `Downloading… ${mb(d.received)} MB`;
    case "verifying":
      return "Checking the download…";
    case "ready":
      return `Version ${d.version} is downloaded and checked.`;
    case "failed":
      return d.message;
  }
}

/** Which button the update row shows. */
export type UpdateAction = "download" | "installNow" | "downloading" | "openPage" | "none";

export function updateAction(s: Pick<UpdateStatus, "offer" | "inAppInstall" | "download">): UpdateAction {
  if (!s.offer) return "none";
  // Portable copies, dev builds, and releases without the installer or SHA256SUMS.txt: a link only.
  if (!s.inAppInstall || !s.offer.setup || !s.offer.sums) return "openPage";
  switch (s.download.state) {
    case "downloading":
    case "verifying":
      return "downloading";
    case "ready":
      return s.download.version === s.offer.version ? "installNow" : "download";
    default:
      return "download";
  }
}

/** Whether the top-bar marker shows (the backend already applies "Skip this version"). */
export function markerVisible(s: Pick<UpdateStatus, "marker" | "offer"> | null): boolean {
  return !!s?.offer && s.marker;
}

/** "Last checked 26 Nov 2026, 18:43" or "Never checked". */
export function lastCheckText(ms: number | null, locale?: string): string {
  if (ms === null) return "Never checked";
  const d = new Date(ms);
  return `Last checked ${d.toLocaleDateString(locale, { day: "numeric", month: "short", year: "numeric" })}, ${d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })}`;
}

/** The release date as a short local date, if GitHub gave one. */
export function releaseDateText(iso: string | null, locale?: string): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  return d.toLocaleDateString(locale, { day: "numeric", month: "short", year: "numeric" });
}
