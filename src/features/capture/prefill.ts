// Pure helpers that turn a parsed contract into form state (Task 09). No React, no IPC.
import { emptyRequirement, emptyStop, isDraftEmpty, type ContractDraft, type RequirementDraft, type StopDraft } from "../contracts/draft.ts";
import { contractRef } from "../contracts/grouping.ts";
import { parseRewardAuec, parseScuMilli } from "../contracts/scu.ts";
import type { ContractDetail } from "../contracts/types.ts";
import type { ParsedContract, ParsedField, ParsedStop } from "./types.ts";

/**
 * Hints for fields read with a problem, keyed by stable row keys (not list positions), so
 * removing a row never moves a hint onto another field:
 * `title`, `rewardAuec`, `<requirementKey>:commodityName`, `<requirementKey>:quantityScu`,
 * `<stopKey>:locationName`, `<stopKey>:quantityScu`.
 */
export type FieldFlags = ReadonlyMap<string, string>;

export const flagKeys = {
  title: "title",
  reward: "rewardAuec",
  commodity: (requirementKey: string) => `${requirementKey}:commodityName`,
  total: (requirementKey: string) => `${requirementKey}:quantityScu`,
  maxBox: (requirementKey: string) => `${requirementKey}:maxBoxSizeScu`,
  location: (stopKey: string) => `${stopKey}:locationName`,
  stopScu: (stopKey: string) => `${stopKey}:quantityScu`,
} as const;

/** The hint shown under a flagged field; null when the field was read reliably. */
export function flagHint(field: ParsedField<unknown>): string | null {
  if (field.flag === "ok") return null;
  return field.reason ?? (field.flag === "missing" ? "Couldn't read this" : "Check this value");
}

const text = (field: ParsedField<string | number>) => (field.value === null ? "" : String(field.value));

/** Builds the form draft and its hints. Unknown pickup SCU stays empty, without a hint. */
export function draftFromParsed(parsed: ParsedContract): { draft: ContractDraft; flags: Map<string, string> } {
  const flags = new Map<string, string>();
  const flag = (key: string, field: ParsedField<unknown>) => {
    const hint = flagHint(field);
    if (hint) flags.set(key, hint);
  };
  const stops = (list: ParsedStop[], missingHint: string): StopDraft[] => {
    if (list.length === 0) {
      const stop = emptyStop();
      flags.set(flagKeys.location(stop.key), missingHint);
      return [stop];
    }
    return list.map((s) => {
      const stop: StopDraft = { ...emptyStop(), locationName: text(s.location), quantityScu: text(s.scu) };
      flag(flagKeys.location(stop.key), s.location);
      flag(flagKeys.stopScu(stop.key), s.scu);
      return stop;
    });
  };
  const requirements: RequirementDraft[] = parsed.requirements.map((r) => {
    const base = emptyRequirement();
    const requirement: RequirementDraft = {
      ...base,
      commodityName: text(r.commodity),
      quantityScu: text(r.totalScu),
      // The Details text's max container size applies to every requirement (Task 10.1).
      maxBoxSizeScu: parsed.maxBoxScu ? text(parsed.maxBoxScu) : "",
      pickups: stops(r.pickups, "No pickup line was read"),
      deliveries: stops(r.deliveries, "No delivery was read"),
    };
    flag(flagKeys.commodity(requirement.key), r.commodity);
    flag(flagKeys.total(requirement.key), r.totalScu);
    if (parsed.maxBoxScu) flag(flagKeys.maxBox(requirement.key), parsed.maxBoxScu);
    return requirement;
  });
  flag(flagKeys.title, parsed.title);
  flag(flagKeys.reward, parsed.reward);
  return {
    draft: { title: text(parsed.title), rewardAuec: text(parsed.reward), requirements: requirements.length ? requirements : [emptyRequirement()] },
    flags,
  };
}

/** Field values by flag key, to see which fields an edit touched. */
function valuesByKey(draft: ContractDraft): Map<string, string> {
  const values = new Map<string, string>([
    [flagKeys.title, draft.title],
    [flagKeys.reward, draft.rewardAuec],
  ]);
  for (const r of draft.requirements) {
    values.set(flagKeys.commodity(r.key), r.commodityName);
    values.set(flagKeys.total(r.key), r.quantityScu);
    values.set(flagKeys.maxBox(r.key), r.maxBoxSizeScu);
    for (const s of [...r.pickups, ...r.deliveries]) {
      values.set(flagKeys.location(s.key), s.locationName);
      values.set(flagKeys.stopScu(s.key), s.quantityScu);
    }
  }
  return values;
}

