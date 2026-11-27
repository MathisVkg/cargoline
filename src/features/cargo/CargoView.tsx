import { memo, useMemo, useRef, useState, type Dispatch } from "react";
import { Boxes, TriangleAlert } from "lucide-react";
import { EmptyState, Panel } from "../../components/ui";
import { formatScu } from "../contracts/scu";
import { capacityState, formatPercent } from "../contracts/summary";
import type { ShipEntry } from "../ships/types";
import { colorKey, shade, type ColorMode, type EstimatedBox, type LegendEntry } from "./boxes";
import { ZOOM_MARGIN, drawnPlatforms, faces, project, viewBoxFor, type Layout, type Placed, type PlatformArea } from "./layout";
import { activeSelection, highlightedBoxes, selectionId, type HighlightAction, type HighlightState } from "./highlight";

const UNIT = 10;

interface Tip {
  x: number;
  y: number;
  item: Placed;
}

/**
 * "Cargo arrangement": a read-only, stylised isometric view of all active cargo.
 * Illustrative only: it never claims the packing fits the real ship and never changes data.
 */
export function CargoView({
  layout,
  boxes,
  mode,
  onModeChange,
  ship,
  activeScuMilli,
  highlight,
  dispatch,
}: {
  layout: Layout | null;
  boxes: readonly EstimatedBox[];
  mode: ColorMode;
  onModeChange: (mode: ColorMode) => void;
  ship: ShipEntry | null;
  activeScuMilli: number | null;
  highlight: HighlightState;
  dispatch: Dispatch<HighlightAction>;
}) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const [tip, setTip] = useState<Tip | null>(null);
  const [focused, setFocused] = useState<LegendEntry | null>(null);
  const selection = activeSelection(highlight);
  const lit = useMemo(() => highlightedBoxes(boxes, selection, mode), [boxes, selection, mode]);
  // Block mode: a block is lit when any of its boxes is.
  const litKeys = useMemo(() => new Set(boxes.filter((b) => lit.has(b.id)).map((b) => colorKey(b, mode))), [boxes, lit, mode]);
  const colors = useMemo(() => new Map(layout?.legend.map((e) => [e.key, e.color]) ?? []), [layout]);
  const estimated = boxes.some((b) => b.quality === "estimated");
  const totalMilli = boxes.reduce((s, b) => s + b.scuMilli, 0);

  const showTip = (event: React.MouseEvent, item: Placed) => {
    const rect = wrapRef.current?.getBoundingClientRect();
    if (rect) setTip({ x: event.clientX - rect.left, y: event.clientY - rect.top, item });
  };

  return (
    <Panel>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-line px-4 py-3">
        <h2 className="m-0 min-w-0 flex-1 text-sm font-semibold tracking-wide text-fg">Cargo arrangement</h2>
        {estimated && (
          <span
            className="rounded-sm border border-line-strong px-1.5 font-mono text-[10px] leading-5 tracking-wider text-muted uppercase"
            title="Box sizes are estimated from the cargo, the max box size and the ship's largest container."
          >
            Estimated boxes
          </span>
        )}
        <div role="group" aria-label="Colour boxes by" className="inline-flex rounded-sm border border-line bg-surface p-0.5">
          {(["destination", "commodity"] as const).map((m) => (
            <button
              key={m}
              type="button"
              aria-pressed={mode === m}
              onClick={() => onModeChange(m)}
              className={[
                "h-7 rounded-[2px] px-2.5 text-xs capitalize whitespace-nowrap transition-colors duration-150 focus-visible:outline-offset-0",
                mode === m ? "bg-raised font-medium text-fg shadow-[inset_0_-2px_0_var(--color-accent)]" : "text-dim hover:text-muted",
              ].join(" ")}
            >
              {m}
            </button>
          ))}
        </div>
      </div>

      {!layout || boxes.length === 0 ? (
        <div className="px-4 py-6">
          <EmptyState icon={Boxes} title="Nothing to visualise" compact>
            A read-only isometric view of active cargo appears here when there are active contracts.
          </EmptyState>
        </div>
      ) : (
        <div className="px-3 pt-3 pb-4">
          <div ref={wrapRef} className="relative" onMouseLeave={() => setTip(null)}>
            <IsoSvg
              layout={layout}
              colors={colors}
              lit={lit}
              litKeys={litKeys}
              dimOthers={selection !== null}
              pinnedId={highlight.pinned ? selectionId(highlight.pinned) : null}
              label={`Isometric view: ${layout.mode === "blocks" ? `${layout.legend.length} blocks` : `${boxes.length} boxes`}, ${formatScu(totalMilli)} SCU, coloured by ${mode}.`}
              onEnter={(e, item) => {
                showTip(e, item);
                if (item.box) dispatch({ type: "hover", selection: { kind: "box", id: item.box.id } });
                else dispatch({ type: "hover", selection: { kind: "group", key: item.colorKey } });
              }}
              onMove={showTip}
              onLeave={() => {
                setTip(null);
                dispatch({ type: "hover", selection: null });
              }}
              onClick={(item) =>
                dispatch({ type: "click", selection: item.box ? { kind: "box", id: item.box.id } : { kind: "group", key: item.colorKey } })
              }
            />
            {layout.zoom && <PlatformOverview layout={layout} />}
            {tip && <Tooltip tip={tip} legend={layout.legend} />}
          </div>

          {layout.mode === "blocks" && (
            <p className="mt-2 text-xs text-muted">Simplified view — very large number of boxes. Each colour is one block of its volume.</p>
          )}
          {layout.zoom && (
            <p className="mt-2 text-xs text-dim">
              Zoomed to the cargo: the platform continues beyond the dashed edges (overview top right).
            </p>
          )}
          {layout.overflowScuMilli > 0 && ship && (
            <p className="mt-2 flex items-start gap-1.5 text-xs text-accent">
              <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
              Above the ship's capacity: {formatScu(layout.overflowScuMilli)} SCU on the overflow platform. Nothing is blocked.
            </p>
          )}
          <ShipLine ship={ship} activeScuMilli={activeScuMilli} />

          <ul aria-label="Legend" className="mt-3 flex flex-wrap gap-1.5">
            {layout.legend.map((entry) => {
              const id = selectionId({ kind: "group", key: entry.key });
              const pinned = highlight.pinned !== null && selectionId(highlight.pinned) === id;
              return (
                <li key={entry.key}>
                  <button
                    type="button"
                    aria-pressed={pinned}
                    onMouseEnter={() => dispatch({ type: "hover", selection: { kind: "group", key: entry.key } })}
                    onMouseLeave={() => dispatch({ type: "hover", selection: null })}
                    onFocus={() => {
                      setFocused(entry);
                      dispatch({ type: "hover", selection: { kind: "group", key: entry.key } });
                    }}
                    onBlur={() => {
                      setFocused(null);
                      dispatch({ type: "hover", selection: null });
                    }}
                    onClick={() => dispatch({ type: "click", selection: { kind: "group", key: entry.key } })}
                    className={[
                      "inline-flex h-7 max-w-full items-center gap-1.5 rounded-sm border px-2 text-xs transition-colors duration-150",
                      pinned ? "border-line-strong bg-raised text-fg" : "border-line text-muted hover:border-line-strong hover:text-fg",
                    ].join(" ")}
                  >
                    <span className="size-2.5 shrink-0 rounded-[2px]" style={{ background: entry.color }} aria-hidden="true" />
                    <span className="min-w-0 truncate">{entry.label}</span>
                    <span className="shrink-0 font-mono text-dim">{formatScu(entry.scuMilli)} SCU</span>
                  </button>
                </li>
              );
            })}
          </ul>
          <p className="mt-2 min-h-4 text-[11px] text-dim" aria-live="polite">
            {focused
              ? `${focused.label}: ${formatScu(focused.scuMilli)} SCU in ${focused.boxes} box${focused.boxes === 1 ? "" : "es"}. Enter pins the highlight; Escape clears it.`
              : "Illustrative only: not the ship's real cargo grid. Hover or click to highlight; Escape clears."}
          </p>
        </div>
      )}
    </Panel>
  );
}

