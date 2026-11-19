/** The first `Deliver …` objective from Game.log, resolved like the parser's names. */
export interface FirstObjective {
  scu: number;
  commodity: string;
  location: string;
}

/** Tauri event `gamelog://event` (never raw lines). */
export type GameLogEvent =
  | { kind: "accepted"; missionId: string; title: string; at: string | null; objective: FirstObjective | null; autoCapture: boolean }
  | { kind: "objective"; missionId: string; objective: FirstObjective }
  /** A mission of an active contract ended in game. */
  | { kind: "ended"; missionId: string; state: string; at: string | null; contractId: number };

export type AcceptedEvent = Extract<GameLogEvent, { kind: "accepted" }>;

/** Tauri event `gamelog://status` and `get_game_log_status`. */
export type WatchStatus =
  | { state: "off" }
  | { state: "watching"; path: string }
  | { state: "fileNotFound"; path: string }
  | { state: "unreadable"; path: string; reason: string };

export interface GameLogSettingsInput {
  monitoring: boolean;
  path: string | null;
  autoCapture: boolean;
}

export const MISSION_STATE_COMPLETED = "MISSION_STATE_COMPLETED";
