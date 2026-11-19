/** Persisted settings, as the backend serialises them (snake_case field names). */
export type CaptureSource = "star_citizen_window" | "full_display";

export interface AppSettings {
  game_log_monitoring_enabled: boolean;
  game_log_path: string | null;
  capture_source: CaptureSource;
  /** 0–5000, multiple of 100. */
  capture_delay_ms: number;
  capture_on_add_contract: boolean;
  capture_shortcut: string | null;
  auto_capture_on_accept: boolean;
  selected_ship_id: string | null;
  automatic_update_checks: boolean;
  contract_grouping: string;
}

export interface CaptureSettingsInput {
  source: CaptureSource;
  delayMs: number;
  onAddContract: boolean;
}

/** A stored screenshot. Unsaved (contractId null) until its contract is saved. */
export interface CaptureInfo {
  id: number;
  contractId: number | null;
  capturedAt: number;
  expiresAt: number;
  source: "window" | "display" | null;
  backend: "wgc" | "print_window" | null;
  width: number | null;
  height: number | null;
}
