/** Persisted settings, as the backend serialises them (snake_case field names). */
export type CaptureSource = "star_citizen_window" | "full_display";

export type CompleteFromGameMode = "ask" | "automatic";

export interface AppSettings {
  game_log_monitoring_enabled: boolean;
  game_log_path: string | null;
  capture_source: CaptureSource;
  /** 0–5000, multiple of 100. */
  capture_delay_ms: number;
  capture_on_add_contract: boolean;
  capture_shortcut: string | null;
  auto_capture_on_accept: boolean;
  /** Complete contracts the game reports as completed (requires monitoring). */
  complete_from_game: boolean;
  complete_from_game_mode: CompleteFromGameMode;
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

/* ---------- OCR (recognition only; Task 09 turns it into form fields) ---------- */

export interface OcrBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface OcrWord {
  text: string;
  /** 0–100 (-1 when unknown). */
  confidence: number;
  box: OcrBox;
}

export interface OcrLine {
  text: string;
  /** Mean word confidence, 0–100 (-1 when unknown). */
  confidence: number;
  box: OcrBox;
  words: OcrWord[];
}

export type OcrRegionKind = "title" | "reward" | "objectives" | "details";

export interface OcrRegion {
  kind: OcrRegionKind;
  box: OcrBox;
  /** "fallback" when the anchor was not found and fixed fractions were used. */
  source: "anchor" | "fallback";
  lines: OcrLine[];
}

/** From the reward card: accepted = "Contract Deadline" (Accepted tab); offer = "Contract Availability" (Offers tab). */
export type ScreenState = "accepted" | "offer" | "unknown";

export interface OcrResult {
  width: number;
  height: number;
  anchors: { header: boolean; details: boolean; reward: boolean };
  screen: ScreenState;
  regions: OcrRegion[];
  timings: { loadMs: number; anchorMs: number; regionsMs: number; totalMs: number };
}

/* ---------- Parsed contract (Task 09; the backend parser's output) ---------- */

/** ok = read reliably; check = filled, to verify; missing = left empty. */
export type ParseFlag = "ok" | "check" | "missing";

export interface ParsedField<T> {
  value: T | null;
  flag: ParseFlag;
  reason: string | null;
}

export type NameResolution = "exact" | "stripped" | "lagrange" | "folded" | "unresolved";

export interface ParsedName extends ParsedField<string> {
  resolution: NameResolution;
  /** The cleaned OCR text it came from. */
  read: string;
}

export interface ParsedStop {
  location: ParsedName;
  /** Whole SCU; null with flag "ok" means unknown (several pickups). */
  scu: ParsedField<number>;
}

export interface ParsedRequirement {
  commodity: ParsedName;
  totalScu: ParsedField<number>;
  pickups: ParsedStop[];
  deliveries: ParsedStop[];
}

export interface ParsedContract {
  screen: ScreenState;
  title: ParsedField<string>;
  reward: ParsedField<number>;
  requirements: ParsedRequirement[];
  problems: string[];
  /** Max container size from the Details text (every requirement); null + "ok" = none stated. */
  maxBoxScu: ParsedField<number>;
}

export interface ContractReading {
  ocr: OcrResult;
  parsed: ParsedContract;
}
