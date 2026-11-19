import { useId, useMemo, type ReactNode } from "react";
import { ArrowDownToLine, ArrowUpFromLine, MapPin, Package, Plus, Trash2, X } from "lucide-react";
import {
  emptyRequirement,
  emptyStop,
  type ContractDraft,
  type RequirementDraft,
  type StopDraft,
  type StopKind,
} from "./draft";
import { CatalogCombobox } from "./CatalogCombobox";
import {
  fromNames,
  fromReferenceCommodity,
  fromReferenceLocation,
  fromUserLocations,
  mergeSuggestions,
  type Suggestion,
} from "./catalogSearch";
import type { LocationEntry, ReferenceCommodity, ReferenceLocation } from "./types";

interface ContractFormProps {
  draft: ContractDraft;
  onChange: (draft: ContractDraft) => void;
  /** Validation messages keyed by input path. */
  issues: Map<string, string>;
  /** Bundled reference lists (suggestions; their spelling wins in the dropdown). */
  referenceLocations?: readonly ReferenceLocation[];
  referenceCommodities?: readonly ReferenceCommodity[];
  /** The user's own entries: location catalog and commodities used in saved contracts. */
  userLocations?: readonly LocationEntry[];
  usedCommodities?: readonly string[];
  disabled?: boolean;
}

const NONE: readonly never[] = [];

/**
 * Controlled contract editor. Knows nothing about how the draft was
 * populated or saved, so it can be reused for OCR review later.
 */
export function ContractForm({
  draft,
  onChange,
  issues,
  referenceLocations = NONE,
  referenceCommodities = NONE,
  userLocations = NONE,
  usedCommodities = NONE,
  disabled = false,
}: ContractFormProps) {
  const headingId = useId();
  // Recompute suggestions only when a catalog or the typed names change.
  const locationKey = draft.requirements
    .flatMap((r) => [...r.pickups, ...r.deliveries])
    .map((s) => s.locationName)
    .join("\u0000");
  const commodityKey = draft.requirements.map((r) => r.commodityName).join("\u0000");
  const bundledLocations = useMemo(() => referenceLocations.map(fromReferenceLocation), [referenceLocations]);
  const bundledCommodities = useMemo(() => referenceCommodities.map(fromReferenceCommodity), [referenceCommodities]);
  const locationSuggestions = useMemo(
    () => mergeSuggestions(bundledLocations, fromUserLocations(userLocations), fromNames(locationKey.split("\u0000"))),
    [bundledLocations, userLocations, locationKey],
  );
  const commoditySuggestions = useMemo(
    () => mergeSuggestions(bundledCommodities, fromNames(usedCommodities, true), fromNames(commodityKey.split("\u0000"))),
    [bundledCommodities, usedCommodities, commodityKey],
  );

  const setRequirement = (index: number, next: RequirementDraft) =>
    onChange({ ...draft, requirements: draft.requirements.map((r, i) => (i === index ? next : r)) });

  return (
    <fieldset disabled={disabled} className="@container flex min-w-0 flex-col gap-4">
      <div className="grid gap-3 @min-[560px]:grid-cols-[minmax(0,1fr)_11rem]">
        <TextField
          label="Contract title"
          value={draft.title}
          onChange={(title) => onChange({ ...draft, title })}
          error={issues.get("title")}
          placeholder="e.g. Small Cargo Haul"
          autoFocus
        />
        <TextField
          label="Reward (aUEC)"
          value={draft.rewardAuec}
          onChange={(rewardAuec) => onChange({ ...draft, rewardAuec })}
          error={issues.get("rewardAuec")}
          placeholder="e.g. 48250"
          inputMode="numeric"
        />
      </div>

      <section aria-labelledby={headingId} className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 id={headingId} className="label-tech">
            Cargo objectives <span className="text-dim">· {draft.requirements.length}</span>
            <span className="ml-2 normal-case tracking-normal text-dim">location SCU optional — leave empty if unknown</span>
          </h3>
          <GhostButton
            icon={Plus}
            onClick={() => onChange({ ...draft, requirements: [...draft.requirements, emptyRequirement()] })}
          >
            Add objective
          </GhostButton>
        </div>
        <FieldError message={issues.get("requirements")} />
        <ol className="flex flex-col gap-2">
          {draft.requirements.map((req, i) => (
            <RequirementEditor
              key={req.key}
              index={i}
              requirement={req}
              issues={issues}
              locations={locationSuggestions}
              commodities={commoditySuggestions}
              canRemove={draft.requirements.length > 1}
              onChange={(next) => setRequirement(i, next)}
              onRemove={() => onChange({ ...draft, requirements: draft.requirements.filter((_, j) => j !== i) })}
            />
          ))}
        </ol>
      </section>
    </fieldset>
  );
}

