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
  | { kind: "internal"; message: string };

/** Catalog entry (location or commodity) from list_locations / list_commodities. */
export interface CatalogEntry {
  id: number;
  name: string;
  kind: string | null;
  /** Parent planet or moon (locations only). */
  body: string | null;
  /** Star system (locations only). */
  system: string | null;
  source: "bundled" | "user";
  seenInHauling: boolean;
}

export interface CatalogCounts {
  bundled: number;
  user: number;
  retired: number;
}

/** Provenance of the bundled reference catalog (get_reference_catalog_info). */
export interface ReferenceCatalogInfo {
  syncedVersion: string | null;
  bundledVersion: string | null;
  sourceName: string | null;
  sourceUrl: string | null;
  gameVersion: string | null;
  fetchedOn: string | null;
  attribution: string | null;
  locations: CatalogCounts;
  commodities: CatalogCounts;
}
