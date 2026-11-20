import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, ArrowDownToLine, ArrowUpFromLine, Boxes, LoaderCircle, MapPin, PackageCheck, RotateCw } from "lucide-react";
import { CollapsiblePanel, EmptyState, PageContainer, PageHeader, Panel, PanelHeader, Tag } from "../components/ui";
import { describeError, listActiveContracts, toCommandError } from "../features/contracts/api";
import { capacityState, formatPercent } from "../features/contracts/summary";
import { formatScu } from "../features/contracts/scu";
import type { CommandError, ContractDetail } from "../features/contracts/types";
import type { FlowState } from "../features/gamelog/flow";
import { describeAmount, formatAmount, summarizeCargo, type OperationSummary, type SummaryLine } from "../features/cargo/summary";
import { useShips } from "../features/ships/store";

type ListState = { status: "loading" } | { status: "error"; error: CommandError } | { status: "ready"; contracts: ContractDetail[] };

/** Pickup / Destination expanded state, kept for the session (survives page switches). */
const sessionOpen = { pickup: true, delivery: true };

/**
 * Cargo overview (Task 13): Pickup and Destination summaries of the active contracts. The
 * "Cargo arrangement" panel stays a placeholder (Task 13.1).
 *
 * Layout rule: the two panels sit side by side whenever the *usable content width* is at least
 * 990 CSS px, and stack only below that. This is driven by a CSS container query on the page
 * content (not a viewport media query and not orientation), so a tall/portrait window that is
 * still >= 990 px wide keeps the two-column layout.
 */
export function CargoPage({ flow }: { flow: FlowState }) {
  const [list, setList] = useState<ListState>({ status: "loading" });
  const [open, setOpen] = useState(sessionOpen);
  const ship = useShips().selected;

  const refresh = useCallback(async () => {
    try {
      setList({ status: "ready", contracts: await listActiveContracts() });
    } catch (error) {
      setList({ status: "error", error: toCommandError(error) });
    }
  }, []);

  // First load, and again whenever contracts change anywhere (add, edit, finalize, restore,
  // ticks, Game.log completions).
  useEffect(() => {
    void refresh();
  }, [refresh, flow.listVersion]);

  const toggle = (operation: "pickup" | "delivery", value: boolean) => {
    sessionOpen[operation] = value;
    setOpen({ ...sessionOpen });
  };

  const summary = useMemo(() => (list.status === "ready" ? summarizeCargo(list.contracts) : null), [list]);

  return (
    <PageContainer wide>
      <div className="@container">
        <PageHeader
          index="02"
          title="Cargo"
          description="Pickup and destination summary with an informational cargo arrangement."
          actions={<HeaderStatus activeScuMilli={summary?.activeScuMilli ?? null} ship={ship} />}
        />

        <div
          data-testid="cargo-layout"
          className="grid grid-cols-1 gap-4 @min-[990px]:grid-cols-[minmax(320px,2fr)_minmax(0,3fr)] @min-[990px]:items-start"
        >
          <div className="flex min-w-0 flex-col gap-4">
            {list.status === "loading" && (
              <Panel>
                <div className="flex items-center justify-center gap-2 px-6 py-10 text-sm text-muted" role="status">
                  <LoaderCircle className="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
                  Loading cargo…
                </div>
              </Panel>
            )}
            {list.status === "error" && (
              <Panel>
                <div role="alert" className="flex flex-col items-center gap-3 px-6 py-8 text-center">
                  <AlertTriangle className="size-5 text-danger" aria-hidden="true" />
                  <p className="max-w-lg text-sm break-words text-muted">{describeError(list.error)}</p>
                  <button
                    type="button"
                    onClick={() => {
                      setList({ status: "loading" });
                      void refresh();
                    }}
                    className="inline-flex h-8 items-center gap-1.5 rounded-sm border border-line px-3 text-sm text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg"
                  >
                    <RotateCw className="size-3.5" aria-hidden="true" />
                    Retry
                  </button>
                </div>
              </Panel>
            )}
            {summary && (
              <>
                <OperationCard
                  title="Pickup"
                  summary={summary.pickup}
                  open={open.pickup}
                  onOpenChange={(v) => toggle("pickup", v)}
                />
                <OperationCard
                  title="Destination"
                  summary={summary.delivery}
                  open={open.delivery}
                  onOpenChange={(v) => toggle("delivery", v)}
                />
              </>
            )}
          </div>

          <Panel>
            <PanelHeader title="Cargo arrangement" meta={<Tag>Not available yet</Tag>} />
            <div className="relative px-4 py-6">
              <DeckOutline />
              <div className="mt-2">
                <EmptyState icon={Boxes} title="Nothing to visualise" compact>
                  A read-only isometric view of active cargo will appear here. Select a ship later to scale the
                  view; capacity is informational only.
                </EmptyState>
              </div>
            </div>
          </Panel>
        </div>
      </div>
    </PageContainer>
  );
}

