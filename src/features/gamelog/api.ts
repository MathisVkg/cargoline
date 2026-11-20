import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { call } from "../contracts/api";
import type { AppSettings } from "../capture/types";
import type { DetectResult, GameLogEvent, GameLogSettingsInput, WatchStatus } from "./types";

/** Saves the Game group (applied to the watcher right away). */
export function updateGameLogSettings(input: GameLogSettingsInput): Promise<AppSettings> {
  return call<AppSettings>("update_game_log_settings", { input });
}

export function getGameLogStatus(): Promise<WatchStatus> {
  return call<WatchStatus>("get_game_log_status");
}

/** Every Game.log found without opening the game (ranked) and the message to show. */
export function detectGameLogPath(): Promise<DetectResult> {
  return call<DetectResult>("detect_game_log_path");
}

/** The folder Browse… opens in (current path folder, else a game channel folder). */
export function gameLogBrowseStart(current: string | null): Promise<string | null> {
  return call<string | null>("game_log_browse_start", { current });
}

/** Native open-file dialog (tauri-plugin-dialog). Null when cancelled. */
export async function browseForGameLog(defaultPath: string | null): Promise<string | null> {
  const picked = await open({
    title: "Select Game.log",
    defaultPath: defaultPath ?? undefined,
    multiple: false,
    directory: false,
    filters: [
      { name: "Game.log", extensions: ["log"] },
      { name: "All files", extensions: ["*"] },
    ],
  });
  return typeof picked === "string" ? picked : null;
}

export function updateCaptureShortcut(shortcut: string | null): Promise<AppSettings> {
  return call<AppSettings>("update_capture_shortcut", { shortcut });
}

export function onGameLogEvent(handler: (event: GameLogEvent) => void): Promise<UnlistenFn> {
  return listen<GameLogEvent>("gamelog://event", (e) => handler(e.payload));
}

export function onGameLogStatus(handler: (status: WatchStatus) => void): Promise<UnlistenFn> {
  return listen<WatchStatus>("gamelog://status", (e) => handler(e.payload));
}

/** Settings changed on the Settings page (the shell reloads the shortcut). */
export const SETTINGS_CHANGED = "cargo-line:settings-changed";
export function announceSettingsChanged(): void {
  window.dispatchEvent(new Event(SETTINGS_CHANGED));
}