/** Editing a field clears its hint (so does removing its row). Returns `flags` itself when nothing changed. */
export function clearChangedFlags(flags: FieldFlags, previous: ContractDraft, next: ContractDraft): FieldFlags {
  if (flags.size === 0) return flags;
  const before = valuesByKey(previous);
  const after = valuesByKey(next);
  const kept = new Map([...flags].filter(([key]) => after.has(key) && after.get(key) === before.get(key)));
  return kept.size === flags.size ? flags : kept;
}

/**
 * What to do with a new reading: fill the form when the user has not typed anything (or the
 * form only holds an earlier reading), otherwise offer it in the "Apply to form" bar.
 */
export function readingAction(form: { draft: ContractDraft; userEdited: boolean }): "fill" | "offer" {
  return !form.userEdited || isDraftEmpty(form.draft) ? "fill" : "offer";
}

/** Applying replaces the whole form: ask first when it holds anything. */
export function applyNeedsConfirm(draft: ContractDraft): boolean {
  return !isDraftEmpty(draft);
}

const normTitle = (s: string) => s.trim().replace(/\s+/g, " ").toLowerCase();

/** An active contract with the same title, reward and total SCU (a hint only, never blocking). */
export function findDuplicate(draft: ContractDraft, active: readonly ContractDetail[]): ContractDetail | null {
  const reward = parseRewardAuec(draft.rewardAuec);
  if (!draft.title.trim() || !reward.ok) return null;
  let total = 0;
  for (const r of draft.requirements) {
    const q = parseScuMilli(r.quantityScu);
    if (!q.ok) return null;
    total += q.value;
  }
  return (
    active.find(
      (c) =>
        c.status === "active" &&
        normTitle(c.title) === normTitle(draft.title) &&
        c.rewardAuec === reward.value &&
        c.requirements.reduce((sum, r) => sum + r.quantityScuMilli, 0) === total,
    ) ?? null
  );
}

export function duplicateHint(contract: ContractDetail): string {
  return `Looks like ${contractRef(contract.id)} — already added?`;
}

/** The non-blocking banner for the screen the text was read from. */
export const BANNER_OFFER = "Read from the Offers screen — amounts can change after accepting. Recapture from the Accepted tab.";
export const BANNER_UNKNOWN_MILD = "Couldn't confirm this is the Accepted tab — check the amounts";
export const BANNER_UNKNOWN_STRONG = "Contract screen not recognised — check every field.";

/** Title, reward and objectives all read, and no parser problems. */
export function readCleanly(parsed: ParsedContract): boolean {
  return (
    parsed.problems.length === 0 &&
    parsed.title.value !== null &&
    parsed.reward.value !== null &&
    parsed.requirements.length > 0
  );
}

/**
 * The banner for the screen the text was read from. Unknown screen: the milder wording when the
 * reading itself was clean, the stronger one when it had problems.
 */
export function screenBanner(parsed: Pick<ParsedContract, "screen" | "problems" | "title" | "reward" | "requirements">): string | null {
  switch (parsed.screen) {
    case "offer":
      return BANNER_OFFER;
    case "unknown":
      return readCleanly(parsed as ParsedContract) ? BANNER_UNKNOWN_MILD : BANNER_UNKNOWN_STRONG;
    case "accepted":
      return null;
  }
}

/** The hint on fields that disagree with the Game.log first objective (Task 10). */
export const GAME_LOG_MISMATCH = "Differs from game log — check";

const sameName = (a: string, b: string) => a.trim().replace(/\s+/g, " ").toLowerCase() === b.trim().replace(/\s+/g, " ").toLowerCase();

/**
 * Compares the form's first objective with the Game.log first objective (both resolved names).
 * Only differing, non-empty fields are flagged; the log never overwrites the form.
 */
export function crossCheckFlags(
  draft: ContractDraft,
  objective: { scu: number; commodity: string; location: string } | null,
): Map<string, string> {
  const flags = new Map<string, string>();
  const first = draft.requirements[0];
  if (!objective || !first) return flags;
  if (first.commodityName.trim() && !sameName(first.commodityName, objective.commodity)) {
    flags.set(flagKeys.commodity(first.key), GAME_LOG_MISMATCH);
  }
  const total = parseScuMilli(first.quantityScu);
  if (first.quantityScu.trim() && (!total.ok || total.value !== objective.scu * 1000)) {
    flags.set(flagKeys.total(first.key), GAME_LOG_MISMATCH);
  }
  const delivery = first.deliveries[0];
  if (delivery && delivery.locationName.trim() && !sameName(delivery.locationName, objective.location)) {
    flags.set(flagKeys.location(delivery.key), GAME_LOG_MISMATCH);
  }
  return flags;
}
