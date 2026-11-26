import { open, save } from "@tauri-apps/plugin-dialog";
import { call } from "../contracts/api";
import { BACKUP_FILTER, backupFileName, type AppMode, type BackupInfo, type CatalogCredit, type LinkKey } from "./model";

export interface DataInfo {
  /** Full path of the app data folder. */
  folder: string;
  safetyCopies: number;
}

export interface ExportResult {
  fileName: string;
  info: BackupInfo;
}

export interface StartupNotice {
  ok: boolean;
  message: string;
}

export interface AboutInfo {
  version: string;
  mode: AppMode;
  catalogs: CatalogCredit[];
  thirdPartyNotices: string;
}

export const getDataInfo = () => call<DataInfo>("get_data_info");
/** Opens the app's own data folder in Explorer (no path is sent). */
export const openDataFolder = () => call<void>("open_data_folder");
export const exportBackup = (path: string) => call<ExportResult>("export_backup", { path });
export const inspectBackup = (path: string) => call<BackupInfo>("inspect_backup", { path });
/** Stages the backup and restarts the app (the promise may never resolve). */
export const restoreBackup = (path: string) => call<void>("restore_backup", { path });
export const takeStartupNotice = () => call<StartupNotice | null>("take_startup_notice");
export const openLink = (key: LinkKey) => call<void>("open_link", { key });
export const getAboutInfo = () => call<AboutInfo>("get_about_info");

/** Native Save dialog for a backup; null when cancelled. */
export async function chooseBackupDestination(): Promise<string | null> {
  const picked = await save({ title: "Export backup", defaultPath: backupFileName(new Date()), filters: [BACKUP_FILTER] });
  return typeof picked === "string" ? picked : null;
}

/** Native Open dialog for a backup; null when cancelled. */
export async function chooseBackupToRestore(): Promise<string | null> {
  const picked = await open({ title: "Restore backup", multiple: false, directory: false, filters: [BACKUP_FILTER] });
  return typeof picked === "string" ? picked : null;
}
