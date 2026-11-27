import { useCallback, useEffect, useMemo, useReducer, useState, type Dispatch } from "react";
import { AlertTriangle, ArrowDownToLine, ArrowUpFromLine, LoaderCircle, MapPin, PackageCheck, RotateCw } from "lucide-react";
import { CollapsiblePanel, EmptyState, PageContainer, PageHeader, Panel } from "../components/ui";
import { describeError, listActiveContracts, toCommandError } from "../features/contracts/api";
import { capacityState, formatPercent } from "../features/contracts/summary";
import { formatScu } from "../features/contracts/scu";
import type { CommandError, ContractDetail } from "../features/contracts/types";
import type { FlowState } from "../features/gamelog/flow";
import { describeAmount, formatAmount, requirementCargo, summarizeCargo, type OperationSummary, type SummaryLine } from "../features/cargo/summary";
import { estimateBoxes, type ColorMode, type EstimatedBox } from "../features/cargo/boxes";
import { layoutCargo } from "../features/cargo/layout";
import {
  NO_HIGHLIGHT,
  activeSelection,
  highlightedLines,
  reduceHighlight,
  selectionId,
  type HighlightAction,
  type Selection,
} from "../features/cargo/highlight";
import { CargoView } from "../features/cargo/CargoView";
import { useShips } from "../features/ships/store";

type ListState = { status: "loading" } | { status: "error"; error: CommandError } | { status: "ready"; contracts: ContractDetail[] };

/** Pickup / Destination expanded state and the colour mode, kept for the session. */
const sessionOpen = { pickup: true, delivery: true };
let sessionMode: ColorMode = "destination";

/**
 * Cargo overview: Pickup and Destination summaries of the active contracts and the
 * isometric "Cargo arrangement", linked by a shared highlight.
 *
 * Layout rule: the two panels sit side by side whenever the **app width** is at least 990 CSS px,
 * and stack only below that (a width rule, never orientation, so a tall/portrait window that is
 * still >= 990 px wide keeps two columns). It is the same `min-[990px]` rule as the Contracts
 * summary. A container query on the page content did not work: at a 990 px app width the
 * content is only ~927 px wide (page padding and scrollbar gutter), so it stacked.
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

  // The box view recomputes when the active list, the ship or the colour mode changes.
  const [mode, setModeState] = useState<ColorMode>(sessionMode);
  const setMode = (m: ColorMode) => {
    sessionMode = m;
    setModeState(m);
  };
  const shipCapacity = ship?.cargoScu ?? null;
  const boxes = useMemo<EstimatedBox[]>(
    () => (list.status === "ready" ? estimateBoxes(requirementCargo(list.contracts)) : []),
    [list],
  );
  const layout = useMemo(
    () => (list.status === "ready" ? layoutCargo(boxes, { mode, shipCapacityScu: shipCapacity }) : null),
    [list.status, boxes, mode, shipCapacity],
  );

  const [highlight, dispatchHighlight] = useReducer(reduceHighlight, NO_HIGHLIGHT);
  const selection = activeSelection(highlight);
  const litLines = useMemo(() => highlightedLines(boxes, selection, mode), [boxes, selection, mode]);
  // Escape clears a pinned highlight (dialogs handle their own Escape first).
  useEffect(() => {
    if (!highlight.pinned) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !document.querySelector("dialog[open]")) dispatchHighlight({ type: "clear" });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [highlight.pinned]);
  // A new list or colour mode can make a pinned box disappear: clear it.
  useEffect(() => {
    dispatchHighlight({ type: "clear" });
  }, [list, mode]);
  const link = { lit: litLines, pinned: highlight.pinned ? selectionId(highlight.pinned) : null, dispatch: dispatchHighlight };

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
          className="grid grid-cols-1 gap-4 min-[990px]:grid-cols-[minmax(320px,2fr)_minmax(0,3fr)] min-[990px]:items-start"
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
                  link={link}
                  summary={summary.pickup}
                  open={open.pickup}
                  onOpenChange={(v) => toggle("pickup", v)}
                />
                <OperationCard
                  title="Destination"
                  link={link}
                  summary={summary.delivery}
                  open={open.delivery}
                  onOpenChange={(v) => toggle("delivery", v)}
                />
              </>
            )}
          </div>

          {list.status === "ready" ? (
            <CargoView
              layout={layout}
              boxes={boxes}
              mode={mode}
              onModeChange={setMode}
              ship={ship}
              activeScuMilli={summary?.activeScuMilli ?? null}
              highlight={highlight}
              dispatch={dispatchHighlight}
            />
          ) : (
            <Panel>
              <div className="px-4 py-3 text-sm font-semibold tracking-wide text-fg">Cargo arrangement</div>
            </Panel>
          )}
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

interface Link {
  /** Summary line keys highlighted by the current selection. */
  lit: Set<string>;
  /** `selectionId` of the pinned selection. */
  pinned: string | null;
  dispatch: Dispatch<HighlightAction>;
}

/** Mouse handlers linking a heading or line to the box view (keyboard: the legend). */
function linkHandlers(link: Link, selection: Selection) {
  return {
    onMouseEnter: () => link.dispatch({ type: "hover", selection }),
    onMouseLeave: () => link.dispatch({ type: "hover", selection: null }),
    onClick: () => link.dispatch({ type: "click", selection }),
  };
}

function OperationCard({
  title,
  summary,
  open,
  onOpenChange,
  link,
}: {
  title: string;
  summary: OperationSummary;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  link: Link;
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
              <h3
                className={[
                  "-mx-1.5 flex min-w-0 cursor-pointer items-baseline gap-2 rounded-sm px-1.5",
                  link.pinned === selectionId({ kind: "location", operation: summary.operation, location: location.locationName }) ? "bg-raised" : "hover:bg-raised/60",
                ].join(" ")}
                {...linkHandlers(link, { kind: "location", operation: summary.operation, location: location.locationName })}
              >
                <Icon className={`size-3.5 shrink-0 self-center ${tone}`} aria-hidden="true" />
                <span className="min-w-0 flex-1 text-sm font-semibold break-words text-fg">{location.locationName}</span>
                <span className="shrink-0 font-mono text-xs whitespace-nowrap text-muted" title={describeAmount(location.total)}>
                  {formatAmount(location.total)}
                </span>
              </h3>
              <ul className="mt-1 flex flex-col pl-5.5">
                {location.lines.map((line) => (
                  <Line
                    key={line.key}
                    line={line}
                    operation={summary.operation}
                    lit={link.lit.has(line.key)}
                    handlers={linkHandlers(link, { kind: "line", operation: summary.operation, location: location.locationName, commodity: line.commodityName })}
                  />
                ))}
              </ul>
            </li>
          ))}
        </ul>
      )}
    </CollapsiblePanel>
  );
}

function Line({
  line,
  operation,
  lit,
  handlers,
}: {
  line: SummaryLine;
  operation: "pickup" | "delivery";
  lit: boolean;
  handlers: ReturnType<typeof linkHandlers>;
}) {
  const refs = line.contractRefs.join(" · ");
  const done = operation === "pickup" ? "picked up" : "turned in";
  return (
    <li
      {...handlers}
      data-line-key={line.key}
      className={[
        "-mx-1.5 flex min-w-0 cursor-pointer items-baseline gap-2 rounded-sm px-1.5 py-0.5 text-sm",
        lit ? "bg-raised shadow-[inset_2px_0_0_var(--color-accent)]" : "hover:bg-raised/60",
        line.allTicked && !lit ? "opacity-55" : "",
      ].join(" ")}
    >
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