function ShipLine({ ship, activeScuMilli }: { ship: ShipEntry | null; activeScuMilli: number | null }) {
  if (!ship) return <p className="mt-2 text-xs text-dim">Select a ship to scale the platform.</p>;
  const state = activeScuMilli === null ? null : capacityState(activeScuMilli, ship.cargoScu);
  const known = state?.kind === "known" ? state : null;
  return (
    <p className="mt-2 text-xs text-muted">
      {ship.name} · {ship.cargoScu.toLocaleString("en-US")} SCU
      {known && (
        <>
          {" · "}
          <span className={known.over ? "text-accent" : "text-fg"}>{formatPercent(known.percent)} used</span>
        </>
      )}
    </p>
  );
}

const pts = (list: [number, number, number][]) =>
  list.map(([x, y, z]) => project(x, y, z, UNIT).map((v) => v.toFixed(2)).join(",")).join(" ");

/**
 * One platform's floor: grid and outline. The overflow platform is amber (outline and tint) with
 * no text on it: a label overlapped the boxes. The note under the view gives its SCU.
 */
function PlatformFloor({ platform: p, zoomed }: { platform: PlatformArea; zoomed: boolean }) {
  const x1 = p.x + p.width;
  const grid: string[] = [];
  const step = p.width > 40 ? 4 : 2;
  const line = (a: [number, number], b: [number, number]) => `M${a[0].toFixed(1)},${a[1].toFixed(1)}L${b[0].toFixed(1)},${b[1].toFixed(1)}`;
  for (let x = p.x; x <= x1 + 1e-9; x += step) grid.push(line(project(x, 0, 0, UNIT), project(x, p.depth, 0, UNIT)));
  for (let y = 0; y <= p.depth; y += step) grid.push(line(project(p.x, y, 0, UNIT), project(x1, y, 0, UNIT)));
  const floor = pts([[p.x, 0, 0], [x1, 0, 0], [x1, p.depth, 0], [p.x, p.depth, 0]]);
  if (p.kind === "overflow") {
    return (
      <g data-platform="overflow">
        <polygon points={floor} fill="var(--color-accent)" fillOpacity={0.12} stroke="var(--color-accent)" strokeOpacity={0.85} strokeWidth={1.4} vectorEffect="non-scaling-stroke" />
        <path d={grid.join("")} stroke="var(--color-accent)" strokeOpacity={0.18} fill="none" vectorEffect="non-scaling-stroke" />
      </g>
    );
  }
  return (
    <g>
      <polygon points={floor} className="fill-raised stroke-line-strong" strokeWidth={1} vectorEffect="non-scaling-stroke" />
      <path d={grid.join("")} className="stroke-line" strokeWidth={1} fill="none" vectorEffect="non-scaling-stroke" />
      {zoomed && (
        // The platform continues beyond the zoomed region: dashed far edges.
        <polyline
          points={pts([[x1, 0, 0], [x1, p.depth, 0], [p.x, p.depth, 0]])}
          fill="none"
          className="stroke-muted"
          strokeDasharray="5 4"
          strokeWidth={1.2}
          vectorEffect="non-scaling-stroke"
        />
      )}
    </g>
  );
}

