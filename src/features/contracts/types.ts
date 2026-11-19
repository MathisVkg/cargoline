/**
 * Types mirroring the Rust contract IPC models (src-tauri/src/contracts/models.rs).
 * Keep both sides in sync; Rust is authoritative.
 */

/** Raw form values sent to `create_contract`. Rust parses and validates the text. */
export interface NewContractInput {
  title: string;
  rewardAuec: string;
  requirements: NewRequirementInput[];
}

export interface NewRequirementInput {
  commodityName: string;
  quantityScu: string;
  maxBoxSizeScu: string | null;
  pickups: NewStopInput[];
  deliveries: NewStopInput[];
}

export interface NewStopInput {
  locationName: string;
  /** null = quantity at this location unknown. */
  quantityScu: string | null;
}

export type ContractStatus = "active" | "completed" | "abandoned";
export type ContractSource = "manual" | "ocr" | "auto_capture";

/** Quantities are integer milli-SCU (1 SCU = 1000). */
export interface ContractDetail {
  id: number;
  run: RunSummary;
  title: string;
  rewardAuec: number;
  status: ContractStatus;
  source: ContractSource;
  /** UTC milliseconds since the Unix epoch. */
  acceptedAt: number;
  /** UTC ms when completed or abandoned; null while active. */
  finishedAt: number | null;
  /** Sum of requirement quantities only. */
  totalScuMilli: number;
  requirements: RequirementDetail[];
}

export interface RunSummary {
  id: number;
  runNumber: number;
  runDate: string;
}

export interface RequirementDetail {
  id: number;
  commodityName: string;
  quantityScuMilli: number;
  maxBoxSizeScuMilli: number | null;
  /** Stored box breakdown; empty when none is known. */
  boxGroups: BoxGroupDetail[];
  pickups: StopDetail[];
  deliveries: StopDetail[];
}

export interface BoxGroupDetail {
  boxSizeScuMilli: number;
  boxCount: number;
  sourceQuality: "exact" | "estimated";
}

/** Persisted Contracts page display mode (`contract_grouping` setting). */
export type ContractGrouping = "by_location" | "by_contract";

export interface StopDetail {
  id: number;
  locationName: string;
  quantityScuMilli: number | null;
  /** Optional Pick Up / Turn In reminder: UTC ms when ticked, null if not. */
  reminderDoneAt: number | null;
}

export interface ValidationIssue {
  /** Input path, e.g. `requirements.0.pickups.1.locationName`. */
  path: string;
  message: string;
}

/** Error returned by Rust commands (tagged by `kind`). */
export type CommandError =
  | { kind: "validation"; issues: ValidationIssue[] }
  | { kind: "unavailable"; message: string }
  | { kind: "database"; message: string }
  | { kind: "internal"; message: string }
  /** The contract does not exist. */
  | { kind: "notFound"; message: string; id: number }
  /** The contract is already completed or abandoned. */
  | { kind: "notActive"; message: string; id: number; status: string }
  /** Bulk action on an out-of-date list; nothing was changed. */
  | { kind: "conflict"; message: string; notFound: number[]; notActive: number[] }
  /** A screenshot could not be taken; manual entry is unaffected. */
  | { kind: "capture"; reason: CaptureFailure; message: string }
  /** Text recognition failed or was cancelled; manual entry is unaffected. */
  | { kind: "ocr"; reason: OcrFailure; message: string };

/** Why text recognition did not return a result (typed by the backend). */
export type OcrFailure = "engineMissing" | "timeout" | "noPanel" | "cancelled" | "failed";

/** Why a capture failed (typed by the backend). */
export type CaptureFailure = "windowNotFound" | "minimized" | "busy" | "timeout" | "unsupported" | "failed";

/** Terminal statuses accepted by finalize_active_contracts. */
export type FinalStatus = "completed" | "abandoned";

/** Entry of the user's location catalog (names typed in saved contracts). */
export interface LocationEntry {
  id: number;
  name: string;
}

/** Bundled reference location (src-tauri/resources/catalog/locations.json). */
export interface ReferenceLocation {
  name: string;
  system: string | null;
  body: string | null;
}

/** Bundled reference commodity (src-tauri/resources/catalog/commodities.json). */
export interface ReferenceCommodity {
  name: string;
  group: string | null;
}
