// Settings → Updates / Data / About texts (pure; Task 17).

/** Settings groups, in page order. */
export const SETTINGS_GROUPS = [
  { index: "01", title: "Game" },
  { index: "02", title: "OCR Capture" },
  { index: "03", title: "Updates" },
  { index: "04", title: "Data" },
  { index: "05", title: "About" },
] as const;

export const SETTINGS_DESCRIPTION =
  "Settings are saved on this computer. Back up or restore your data in the Data group.";

export const UPDATES_HINT = "Update checking arrives in a later version.";

/* ------------------------------------------------------------------ Data */

/** What a backup contains (`inspect_backup`, `export_backup`). */
export interface BackupInfo {
  schemaVersion: number;
  activeContracts: number;
  finishedContracts: number;
  runs: number;
  bytes: number;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** `cargo-line-backup-2026-11-26.sqlite3` (local date). */
export function backupFileName(now: Date): string {
  return `cargo-line-backup-${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}.sqlite3`;
}

export const BACKUP_FILTER = { name: "Cargo Line backup (*.sqlite3)", extensions: ["sqlite3"] };

/** "C:\…\io.github.mathisvkg.sccargoapp": the drive and the last folder, full path in the tooltip. */
export function shortenPath(path: string, max = 48): string {
  if (path.length <= max) return path;
  const parts = path.split(/[\\/]/).filter(Boolean);
  if (parts.length < 3) return `…${path.slice(-(max - 1))}`;
  const sep = path.includes("\\") ? "\\" : "/";
  return `${parts[0]}${sep}…${sep}${parts[parts.length - 1]}`;
}

/** "1.2 MB", "340 KB", "512 B". */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export const SCREENSHOTS_NOT_INCLUDED = "Screenshots are not included.";

export function exportNotice(fileName: string, bytes: number): string {
  return `Backup saved: ${fileName} (${formatBytes(bytes)}).`;
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** What the backup contains, for the confirmation. */
export function backupContents(info: BackupInfo): string {
  return `This backup contains ${plural(info.activeContracts, "active contract")}, ${plural(info.finishedContracts, "finished contract")} and ${plural(info.runs, "run")}.`;
}

export const RESTORE_QUESTION =
  "Replace all current data with this backup? Your current data is saved as a safety copy first. Cargo Line restarts to finish.";

/** Refusal texts by reason (the backend sends the same wording). */
export const REFUSALS: Record<string, string> = {
  notSqlite: "This file is not a Cargo Line backup (it is not a SQLite database).",
  notCargoLine: "This file is not a Cargo Line backup (it belongs to another program).",
  tooNew: "This backup was made by a newer version of Cargo Line.",
  corrupted: "This backup is damaged and cannot be used.",
  sameAsLive: "Choose a file other than the database Cargo Line is using.",
};

export function refusalMessage(error: { kind: string; reason?: string; message?: string }): string {
  if (error.kind === "backup" && error.reason && error.reason in REFUSALS) {
    // The backend message may add detail (e.g. what integrity_check found).
    return error.reason === "corrupted" && error.message ? error.message : REFUSALS[error.reason];
  }
  return error.message ?? "The backup could not be used.";
}

/* ----------------------------------------------------------------- About */

export interface CatalogCredit {
  name: string;
  gameVersion: string;
  generatedAt: string;
}

export const ABOUT_DESCRIPTION = "Offline-first hauling contract manager for Star Citizen. No account, no server.";
export const LICENCE_TEXT = "Free software under the GNU General Public License v3.0";
export const DATA_CREDIT = "Ship, location and commodity data from the Star Citizen Wiki API (api.star-citizen.wiki).";
export const DATA_TERMS = "Used under its terms; non-commercial.";
export const UNOFFICIAL =
  "Cargo Line is an unofficial Star Citizen fan tool, not affiliated with the Cloud Imperium group of companies. Star Citizen® is a registered trademark of Cloud Imperium Rights LLC. All game content belongs to its respective owners.";
export const REPOSITORY_LABEL = "github.com/MathisVkg/cargoline";

/** "Ships: game 4.10.3-LIVE.13165946, generated 2026-11-24". */
export function catalogCreditLine(c: CatalogCredit): string {
  return `${c.name}: game ${c.gameVersion}, generated ${c.generatedAt}`;
}

/** Link keys the backend accepts (it holds the URLs). */
export type LinkKey = "repository" | "license" | "wikiApi";

/** Splits the notices Markdown into display blocks: headings, table rows, paragraphs, rules. */
export type NoticeBlock = { kind: "heading"; level: number; text: string } | { kind: "rule" } | { kind: "text"; text: string };

export function noticeBlocks(markdown: string): NoticeBlock[] {
  const blocks: NoticeBlock[] = [];
  let paragraph: string[] = [];
  const flush = () => {
    if (paragraph.length) blocks.push({ kind: "text", text: paragraph.join("\n") });
    paragraph = [];
  };
  for (const raw of markdown.replace(/\r\n/g, "\n").split("\n")) {
    const line = raw.trimEnd();
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      flush();
      blocks.push({ kind: "heading", level: heading[1].length, text: heading[2] });
    } else if (/^-{3,}$/.test(line.trim())) {
      flush();
      blocks.push({ kind: "rule" });
    } else if (line.trim() === "") {
      flush();
    } else if (/^\|[\s:|-]+\|$/.test(line.trim())) {
      // table separator row: skip
    } else {
      paragraph.push(line.startsWith("|") ? line.split("|").slice(1, -1).map((c) => c.trim()).join("  ·  ") : line);
    }
  }
  flush();
  return blocks;
}
