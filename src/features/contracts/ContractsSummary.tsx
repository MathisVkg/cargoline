import type { ReactNode } from "react";
import { formatScu } from "./scu";
import { capacityState, formatPercent, type ContractsSummary as Summary } from "./summary";

interface ContractsSummaryProps {
  /** `null` while loading or on a list error: shows "—" instead of zeros. */
  summary: Summary | null;
  /** Selected ship capacity in whole SCU; `null` until a ship catalog exists. */
  capacityScu: number | null;
}

/**
 * Operational summary strip: active contracts, total SCU, ship capacity.
 * One row at >= 990 px of app width; wraps below that.
 */
export function ContractsSummary({ summary, capacityScu }: ContractsSummaryProps) {
  return (
    <section
      aria-label="Contracts summary"
      className="mb-4 grid grid-cols-2 border-y border-line min-[990px]:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,2fr)] min-[990px]:divide-x min-[990px]:divide-line"
    >
      <Stat label="Active contracts">{summary ? summary.activeCount : <Pending />}</Stat>
      <Stat label="Total SCU" className="border-l border-line min-[990px]:border-l-0">
        {summary ? (
          <>
            {formatScu(summary.totalScuMilli)}
            <span className="ml-1.5 text-sm text-dim">SCU</span>
          </>
        ) : (
          <Pending />
        )}
      </Stat>
      <div className="col-span-2 border-t border-line px-4 py-3 min-[990px]:col-span-1 min-[990px]:border-t-0">
        <CapacityMeter activeScuMilli={summary?.totalScuMilli ?? null} capacityScu={capacityScu} />
      </div>
    </section>
  );
}

function Stat({ label, children, className = "" }: { label: string; children: ReactNode; className?: string }) {
  return (
    <div className={["min-w-0 px-4 py-3", className].join(" ")}>
      <p className="label-tech">{label}</p>
      <p className="mt-0.5 font-mono text-2xl leading-tight text-fg tabular-nums">{children}</p>
    </div>
  );
}

function Pending() {
  return (
    <span className="text-dim">
      <span aria-hidden="true">—</span>
      <span className="sr-only">Not available</span>
    </span>
  );
}

interface CapacityMeterProps {
  /** Active SCU in milli-SCU; `null` while loading or on error. */
  activeScuMilli: number | null;
  /** Ship capacity in whole SCU; `null` = no ship selected. */
  capacityScu: number | null;
}

/**
 * Informational capacity meter. Unknown capacity is a neutral state, not an
 * error. Over 100 % shows the real percentage with a full bar and a
 * restrained accent; it never warns or blocks.
 */
export function CapacityMeter({ activeScuMilli, capacityScu }: CapacityMeterProps) {
  const state = activeScuMilli === null ? null : capacityState(activeScuMilli, capacityScu);
  const known = state?.kind === "known" ? state : null;
  return (
    <div className="flex h-full min-w-0 flex-col justify-center">
      <div className="flex items-baseline justify-between gap-3">
        <p className="label-tech">Ship capacity</p>
        <p
          className={[
            "font-mono text-sm whitespace-nowrap tabular-nums",
            known?.over ? "text-accent" : known ? "text-fg" : "text-dim",
          ].join(" ")}
        >
          {state === null ? (
            "—"
          ) : known ? (
            <>
              {formatPercent(known.percent)}
              <span className="ml-1.5 text-xs text-dim">
                of {capacityScu?.toLocaleString("en-US")} SCU
              </span>
            </>
          ) : (
            "Select a ship"
          )}
        </p>
      </div>
      <div
        className="mt-2 h-1 w-full overflow-hidden rounded-full bg-line"
        {...(known
          ? {
              role: "meter",
              "aria-label": "Ship capacity used",
              "aria-valuemin": 0,
              "aria-valuemax": 100,
              "aria-valuenow": Math.round(known.percent),
              "aria-valuetext": formatPercent(known.percent),
            }
          : { "aria-hidden": true })}
      >
        {known && (
          <div
            className={["h-full rounded-full", known.over ? "bg-accent" : "bg-muted"].join(" ")}
            style={{ width: `${known.fill * 100}%` }}
          />
        )}
      </div>
    </div>
  );
}
