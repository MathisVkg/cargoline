import type { ReactNode } from "react";
import { ArrowDownToLine, ArrowUpFromLine, ChevronDown } from "lucide-react";
import { boxInfo, type Operation, type StopQuantity } from "./grouping";
import { formatScu } from "./scu";
import type { BoxGroupDetail } from "./types";

/** Shared presentational pieces for the Contracts display modes. */

export const OPERATION = {
  pickup: { label: "Pickup", action: "Pick up", icon: ArrowUpFromLine, tone: "text-pickup", chip: "border-pickup/40 bg-pickup/10" },
  delivery: { label: "Turn In", action: "Turn in", icon: ArrowDownToLine, tone: "text-delivery", chip: "border-delivery/40 bg-delivery/10" },
} as const;

export function OperationTag({ operation }: { operation: Operation }) {
  const meta = OPERATION[operation];
  const Icon = meta.icon;
  return (
    <span
      className={[
        "inline-flex h-6 w-fit items-center gap-1 rounded-sm border px-1.5 font-mono text-[10px] font-semibold tracking-[0.08em] whitespace-nowrap uppercase",
        meta.tone,
        meta.chip,
      ].join(" ")}
    >
      <Icon className="size-3 shrink-0" aria-hidden="true" />
      {meta.label}
    </span>
  );
}

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
}

/** Full-width expand/collapse header for a display group. */
export function GroupHeader({ expanded, controls, onToggle, title, meta, aside, leading, className = "" }: GroupHeaderProps) {
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
      {aside && <span className="flex shrink-0 flex-wrap items-center justify-end gap-x-3 gap-y-0.5">{aside}</span>}
    </button>
  );
}

export function plural(count: number, word: string): string {
  return `${count} ${word}${count === 1 ? "" : "s"}`;
}
