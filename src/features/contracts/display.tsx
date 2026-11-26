import type { ReactNode } from "react";
import { ArrowDownToLine, ArrowUpFromLine, Check, ChevronDown, Square, SquareCheck, SquareMinus } from "lucide-react";
import { reminderBadgeText, reminderLabel, type ReminderIntent } from "./reminders";
import { boxInfo, type Operation, type StopQuantity } from "./grouping";
import { formatScu } from "./scu";
import type { BoxGroupDetail } from "./types";

/** Shared presentational pieces for the Contracts display modes. */

export const OPERATION = {
  pickup: { label: "Pickup", action: "Pick up", icon: ArrowUpFromLine, tone: "text-pickup", chip: "border-pickup/40 bg-pickup/10" },
  delivery: { label: "Turn In", action: "Turn in", icon: ArrowDownToLine, tone: "text-delivery", chip: "border-delivery/40 bg-delivery/10" },
} as const;

/** SCU for one stop. Unknown quantities show "?" with the objective total for context. */
export function StopQuantityText({ quantity, requirementScuMilli }: { quantity: StopQuantity; requirementScuMilli: number }) {
  if (quantity.kind === "unknown") {
    const total = formatScu(requirementScuMilli);
    return (
      <span title={`Quantity at this location unknown · objective total ${total} SCU`}>
        <span className="text-dim">?</span>
        <span className="text-[11px] text-dim"> /{total}</span>
        <span className="sr-only">
          {" "}
          Quantity at this location unknown, objective total {total} SCU
        </span>
      </span>
    );
  }
  return (
    <span title={quantity.kind === "derived" ? "Only location for this objective" : undefined} className="text-fg">
      {formatScu(quantity.scuMilli)}
    </span>
  );
}

/** Stored box breakdown, or known max box size, or a neutral dash. */
export function BoxText({ boxGroups, maxBoxSizeScuMilli }: { boxGroups: BoxGroupDetail[]; maxBoxSizeScuMilli: number | null }) {
  const info = boxInfo(boxGroups, maxBoxSizeScuMilli);
  if (info.kind === "breakdown") {
    const text = info.groups.map((g) => `${g.boxCount}×${formatScu(g.boxSizeScuMilli)}`).join(" · ");
    return (
      <span title={info.estimated ? "Estimated box breakdown (SCU per box)" : "Box breakdown (SCU per box)"}>
        {text}
        {info.estimated && <span className="text-dim"> est.</span>}
      </span>
    );
  }
  if (info.kind === "max") {
    return <span title="Maximum box size">max {formatScu(info.maxBoxSizeScuMilli)}</span>;
  }
  return (
    <span className="text-dim" title="No box information">
      —<span className="sr-only">No box information</span>
    </span>
  );
}

interface GroupHeaderProps {
  expanded: boolean;
  controls: string;
  onToggle: () => void;
  title: ReactNode;
  meta?: ReactNode;
  aside?: ReactNode;
  leading?: ReactNode;
  className?: string;
  /** Sizing of the right-hand side; default `shrink-0` (never wraps). */
  asideClassName?: string;
}

/** Full-width expand/collapse header for a display group. */
export function GroupHeader({ expanded, controls, onToggle, title, meta, aside, leading, className = "", asideClassName = "shrink-0" }: GroupHeaderProps) {
  return (
    <button
      type="button"
      aria-expanded={expanded}
      aria-controls={controls}
      onClick={onToggle}
      className={[
        "flex w-full min-w-0 items-center gap-2.5 px-2 py-1.5 text-left transition-colors duration-150 hover:bg-raised/70 focus-visible:-outline-offset-2",
        className,
      ].join(" ")}
    >
      <ChevronDown
        className={["size-4 shrink-0 text-dim transition-transform duration-150", expanded ? "" : "-rotate-90"].join(" ")}
        aria-hidden="true"
      />
      {leading}
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-semibold break-words text-fg">{title}</span>
        {meta && <span className="label-tech block">{meta}</span>}
      </span>
      {aside && <span className={`flex ${asideClassName} flex-wrap items-center justify-end gap-x-3 gap-y-0.5`}>{aside}</span>}
    </button>
  );
}

export function plural(count: number, word: string): string {
  return `${count} ${word}${count === 1 ? "" : "s"}`;
}

interface ReminderControlProps {
  operation: Operation;
  /** Shared click intent of the row: state, value to send and stop ids. */
  intent: ReminderIntent;
  /** Called with the row's intent; the badge and the checkbox send exactly the same thing. */
  onActivate: (intent: ReminderIntent) => void;
  /** Describes the row for screen readers, e.g. "Aluminum for C02". */
  context: string;
}

/** Real checkbox for the optional Pick Up / Turn In reminder (personal tick only). */
export function ReminderCheckbox({ operation, intent, onActivate, context }: ReminderControlProps) {
  const meta = OPERATION[operation];
  const { state } = intent;
  const label = reminderLabel(operation, state);
  const Icon = state === "all" ? SquareCheck : state === "mixed" ? SquareMinus : Square;
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={state === "all" ? true : state === "mixed" ? "mixed" : false}
      aria-label={`${label}: ${context}`}
      title={state === "mixed" ? `${label} (partly done)` : label}
      onClick={() => onActivate(intent)}
      className={[
        "grid size-7 shrink-0 place-items-center rounded-sm transition-colors duration-150 hover:bg-raised",
        state === "none" ? "text-dim hover:text-muted" : meta.tone,
      ].join(" ")}
    >
      <Icon className="size-4" aria-hidden="true" />
    </button>
  );
}

/**
 * The PICK UP / TURN IN badge as a toggle button for the same reminder as
 * the row checkbox. Ticked: "PICKED UP" / "TURNED IN" with a check icon.
 * Mixed: normal text plus a small partial mark. Fixed minimum width so the
 * row never shifts when the text changes.
 */
export function ReminderBadge({ operation, intent, onActivate, context }: ReminderControlProps) {
  const meta = OPERATION[operation];
  const { state } = intent;
  const label = reminderLabel(operation, state);
  const Icon = state === "all" ? Check : meta.icon;
  return (
    <button
      type="button"
      aria-pressed={state === "all" ? true : state === "mixed" ? "mixed" : false}
      aria-label={`${label}: ${context}`}
      title={state === "mixed" ? `${label} (partly done)` : label}
      onClick={() => onActivate(intent)}
      className={[
        "inline-flex h-6 w-fit min-w-[6.25rem] items-center gap-1 rounded-sm border px-1.5 font-mono text-[10px] font-semibold tracking-[0.08em] whitespace-nowrap uppercase",
        "cursor-pointer transition-[background-color,border-color,filter] duration-150 hover:brightness-125 focus-visible:outline-offset-1",
        meta.tone,
        meta.chip,
        state === "all" ? "border-current/60" : "",
      ].join(" ")}
    >
      <Icon className="size-3 shrink-0" aria-hidden="true" />
      <span>{reminderBadgeText(operation, state)}</span>
      {state === "mixed" && <SquareMinus className="ml-auto size-3 shrink-0 opacity-80" aria-hidden="true" />}
    </button>
  );
}