/** "312 SCU active · C2 Hercules Starlifter 696 SCU · 45 %" (amber above 100 %, never blocking). */
function HeaderStatus({ activeScuMilli, ship }: { activeScuMilli: number | null; ship: { name: string; cargoScu: number } | null }) {
  if (activeScuMilli === null) {
    return (
      <span className="label-tech">
        <span aria-hidden="true">—</span>
        <span className="sr-only">Loading</span> SCU active
      </span>
    );
  }
  const capacity = ship ? capacityState(activeScuMilli, ship.cargoScu) : null;
  const known = capacity?.kind === "known" ? capacity : null;
  return (
    <span className="label-tech flex flex-wrap items-baseline justify-end gap-x-1.5 text-right">
      <span>{formatScu(activeScuMilli)} SCU active</span>
      {ship && known && (
        <>
          <span aria-hidden="true">·</span>
          <span className="normal-case">
            {ship.name} {ship.cargoScu.toLocaleString("en-US")} SCU
          </span>
          <span aria-hidden="true">·</span>
          <span className={known.over ? "text-accent" : "text-fg"}>{formatPercent(known.percent)}</span>
        </>
      )}
    </span>
  );
}

const EMPTY_TEXT = {
  pickup: { icon: MapPin, title: "No pickups", body: "Pickup locations and their commodities will be listed here." },
  delivery: { icon: PackageCheck, title: "No deliveries", body: "Destination locations and their commodities will be listed here." },
} as const;

function OperationCard({
  title,
  summary,
  open,
  onOpenChange,
}: {
  title: string;
  summary: OperationSummary;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const empty = EMPTY_TEXT[summary.operation];
  const Icon = summary.operation === "pickup" ? ArrowUpFromLine : ArrowDownToLine;
  const tone = summary.operation === "pickup" ? "text-pickup" : "text-delivery";
  return (
    <CollapsiblePanel
      title={title}
      open={open}
      onOpenChange={onOpenChange}
      meta={
        <span className={`font-mono text-xs whitespace-nowrap ${tone}`} title={describeAmount(summary.total)}>
          {formatAmount(summary.total)}
        </span>
      }
    >
      {summary.locations.length === 0 ? (
        <EmptyState icon={empty.icon} title={empty.title} compact>
          {empty.body}
        </EmptyState>
      ) : (
        <ul className="divide-y divide-line" aria-label={`${title} locations`}>
          {summary.locations.map((location) => (
            <li key={location.key} className="px-4 py-2.5">
              <h3 className="flex min-w-0 items-baseline gap-2">
                <Icon className={`size-3.5 shrink-0 self-center ${tone}`} aria-hidden="true" />
                <span className="min-w-0 flex-1 text-sm font-semibold break-words text-fg">{location.locationName}</span>
                <span className="shrink-0 font-mono text-xs whitespace-nowrap text-muted" title={describeAmount(location.total)}>
                  {formatAmount(location.total)}
                </span>
              </h3>
              <ul className="mt-1 flex flex-col pl-5.5">
                {location.lines.map((line) => (
                  <Line key={line.key} line={line} operation={summary.operation} />
                ))}
              </ul>
            </li>
          ))}
        </ul>
      )}
    </CollapsiblePanel>
  );
}

function Line({ line, operation }: { line: SummaryLine; operation: "pickup" | "delivery" }) {
  const refs = line.contractRefs.join(" · ");
  const done = operation === "pickup" ? "picked up" : "turned in";
  return (
    <li className={["flex min-w-0 items-baseline gap-2 py-0.5 text-sm", line.allTicked ? "opacity-55" : ""].join(" ")}>
      <span className="min-w-0 flex-1 truncate text-muted" title={line.commodityName}>
        {line.commodityName}
        {line.allTicked && <span className="sr-only"> (all {done})</span>}
      </span>
      <span className="shrink-0 truncate font-mono text-[11px] text-dim" title={`Contracts ${refs}`}>
        {refs}
      </span>
      <span className="w-24 shrink-0 text-right font-mono text-xs whitespace-nowrap text-fg" title={describeAmount(line.amount)}>
        {formatAmount(line.amount)}
      </span>
    </li>
  );
}

/** Decorative empty isometric deck grid (no cargo data). */
function DeckOutline() {
  const cols = 8;
  const rows = 5;
  const w = 36; // half-width of one cell in px (isometric 2:1)
  const h = w / 2;
  const ox = rows * w;
  const lines: string[] = [];
  for (let c = 0; c <= cols; c++) {
    lines.push(`M${ox + c * w},${c * h} L${ox + c * w - rows * w},${c * h + rows * h}`);
  }
  for (let r = 0; r <= rows; r++) {
    lines.push(`M${ox - r * w},${r * h} L${ox - r * w + cols * w},${r * h + cols * h}`);
  }
  const vw = (cols + rows) * w;
  const vh = (cols + rows) * h;
  return (
    <svg
      viewBox={`-2 -2 ${vw + 4} ${vh + 4}`}
      className="mx-auto block h-auto w-full max-w-xl text-line-strong"
      aria-hidden="true"
    >
      <path d={lines.join(" ")} fill="none" stroke="currentColor" strokeWidth="1" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
