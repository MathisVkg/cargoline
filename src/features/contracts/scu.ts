/**
 * SCU and aUEC text handling. Mirrors src-tauri/src/contracts/validation.rs
 * using string/integer arithmetic only (no floating point).
 */

export const MILLI_PER_SCU = 1000;
const MAX_SCU_MILLI = 1_000_000 * MILLI_PER_SCU;
const MAX_REWARD_AUEC = 1_000_000_000_000;

export type Parsed = { ok: true; value: number } | { ok: false; message: string };

const isDigits = (s: string) => /^[0-9]*$/.test(s);

/** "12" | "4.5" | "0,125" -> milli-SCU. At most three decimals. */
export function parseScuMilli(text: string): Parsed {
  const trimmed = text.trim();
  if (trimmed === "") return { ok: false, message: "Enter a quantity." };
  const normalized = trimmed.replace(",", ".");
  const dot = normalized.indexOf(".");
  const whole = dot === -1 ? normalized : normalized.slice(0, dot);
  const fraction = dot === -1 ? "" : normalized.slice(dot + 1);
  if (whole === "" || !isDigits(whole) || !isDigits(fraction) || (dot !== -1 && fraction === "")) {
    return { ok: false, message: "Enter a number such as 12 or 4.5." };
  }
  if (fraction.length > 3) return { ok: false, message: "Use at most 3 decimal places." };
  const significant = whole.replace(/^0+/, "");
  if (significant.length > 7) return { ok: false, message: "Quantity is too large." };
  const milli = Number(significant || "0") * MILLI_PER_SCU + Number(fraction.padEnd(3, "0"));
  if (milli > MAX_SCU_MILLI) return { ok: false, message: "Quantity is too large." };
  return { ok: true, value: milli };
}

/** Whole aUEC; digits optionally grouped in threes with spaces or commas. */
export function parseRewardAuec(text: string): Parsed {
  const trimmed = text.trim();
  const invalid: Parsed = { ok: false, message: "Enter a whole number, such as 48250." };
  if (trimmed === "") return { ok: false, message: "Enter the reward (0 if none)." };
  const separator = [...trimmed].find((c) => c === "," || c === " ");
  let digits = trimmed;
  if (separator) {
    const groups = trimmed.split(separator);
    const grouped = groups[0].length >= 1 && groups[0].length <= 3 && groups.slice(1).every((g) => g.length === 3);
    if (!grouped) return invalid;
    digits = groups.join("");
  }
  if (!/^[0-9]+$/.test(digits)) return invalid;
  const significant = digits.replace(/^0+/, "");
  if (significant.length > 13) return { ok: false, message: "Reward is too large." };
  const value = Number(significant || "0");
  if (value > MAX_REWARD_AUEC) return { ok: false, message: "Reward is too large." };
  return { ok: true, value };
}

const grouping = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

/** 12000 -> "12", 4500 -> "4.5", 125 -> "0.125". */
export function formatScu(milli: number): string {
  const whole = Math.trunc(milli / MILLI_PER_SCU);
  const fraction = String(milli % MILLI_PER_SCU).padStart(3, "0").replace(/0+$/, "");
  return fraction ? `${grouping.format(whole)}.${fraction}` : grouping.format(whole);
}

export function formatAuec(value: number): string {
  return grouping.format(value);
}