/** Top-down overview of the whole main platform with the cargo footprint, shown when zoomed. */
function PlatformOverview({ layout }: { layout: Layout }) {
  const main = layout.platforms[0];
  const scale = 84 / main.width;
  const w = main.width * scale;
  const h = main.depth * scale;
  const cw = Math.min(main.width, layout.cargoFootprint.width) * scale;
  const ch = Math.min(main.depth, layout.cargoFootprint.depth) * scale;
  const used = Math.round((100 * layout.cargoFootprint.width * layout.cargoFootprint.depth) / (main.width * main.depth));
  return (
    <figure className="pointer-events-none absolute top-1 right-1 m-0 rounded-sm border border-line bg-surface/90 px-2 py-1.5">
      <svg width={w} height={h} viewBox={`-1 -1 ${w + 2} ${h + 2}`} aria-hidden="true" className="block">
        <rect x={0} y={0} width={w} height={h} className="fill-raised stroke-line-strong" strokeWidth={1} />
        {/* Front-left corner = bottom-left in this top-down sketch. */}
        <rect x={0} y={h - ch} width={cw} height={ch} fill="var(--color-fg)" fillOpacity={0.55} />
        <rect x={0} y={h - ch} width={Math.min(w, cw + ZOOM_MARGIN * scale)} height={Math.min(h, ch + ZOOM_MARGIN * scale)} fill="none" className="stroke-muted" strokeDasharray="2 2" strokeWidth={1} />
      </svg>
      <figcaption className="mt-1 text-[10px] leading-tight text-dim">
        Platform {main.width} × {main.depth}
        <br />
        cargo floor {used} % · zoomed
      </figcaption>
    </figure>
  );
}

