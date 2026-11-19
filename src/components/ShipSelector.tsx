import { ChevronDown, Rocket } from "lucide-react";

/**
 * Ship selector placeholder. The bundled ship catalog is a later task, so the
 * control is intentionally disabled and holds no ship data of any kind.
 */
export function ShipSelector() {
  return (
    <div className="flex min-w-0 shrink items-center gap-3">
      <span id="ship-selector-status" className="label-tech hidden whitespace-nowrap sm:inline">
        Catalog not available yet
      </span>
      <button
        type="button"
        disabled
        aria-describedby="ship-selector-status"
        title="Ship catalog not available yet"
        className="flex h-9 w-44 min-w-0 items-center gap-2 rounded-sm border border-line bg-raised px-3 text-left text-muted disabled:cursor-not-allowed sm:w-56"
      >
        <Rocket className="size-4 shrink-0 text-dim" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate">Select ship</span>
        <ChevronDown className="size-4 shrink-0 text-dim" aria-hidden="true" />
      </button>
    </div>
  );
}
