import { useId } from "react";
import { BoxText, GroupHeader, OPERATION, ReminderCheckbox, StopQuantityText, plural } from "./display";
import { boxInfo, type ContractGroup, type ObjectiveRow, type StopEntry } from "./grouping";
import { ContractActionsMenu, type RequestEdit, type RequestFinalize, type ToggleReminder } from "./ContractActionsMenu";
import { reminderIntent } from "./reminders";
import { formatAuec, formatScu } from "./scu";

interface ContractViewProps {
  groups: ContractGroup[];
  isExpanded: (key: string) => boolean;
  onToggle: (key: string) => void;
  onRequestFinalize: RequestFinalize;
  onEditContract: RequestEdit;
  onToggleReminder: ToggleReminder;
}

/** Review mode: one expandable group per active contract, newest first. */
export function ContractView({ groups, isExpanded, onToggle, onRequestFinalize, onEditContract, onToggleReminder }: ContractViewProps) {
  return (
    <div className="@container flex flex-col gap-1" role="list" aria-label="Active contracts">
      {groups.map((group) => (
        <ContractSection
          key={group.key}
          group={group}
          expanded={isExpanded(group.key)}
          onToggle={() => onToggle(group.key)}
          onRequestFinalize={onRequestFinalize}
          onEditContract={onEditContract}
          onToggleReminder={onToggleReminder}
        />
      ))}
    </div>
  );
}

// narrow: SCU | commodity, then pickup and turn-in lists under the commodity
// wide:   SCU | commodity | pickups | turn-ins
const LINE =
  "grid gap-x-3 gap-y-1 grid-cols-[4.25rem_minmax(0,1fr)] " +
  "@min-[720px]:grid-cols-[4.25rem_minmax(0,0.8fr)_minmax(0,1.2fr)_minmax(0,1.2fr)]";

const timeFormat = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });

function ContractSection({
  group,
  expanded,
  onToggle,
  onRequestFinalize,
  onEditContract,
  onToggleReminder,
}: {
  group: ContractGroup;
  expanded: boolean;
  onToggle: () => void;
  onRequestFinalize: RequestFinalize;
  onEditContract: RequestEdit;
  onToggleReminder: ToggleReminder;
}) {
  const bodyId = useId();
  return (
    <section role="listitem" className="min-w-0 border-b border-line">
      <div className="flex min-w-0 items-center">
      <h3 className="m-0 min-w-0 flex-1">
        <GroupHeader
          expanded={expanded}
          controls={bodyId}
          onToggle={onToggle}
          leading={
            <span className="shrink-0 rounded-sm border border-line px-1.5 font-mono text-[11px] leading-5 text-muted">
              {group.contractRef}
            </span>
          }
          title={group.title}
          aside={
            <>
              <span className="font-mono text-sm whitespace-nowrap">
                <span className="text-fg">{formatScu(group.totalScuMilli)}</span> <span className="text-dim">SCU</span>
              </span>
              <span className="w-28 text-right font-mono text-sm whitespace-nowrap">
                <span className="text-fg">{formatAuec(group.rewardAuec)}</span> <span className="text-dim">aUEC</span>
              </span>
            </>
          }
        />
      </h3>
        <ContractActionsMenu
          variant="icon"
          contractId={group.contractId}
          contractRef={group.contractRef}
          title={group.title}
          onRequest={onRequestFinalize}
          onEdit={onEditContract}
        />
      </div>
      <div id={bodyId} hidden={!expanded} className="pb-1.5 pl-8">
        <p className="label-tech px-2 pb-1">
          Run {group.runNumber} · {group.runDate} · accepted {timeFormat.format(group.acceptedAt)} ·{" "}
          {plural(group.objectives.length, "objective")}
        </p>
        <ol className="divide-y divide-line/70 border-t border-line/70">
          {group.objectives.map((objective) => (
            <ObjectiveLine
              key={objective.key}
              objective={objective}
              contractRef={group.contractRef}
              onToggleReminder={onToggleReminder}
            />
          ))}
        </ol>
      </div>
    </section>
  );
}

function ObjectiveLine({
  objective,
  contractRef,
  onToggleReminder,
}: {
  objective: ObjectiveRow;
  contractRef: string;
  onToggleReminder: ToggleReminder;
}) {
  const box = boxInfo(objective.boxGroups, objective.maxBoxSizeScuMilli);
  return (
    <li className={`${LINE} px-2 py-1.5 text-sm`}>
      <span className="text-right font-mono whitespace-nowrap text-fg">
        {formatScu(objective.totalScuMilli)} <span className="text-xs text-dim">SCU</span>
      </span>
      <span className="min-w-0">
        <span className="block break-words text-fg">{objective.commodityName}</span>
        {box.kind !== "none" && (
          <span className="block font-mono text-[11px] text-dim">
            <BoxText boxGroups={objective.boxGroups} maxBoxSizeScuMilli={objective.maxBoxSizeScuMilli} />
          </span>
        )}
      </span>
      <StopList
        operation="pickup"
        stops={objective.pickups}
        requirementScuMilli={objective.totalScuMilli}
        context={`${objective.commodityName} for ${contractRef}`}
        onToggleReminder={onToggleReminder}
      />
      <StopList
        operation="delivery"
        stops={objective.deliveries}
        requirementScuMilli={objective.totalScuMilli}
        context={`${objective.commodityName} for ${contractRef}`}
        onToggleReminder={onToggleReminder}
      />
    </li>
  );
}

function StopList({
  operation,
  stops,
  requirementScuMilli,
  context,
  onToggleReminder,
}: {
  operation: "pickup" | "delivery";
  stops: StopEntry[];
  requirementScuMilli: number;
  context: string;
  onToggleReminder: ToggleReminder;
}) {
  const meta = OPERATION[operation];
  const Icon = meta.icon;
  return (
    <div className="col-start-2 min-w-0 @min-[720px]:col-start-auto">
      <ul className="flex flex-col">
        {stops.map((stop, i) => {
          const intent = reminderIntent([stop]);
          const dim = intent.state === "all" ? "opacity-55" : "";
          return (
            <li key={stop.stopId} className="flex min-w-0 items-center gap-2">
              <Icon
                className={["size-3 shrink-0", meta.tone, i === 0 ? "" : "invisible", dim].join(" ")}
                aria-hidden="true"
              />
              <span className="sr-only">{meta.label}: </span>
              <span className={["min-w-0 flex-1 break-words text-muted", dim].join(" ")}>{stop.locationName}</span>
              {stop.quantity.kind !== "derived" && (
                <span className={["shrink-0 font-mono text-xs", dim].join(" ")}>
                  <StopQuantityText quantity={stop.quantity} requirementScuMilli={requirementScuMilli} />
                </span>
              )}
              <ReminderCheckbox
                operation={operation}
                intent={intent}
                context={`${context} at ${stop.locationName}`}
                onActivate={(i) => onToggleReminder(i.stopIds, i.done)}
              />
            </li>
          );
        })}
      </ul>
    </div>
  );
}
