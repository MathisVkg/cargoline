import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { call } from "../contracts/api";
import type { AppSettings } from "../capture/types";
import type { GameLogEvent, GameLogSettingsInput, WatchStatus } from "./types";

/** Saves the Game group (applied to the watcher right away). */
export function updateGameLogSettings(input: GameLogSettingsInput): Promise<AppSettings> {
  return call<AppSettings>("update_game_log_settings", { input });
}

export function getGameLogStatus(): Promise<WatchStatus> {
  return call<WatchStatus>("get_game_log_status");
}

/** `…\LIVE\Game.log` from the running StarCitizen.exe. */
export function detectGameLogPath(): Promise<string> {
  return call<string>("detect_game_log_path");
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