/** The SVG is memoised on layout and highlight so hovering stays cheap. */
const IsoSvg = memo(function IsoSvg({
  layout,
  colors,
  lit,
  litKeys,
  dimOthers,
  pinnedId,
  label,
  onEnter,
  onMove,
  onLeave,
  onClick,
}: {
  layout: Layout;
  colors: Map<string, string>;
  lit: Set<string>;
  litKeys: Set<string>;
  dimOthers: boolean;
  pinnedId: string | null;
  label: string;
  onEnter: (e: React.MouseEvent, item: Placed) => void;
  onMove: (e: React.MouseEvent, item: Placed) => void;
  onLeave: () => void;
  onClick: (item: Placed) => void;
}) {
  const viewBox = viewBoxFor(layout, UNIT);
  return (
    <svg viewBox={viewBox} role="img" aria-label={label} className="block h-auto w-full select-none">
      {drawnPlatforms(layout).map((p) => (
        <PlatformFloor key={p.kind} platform={p} zoomed={p.kind === "main" && layout.zoom !== null} />
      ))}
      {layout.items.map((item) => {
        const color = colors.get(item.colorKey) ?? "#8b93a1";
        const id = item.box ? `box:${item.box.id}` : `group:${item.colorKey}`;
        const on = item.box ? lit.has(item.box.id) : litKeys.has(item.colorKey);
        const dim = dimOthers && !on;
        const f = faces(item, UNIT);
        const strong = pinnedId === id || (dimOthers && on);
        return (
          <g
            key={item.id}
            opacity={dim ? 0.16 : 1}
            className="cursor-pointer motion-safe:transition-opacity motion-safe:duration-150"
            onMouseEnter={(e) => onEnter(e, item)}
            onMouseMove={(e) => onMove(e, item)}
            onMouseLeave={onLeave}
            onClick={() => onClick(item)}
          >
            <polygon points={f.side} fill={shade(color, 0.58)} />
            <polygon points={f.front} fill={shade(color, 0.78)} />
            <polygon points={f.top} fill={shade(color, 1.12)} />
            <g
              fill="none"
              // Overflow boxes keep amber edges: the floor tint alone is hidden under the boxes.
              stroke={strong ? "#f4f6f8" : item.overflow ? "var(--color-accent)" : "#0a0c0f"}
              strokeOpacity={strong ? 0.95 : item.overflow ? 0.9 : 0.55}
              strokeWidth={strong ? 1.6 : 0.8}
              strokeDasharray={item.box?.partial ? "3 2" : undefined}
              vectorEffect="non-scaling-stroke"
              strokeLinejoin="round"
            >
              <polygon points={f.side} />
              <polygon points={f.front} />
              <polygon points={f.top} />
            </g>
            {/* Thin blocks would overlap their neighbours' labels: their SCU is in the legend and tooltip. */}
            {item.blockScuMilli !== null && item.w >= 2 && <BlockLabel item={item} />}
          </g>
        );
      })}
    </svg>
  );
});

function BlockLabel({ item }: { item: Placed }) {
  const [x, y] = project(item.x + item.w / 2, item.y + item.d / 2, item.z + item.h, UNIT);
  return (
    <text x={x} y={y} textAnchor="middle" dominantBaseline="middle" className="fill-fg font-mono" fontSize={UNIT * 1.1} style={{ paintOrder: "stroke" }} stroke="#0a0c0f" strokeWidth={3}>
      {formatScu(item.blockScuMilli ?? 0)} SCU
    </text>
  );
}

function Tooltip({ tip, legend }: { tip: Tip; legend: LegendEntry[] }) {
  const { item } = tip;
  const box = item.box;
  const entry = legend.find((e) => e.key === item.colorKey);
  return (
    <div
      role="tooltip"
      className="pointer-events-none absolute z-10 w-60 rounded-sm border border-line-strong bg-surface px-3 py-2 text-xs shadow-xl"
      style={{ left: Math.max(0, tip.x + 14), top: tip.y + 14, transform: tip.x > 260 ? "translateX(calc(-100% - 28px))" : undefined }}
    >
      {box ? (
        <>
          <p className="font-semibold text-fg">
            {box.commodityName} · {formatScu(box.scuMilli)} SCU{box.partial ? " (part box)" : ""}
          </p>
          <p className="text-dim">
            {box.contractRef} · {box.sizeScu} SCU container · {box.quality === "exact" ? "from contract" : "estimated size"}
          </p>
          <p className="mt-1 text-muted">
            <span className="text-pickup">From</span> {box.pickups.join(", ")}
          </p>
          <p className="text-muted">
            <span className="text-delivery">To</span>{" "}
            {box.destination ?? `Several destinations (${box.destinations.join(", ")})`}
          </p>
        </>
      ) : (
        <>
          <p className="font-semibold text-fg">{entry?.label}</p>
          <p className="text-muted">
            {formatScu(item.blockScuMilli ?? 0)} SCU in {entry?.boxes} boxes (simplified)
          </p>
        </>
      )}
    </div>
  );
}