interface RequirementEditorProps {
  index: number;
  requirement: RequirementDraft;
  issues: Map<string, string>;
  locations: readonly Suggestion[];
  commodities: readonly Suggestion[];
  canRemove: boolean;
  onChange: (next: RequirementDraft) => void;
  onRemove: () => void;
}

function RequirementEditor({
  index,
  requirement: req,
  issues,
  locations,
  commodities,
  canRemove,
  onChange,
  onRemove,
}: RequirementEditorProps) {
  const path = `requirements.${index}`;
  const number = String(index + 1).padStart(2, "0");
  const label = `Objective ${number}`;
  return (
    <li className="rounded-sm border border-line bg-canvas/40 p-2.5" aria-label={label}>
      <div className="grid items-start gap-2 grid-cols-[1.5rem_minmax(0,1fr)_2rem] @min-[560px]:grid-cols-[1.5rem_minmax(0,1fr)_6.5rem_7.5rem_2rem]">
        <span className="pt-5 font-mono text-xs text-accent" aria-hidden="true">
          {number}
        </span>
        <CatalogCombobox
          label="Commodity"
          showLabel
          value={req.commodityName}
          onChange={(commodityName) => onChange({ ...req, commodityName })}
          suggestions={commodities}
          noun="commodity"
          error={issues.get(`${path}.commodityName`)}
          placeholder="Search or add commodity"
          icon={Package}
        />
        <div className="col-start-2 grid grid-cols-2 gap-2 @min-[560px]:contents">
          <TextField
            label="Total SCU"
            value={req.quantityScu}
            onChange={(quantityScu) => onChange({ ...req, quantityScu })}
            error={issues.get(`${path}.quantityScu`)}
            placeholder="e.g. 12"
            inputMode="decimal"
          />
          <TextField
            label="Max box SCU"
            optional
            value={req.maxBoxSizeScu}
            onChange={(maxBoxSizeScu) => onChange({ ...req, maxBoxSizeScu })}
            error={issues.get(`${path}.maxBoxSizeScu`)}
            placeholder="e.g. 8"
            inputMode="decimal"
          />
        </div>
        <div className="col-start-3 row-start-1 pt-4 @min-[560px]:col-start-auto @min-[560px]:row-start-auto">
          <IconButton icon={Trash2} label={`Remove ${label}`} onClick={onRemove} disabled={!canRemove} />
        </div>
      </div>

      <div className="mt-2 grid gap-3 pl-8 @min-[640px]:grid-cols-2">
        {(["pickups", "deliveries"] as const).map((kind) => (
          <StopList
            key={kind}
            kind={kind}
            stops={req[kind]}
            path={`${path}.${kind}`}
            objectiveLabel={label}
            issues={issues}
            suggestions={locations}
            onChange={(stops) => onChange({ ...req, [kind]: stops })}
          />
        ))}
      </div>
    </li>
  );
}

const STOP_KIND = {
  pickups: { title: "Pickup", noun: "pickup", icon: ArrowUpFromLine, tone: "text-pickup", rail: "border-pickup/60" },
  deliveries: { title: "Delivery", noun: "delivery", icon: ArrowDownToLine, tone: "text-delivery", rail: "border-delivery/60" },
} as const;

interface StopListProps {
  kind: StopKind;
  stops: StopDraft[];
  path: string;
  objectiveLabel: string;
  issues: Map<string, string>;
  suggestions: readonly Suggestion[];
  onChange: (stops: StopDraft[]) => void;
}

