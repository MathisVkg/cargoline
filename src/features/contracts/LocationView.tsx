import { useId } from "react";
import { ChevronRight, Square } from "lucide-react";
import { BoxText, GroupHeader, OPERATION, OperationTag, StopQuantityText, plural } from "./display";
import type { LocationGroup, ManifestEntry, OperationRow, OperationTotal } from "./grouping";
import { formatScu } from "./scu";

interface LocationViewProps {
  groups: LocationGroup[];
  isExpanded: (key: string) => boolean;
  onToggle: (key: string) => void;
}

/**
 * Operational manifest: locations as section headings (pickup locations
 * first), one thin-separated line per cargo operation. Secondary details
 * move to a second line when the content area is narrow (container query).
 */
export function LocationView({ groups, isExpanded, onToggle }: LocationViewProps) {
  return (
    <div className="@container flex flex-col gap-3" role="list" aria-label="Cargo operations by location">
      {groups.map((group) => (
        <LocationSection key={group.key} group={group} isExpanded={isExpanded} onToggle={onToggle} />
      ))}
    </div>
  );
}

// SCU | operation | commodity | action  (+ second line: route · contract · boxes)
// wide: SCU | operation | commodity | route | contract | boxes | action
const LINE =
  "grid items-center gap-x-3 grid-cols-[4.25rem_5rem_minmax(0,1fr)_1.75rem] " +
  "@min-[720px]:grid-cols-[4.25rem_5rem_minmax(0,0.9fr)_minmax(0,1.3fr)_3rem_6.5rem_1.75rem]";

function LocationSection({
  group,
  isExpanded,
  onToggle,
}: {
  group: LocationGroup;
  isExpanded: (key: string) => boolean;
  onToggle: (key: string) => void;
}) {
  const bodyId = useId();
  const expanded = isExpanded(group.key);
  const indicator = group.hasPickup ? "bg-pickup" : "bg-delivery";
  return (
    <section role="listitem" className="min-w-0">
      <h3 className="m-0 border-b border-line-strong">
        <GroupHeader
          expanded={expanded}
          controls={bodyId}
          onToggle={() => onToggle(group.key)}
          leading={
            <span
              aria-hidden="true"
              className={["size-2 shrink-0 rounded-[1px]", indicator].join(" ")}
              title={group.hasPickup ? "Has pickups" : "Turn-ins only"}
            />
          }
          title={group.locationName}
          aside={
            <>
              <span className="label-tech hidden whitespace-nowrap sm:inline">
                {plural(group.contractCount, "contract")}
              </span>
              {group.pickup && <TotalBadge operation="pickup" total={group.pickup} />}
              {group.delivery && <TotalBadge operation="delivery" total={group.delivery} />}
            </>
          }
        />
      </h3>
      <ul id={bodyId} hidden={!expanded} className="divide-y divide-line/80">
        {group.entries.map((entry) => (
          <ManifestLine
            key={entry.key}
            entry={entry}
            expanded={isExpanded(entry.key)}
            onToggle={() => onToggle(entry.key)}
          />
        ))}
      </ul>
    </section>
  );
}

function TotalBadge({ operation, total }: { operation: "pickup" | "delivery"; total: OperationTotal }) {
  const meta = OPERATION[operation];
  const known = total.scuMilli > 0 ? `${formatScu(total.scuMilli)} SCU` : "";
  const text = total.complete ? `${formatScu(total.scuMilli)} SCU` : known ? `${known} + ?` : "? SCU";
  const description = total.complete
    ? `${meta.label} total ${text}`
    : `${meta.label}: some quantities at this location are unknown${known ? `; at least ${known}` : ""}`;
  return (
    <span className={["font-mono text-xs whitespace-nowrap", meta.tone].join(" ")} title={description}>
      <span aria-hidden="true">
        {meta.label} {text}
      </span>
      <span className="sr-only">{description}</span>
    </span>
  );
}

