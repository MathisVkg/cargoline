// Add Contract suggestion lists (Task 18). Loaded when the form first opens, never at startup.
// - The bundled reference lists never change while the app runs: loaded once, kept for the
//   session (a failed load is retried at the next open).
// - The locations and commodities taken from saved contracts change when contracts are saved,
//   so they are asked for again at every open.
import { listLocations, listReferenceCommodities, listReferenceLocations, listUsedCommodities } from "./api";
import { sessionCache } from "../perf/sessionCache";
import type { LocationEntry, ReferenceCommodity, ReferenceLocation } from "./types";

export const referenceLocations: () => Promise<ReferenceLocation[]> = sessionCache(listReferenceLocations);
export const referenceCommodities: () => Promise<ReferenceCommodity[]> = sessionCache(listReferenceCommodities);
export const userLocations = (): Promise<LocationEntry[]> => listLocations();
export const usedCommodities = (): Promise<string[]> => listUsedCommodities();