function StopList({ kind, stops, path, objectiveLabel, issues, suggestions, onChange }: StopListProps) {
  const meta = STOP_KIND[kind];
  const Icon = meta.icon;
  const update = (i: number, next: StopDraft) => onChange(stops.map((s, j) => (j === i ? next : s)));
  return (
    <div className={["flex min-w-0 flex-col gap-1.5 border-l-2 pl-2.5", meta.rail].join(" ")}>
      <div className="flex items-center justify-between gap-2">
        <span className={["flex items-center gap-1.5 text-[11px] font-semibold tracking-wider uppercase", meta.tone].join(" ")}>
          <Icon className="size-3.5 shrink-0" aria-hidden="true" />
          {meta.title}
        </span>
        <GhostButton icon={Plus} onClick={() => onChange([...stops, emptyStop()])}>
          Add {meta.noun}
        </GhostButton>
      </div>
      <FieldError message={issues.get(path)} />
      <ul className="flex flex-col gap-1.5">
        {stops.map((stop, i) => (
          <li key={stop.key} className="flex items-start gap-1.5">
            <div className="min-w-0 flex-1">
              <CatalogCombobox
                label={`${meta.title} location ${i + 1} of ${objectiveLabel}`}
                value={stop.locationName}
                onChange={(locationName) => update(i, { ...stop, locationName })}
                suggestions={suggestions}
                noun="location"
                icon={MapPin}
                error={issues.get(`${path}.${i}.locationName`)}
                placeholder={`Search or add ${meta.noun} location`}
                tone={meta.tone}
              />
            </div>
            <div className="w-16 shrink-0">
              <TextField
                label={`${meta.title} ${i + 1} SCU`}
                hideLabel
                value={stop.quantityScu}
                onChange={(quantityScu) => update(i, { ...stop, quantityScu })}
                error={issues.get(`${path}.${i}.quantityScu`)}
                placeholder="SCU"
                inputMode="decimal"
              />
            </div>
            <IconButton
              icon={X}
              label={`Remove ${meta.noun} location ${i + 1} from ${objectiveLabel}`}
              onClick={() => onChange(stops.filter((_, j) => j !== i))}
              disabled={stops.length <= 1}
            />
          </li>
        ))}
      </ul>
    </div>
  );
}

/* ---------- form primitives ---------- */

interface TextFieldProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  error?: string;
  placeholder?: string;
  inputMode?: "text" | "numeric" | "decimal";
  optional?: boolean;
  hideLabel?: boolean;
  autoFocus?: boolean;
}

function TextField({ label, value, onChange, error, placeholder, inputMode, optional, hideLabel, autoFocus }: TextFieldProps) {
  const id = useId();
  const errorId = `${id}-error`;
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <label htmlFor={id} className={hideLabel ? "sr-only" : "text-[11px] font-medium text-muted"}>
        {label}
        {optional && <span className="font-normal text-dim"> · optional</span>}
      </label>
      <input
        id={id}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        inputMode={inputMode}
        autoFocus={autoFocus}
        autoComplete="off"
        spellCheck={false}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errorId : undefined}
        className={[
          "h-8 w-full min-w-0 rounded-sm border bg-raised px-2 text-sm text-fg placeholder:text-placeholder",
          "transition-colors duration-150 focus-visible:outline-offset-0 disabled:opacity-60",
          error ? "border-danger" : "border-line hover:border-line-strong",
        ].join(" ")}
      />
      {error && (
        <p id={errorId} className="text-xs break-words text-danger">
          {error}
        </p>
      )}
    </div>
  );
}

function FieldError({ message }: { message?: string }) {
  if (!message) return null;
  return <p className="text-xs break-words text-danger">{message}</p>;
}

function GhostButton({ icon: Icon, children, onClick }: { icon: typeof Plus; children: ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex h-7 items-center gap-1 rounded-sm px-1.5 text-xs font-medium text-muted transition-colors duration-150 hover:bg-raised hover:text-fg disabled:opacity-50"
    >
      <Icon className="size-3.5" aria-hidden="true" />
      {children}
    </button>
  );
}

function IconButton({
  icon: Icon,
  label,
  onClick,
  disabled,
}: {
  icon: typeof Plus;
  label: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      title={label}
      className="grid size-8 shrink-0 place-items-center rounded-sm text-dim transition-colors duration-150 hover:bg-raised hover:text-fg disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent"
    >
      <Icon className="size-4" aria-hidden="true" />
    </button>
  );
}
