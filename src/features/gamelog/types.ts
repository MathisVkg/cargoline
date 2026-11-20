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
  | {
      kind: "ended";
      missionId: string;
      state: string;
      at: string | null;
      contractId: number;
      /** "Complete contracts finished in game" when the event was read. */
      completeFromGame: CompleteFromGame;
    };

export type CompleteFromGame = "off" | "ask" | "automatic";

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
  completeFromGame: boolean;
  completeFromGameMode: "ask" | "automatic";
}

export const MISSION_STATE_COMPLETED = "MISSION_STATE_COMPLETED";

/** A Game.log found by Detect (Task 10.1). */
export interface FoundLog {
  path: string;
  channel: string;
  modifiedMs: number | null;
  /** process = the running game; launcher = the RSI Launcher log; folders = common install folders. */
  method: "process" | "launcher" | "folders";
}

export interface DetectResult {
  /** Ranked: LIVE first, then the most recently modified. */
  found: FoundLog[];
  message: string;
}
