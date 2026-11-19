import { locationKey } from "./grouping.ts";
import { parseRewardAuec, parseScuMilli } from "./scu.ts";
import type { NewContractInput, NewStopInput, ValidationIssue } from "./types.ts";

/**
 * Editable form state for a contract. Independent of how it was populated
 * (manual entry now, OCR results later).
 */
export interface ContractDraft {
  title: string;
  rewardAuec: string;
  requirements: RequirementDraft[];
}

export interface RequirementDraft {
  key: string;
  commodityName: string;
  quantityScu: string;
  maxBoxSizeScu: string;
  pickups: StopDraft[];
  deliveries: StopDraft[];
}

export interface StopDraft {
  key: string;
  locationName: string;
  quantityScu: string;
}

export type StopKind = "pickups" | "deliveries";

let nextKey = 0;
const newKey = () => `k${++nextKey}`;

export const emptyStop = (): StopDraft => ({ key: newKey(), locationName: "", quantityScu: "" });

export const emptyRequirement = (): RequirementDraft => ({
  key: newKey(),
  commodityName: "",
  quantityScu: "",
  maxBoxSizeScu: "",
  pickups: [emptyStop()],
  deliveries: [emptyStop()],
});

export const emptyDraft = (): ContractDraft => ({ title: "", rewardAuec: "", requirements: [emptyRequirement()] });

export function isDraftEmpty(draft: ContractDraft): boolean {
  const blank = (s: string) => s.trim() === "";
  return (
    blank(draft.title) &&
    blank(draft.rewardAuec) &&
    draft.requirements.every(
      (r) =>
        blank(r.commodityName) &&
        blank(r.quantityScu) &&
        blank(r.maxBoxSizeScu) &&
        [...r.pickups, ...r.deliveries].every((s) => blank(s.locationName) && blank(s.quantityScu)),
    )
  );
}

const optional = (text: string): string | null => (text.trim() === "" ? null : text);

/** Converts the draft into the IPC payload (raw text; Rust parses it). */
export function toInput(draft: ContractDraft): NewContractInput {
  const stops = (list: StopDraft[]): NewStopInput[] =>
    list.map((s) => ({ locationName: s.locationName, quantityScu: optional(s.quantityScu) }));
  return {
    title: draft.title,
    rewardAuec: draft.rewardAuec,
    requirements: draft.requirements.map((r) => ({
      commodityName: r.commodityName,
      quantityScu: r.quantityScu,
      maxBoxSizeScu: optional(r.maxBoxSizeScu),
      pickups: stops(r.pickups),
      deliveries: stops(r.deliveries),
    })),
  };
}

/* ------------------------------------------------------------------------- */
/* Client-side validation — mirrors contracts/validation.rs for instant       */
/* feedback. Rust re-validates every submission and is authoritative.         */
/* ------------------------------------------------------------------------- */

const LIMITS = { title: 200, commodity: 100, location: 200, requirements: 50, stops: 20 };
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

function checkText(issues: ValidationIssue[], path: string, value: string, label: string, max: number) {
  const v = value.trim();
  if (v === "") issues.push({ path, message: `${label} is required.` });
  else if ([...v].length > max) issues.push({ path, message: `${label} must be at most ${max} characters.` });
  else if (CONTROL.test(v)) issues.push({ path, message: `${label} contains invalid characters.` });
}

export function validateDraft(draft: ContractDraft): ValidationIssue[] {
  const issues: ValidationIssue[] = [];
  checkText(issues, "title", draft.title, "Contract title", LIMITS.title);
  const reward = parseRewardAuec(draft.rewardAuec);
  if (!reward.ok) issues.push({ path: "rewardAuec", message: reward.message });

  if (draft.requirements.length === 0) {
    issues.push({ path: "requirements", message: "Add at least one cargo objective." });
  } else if (draft.requirements.length > LIMITS.requirements) {
    issues.push({ path: "requirements", message: `At most ${LIMITS.requirements} cargo objectives are supported.` });
  }

  draft.requirements.forEach((req, i) => {
    const path = `requirements.${i}`;
    checkText(issues, `${path}.commodityName`, req.commodityName, "Commodity", LIMITS.commodity);

    let total: number | null = null;
    const qty = parseScuMilli(req.quantityScu);
    if (!qty.ok) issues.push({ path: `${path}.quantityScu`, message: qty.message });
    else if (qty.value === 0) issues.push({ path: `${path}.quantityScu`, message: "Total SCU must be greater than 0." });
    else total = qty.value;

    if (req.maxBoxSizeScu.trim() !== "") {
      const box = parseScuMilli(req.maxBoxSizeScu);
      if (!box.ok) issues.push({ path: `${path}.maxBoxSizeScu`, message: box.message });
      else if (box.value === 0)
        issues.push({ path: `${path}.maxBoxSizeScu`, message: "Max box size must be greater than 0." });
    }

    validateStops(issues, `${path}.pickups`, req.pickups, "pickup", total);
    validateStops(issues, `${path}.deliveries`, req.deliveries, "delivery", total);
  });
  return issues;
}

function validateStops(
  issues: ValidationIssue[],
  path: string,
  stops: StopDraft[],
  kind: string,
  total: number | null,
) {
  if (stops.length === 0) issues.push({ path, message: `Add at least one ${kind} location.` });
  else if (stops.length > LIMITS.stops)
    issues.push({ path, message: `At most ${LIMITS.stops} ${kind} locations are supported.` });

  const seen = new Set<string>();
  let quantitiesOk = true;
  const known: number[] = [];
  let unknown = 0;
  stops.forEach((stop, i) => {
    const name = stop.locationName.trim();
    checkText(issues, `${path}.${i}.locationName`, stop.locationName, "Location", LIMITS.location);
    if (name !== "") {
      const folded = locationKey(name);
      if (seen.has(folded)) issues.push({ path: `${path}.${i}.locationName`, message: `This ${kind} location is listed twice.` });
      seen.add(folded);
    }
    if (stop.quantityScu.trim() === "") {
      unknown += 1;
      return;
    }
    const q = parseScuMilli(stop.quantityScu);
    if (!q.ok) {
      quantitiesOk = false;
      issues.push({ path: `${path}.${i}.quantityScu`, message: q.message });
    } else if (q.value === 0) {
      quantitiesOk = false;
      issues.push({ path: `${path}.${i}.quantityScu`, message: "Leave empty if unknown, or enter more than 0." });
    } else {
      known.push(q.value);
    }
  });

  if (total !== null && quantitiesOk && stops.length > 0) {
    const sum = known.reduce((a, b) => a + b, 0);
    if (unknown === 0 && sum !== total) {
      issues.push({ path, message: `The ${kind} quantities must add up to the total SCU.` });
    } else if (unknown > 0 && sum >= total && sum > 0) {
      issues.push({ path, message: `Known ${kind} quantities leave nothing for the locations without a quantity.` });
    }
  }
}

/** First message per path. */
export function issuesByPath(issues: ValidationIssue[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const issue of issues) if (!map.has(issue.path)) map.set(issue.path, issue.message);
  return map;
}