function ManifestLine({ entry, expanded, onToggle }: { entry: ManifestEntry; expanded: boolean; onToggle: () => void }) {
  const meta = OPERATION[entry.operation];
  const combined = entry.parts.length > 1;
  const single = entry.parts[0];
  const detailsId = useId();
  return (
    <li className="min-w-0">
      <div className={`${LINE} px-2 py-1 text-sm`}>
        <span className={["text-right font-mono whitespace-nowrap", entry.quantity.kind === "unknown" ? "" : meta.tone].join(" ")}>
          <QuantityCell entry={entry} />
        </span>
        <OperationTag operation={entry.operation} />
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="min-w-0 truncate text-fg" title={entry.commodityName}>
            {entry.commodityName}
          </span>
          {combined && (
            <button
              type="button"
              aria-expanded={expanded}
              aria-controls={detailsId}
              onClick={onToggle}
              className="inline-flex shrink-0 items-center gap-0.5 rounded-sm px-1 text-[11px] text-dim hover:bg-raised hover:text-fg"
              title="Show the objectives combined in this line"
            >
              <ChevronRight className={["size-3 transition-transform duration-150", expanded ? "rotate-90" : ""].join(" ")} aria-hidden="true" />
              {entry.parts.length} objectives
            </button>
          )}
        </span>

        <button
          type="button"
          disabled
          aria-label={`${meta.action} ${entry.commodityName} for ${entry.contractRef}: not available yet`}
          title={`${meta.action}: not available yet`}
          className="grid size-7 place-items-center justify-self-end rounded-sm text-dim/60 disabled:cursor-not-allowed @min-[720px]:order-last"
        >
          <Square className="size-4" aria-hidden="true" />
        </button>

        {/* Second line when narrow; own columns when wide. */}
        <span className="col-span-3 col-start-2 flex min-w-0 flex-wrap items-center gap-x-3 text-xs text-dim @min-[720px]:contents">
          <Route operation={entry.operation} locations={entry.oppositeLocations} />
          <span className="font-mono text-muted" title={entry.contractTitle}>
            {entry.contractRef}
            <span className="sr-only"> {entry.contractTitle}</span>
          </span>
          <span className="min-w-0 truncate font-mono text-xs">
            {combined ? (
              <span className="text-dim" title="Box information is listed per objective">
                {entry.parts.some((p) => p.boxGroups.length > 0 || p.maxBoxSizeScuMilli !== null) ? "per objective" : "—"}
              </span>
            ) : (
              <BoxText boxGroups={single.boxGroups} maxBoxSizeScuMilli={single.maxBoxSizeScuMilli} />
            )}
          </span>
        </span>
      </div>

      {combined && (
        <ul id={detailsId} hidden={!expanded} className="mb-1 ml-[4.25rem] border-l border-line pl-3">
          {entry.parts.map((part) => (
            <PartLine key={part.key} part={part} />
          ))}
        </ul>
      )}
    </li>
  );
}

function QuantityCell({ entry }: { entry: ManifestEntry }) {
  if (entry.parts.length === 1) {
    return <StopQuantityText quantity={entry.quantity} requirementScuMilli={entry.parts[0].requirementScuMilli} />;
  }
  // Combined lines only ever hold known or derived quantities.
  return (
    <span title={`Combined from ${entry.parts.length} objectives`}>
      {entry.quantity.kind === "unknown" ? "?" : formatScu(entry.quantity.scuMilli)}
    </span>
  );
}

function PartLine({ part }: { part: OperationRow }) {
  return (
    <li className="flex min-w-0 flex-wrap items-baseline gap-x-3 py-0.5 text-xs text-dim">
      <span className="w-12 shrink-0 text-right font-mono text-muted">
        <StopQuantityText quantity={part.quantity} requirementScuMilli={part.requirementScuMilli} />
      </span>
      <Route operation={part.operation} locations={part.oppositeLocations} />
      <span className="font-mono">
        <BoxText boxGroups={part.boxGroups} maxBoxSizeScuMilli={part.maxBoxSizeScuMilli} />
      </span>
    </li>
  );
}

function Route({ operation, locations }: { operation: "pickup" | "delivery"; locations: string[] }) {
  const direction = operation === "pickup" ? "To" : "From";
  const [first, ...more] = locations;
  return (
    <span className="min-w-0 truncate text-muted @min-[720px]:text-sm" title={`${direction}: ${locations.join(", ")}`}>
      <span className="sr-only">{direction} </span>
      <span aria-hidden="true" className="text-dim">
        {operation === "pickup" ? "→ " : "← "}
      </span>
      {first}
      {more.length > 0 && <span className="text-dim"> +{more.length}</span>}
    </span>
  );
}
