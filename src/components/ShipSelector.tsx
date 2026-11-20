import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { Check, ChevronDown, Rocket, Search, Star } from "lucide-react";
import { shipOptions } from "../features/ships/model";
import { clearShipError, loadShips, selectShip, toggleFavorite, useShips } from "../features/ships/store";
import type { ShipEntry } from "../features/ships/types";

interface Placement {
  top: number;
  right: number;
}

/**
 * Top-bar ship selector (Task 11). Informational only: the selected ship gives the Contracts
 * summary its capacity; it is never linked to contracts or runs.
 *
 * Popover pattern: the trigger opens a small dialog with a search field and the list (favourites
 * first). ArrowDown / ArrowUp move between ships, Enter or a click selects, the star toggles a
 * favourite without selecting, Escape closes and returns focus to the trigger.
 */
export function ShipSelector() {
  const id = useId();
  const { loaded, ships, selected, error } = useShips();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [placement, setPlacement] = useState<Placement | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void loadShips();
  }, []);

  const close = (refocus: boolean) => {
    setOpen(false);
    setQuery("");
    if (refocus) triggerRef.current?.focus();
  };

  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const rect = triggerRef.current?.getBoundingClientRect();
      if (rect) setPlacement({ top: rect.bottom + 4, right: Math.max(8, window.innerWidth - rect.right) });
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open]);

  useEffect(() => {
    if (open && placement) searchRef.current?.focus();
  }, [open, placement]);

  // A press outside the trigger and the panel closes it (focus stays where the user clicked).
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!panelRef.current?.contains(target) && !triggerRef.current?.contains(target)) close(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  const { favorites, others } = shipOptions(ships, query);
  const total = favorites.length + others.length;

  const choose = (shipId: string | null) => {
    void selectShip(shipId);
    close(true);
  };

  const shipButtons = () => Array.from(panelRef.current?.querySelectorAll<HTMLElement>("[data-ship-option]") ?? []);
  const moveFocus = (from: HTMLElement | null, step: 1 | -1 | "first" | "last") => {
    const items = shipButtons();
    if (items.length === 0) return;
    const index = from ? items.indexOf(from) : -1;
    const next =
      step === "first" ? 0 : step === "last" ? items.length - 1 : index === -1 ? 0 : index + step;
    if (next < 0) searchRef.current?.focus();
    else items[Math.min(next, items.length - 1)]?.focus();
  };

  const onPanelKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    const onOption = target.hasAttribute("data-ship-option");
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close(true);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      moveFocus(onOption ? target : null, 1);
    } else if (event.key === "ArrowUp" && onOption) {
      event.preventDefault();
      moveFocus(target, -1);
    } else if (event.key === "Home" && onOption) {
      event.preventDefault();
      moveFocus(null, "first");
    } else if (event.key === "End" && onOption) {
      event.preventDefault();
      moveFocus(null, "last");
    } else if (event.key === "Enter" && target === searchRef.current) {
      // Enter in the search field picks the only match.
      event.preventDefault();
      const only = total === 1 ? (favorites[0] ?? others[0]) : null;
      if (only) choose(only.id);
    }
  };

  const label = selected ? selected.name : "Select ship";
  return (
    <div className="flex min-w-0 shrink items-center gap-3">
      {error && (
        <span role="status" className="hidden max-w-48 truncate text-xs text-danger lg:inline" title={error}>
          {error}
        </span>
      )}
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? `${id}-panel` : undefined}
        aria-label={selected ? `Ship: ${selected.name}, ${selected.cargoScu} SCU. Change ship` : "Select ship"}
        disabled={!loaded}
        onClick={() => (open ? close(false) : (clearShipError(), setOpen(true)))}
        className="flex h-9 w-44 min-w-0 items-center gap-2 rounded-sm border border-line bg-raised px-3 text-left transition-colors duration-150 hover:border-line-strong disabled:cursor-wait aria-expanded:border-line-strong sm:w-60"
      >
        <Rocket className={["size-4 shrink-0", selected ? "text-accent" : "text-dim"].join(" ")} aria-hidden="true" />
        <span className={["min-w-0 flex-1 truncate text-sm", selected ? "text-fg" : "text-muted"].join(" ")}>{label}</span>
        {selected && (
          <span className="hidden shrink-0 font-mono text-xs text-dim tabular-nums sm:inline">
            {selected.cargoScu.toLocaleString("en-US")} SCU
          </span>
        )}
        <ChevronDown className="size-4 shrink-0 text-dim" aria-hidden="true" />
      </button>

      {open && placement && (
        <div
          ref={panelRef}
          id={`${id}-panel`}
          role="dialog"
          aria-label="Select ship"
          onKeyDown={onPanelKeyDown}
          style={{ top: placement.top, right: placement.right }}
          className="fixed z-50 flex max-h-[min(30rem,calc(100vh-6rem))] w-[min(22rem,calc(100vw-1rem))] flex-col overflow-hidden rounded-sm border border-line-strong bg-surface shadow-2xl"
        >
          <div className="flex items-center gap-2 border-b border-line px-3">
            <Search className="size-4 shrink-0 text-dim" aria-hidden="true" />
            <input
              ref={searchRef}
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search name or manufacturer"
              aria-label="Search ships by name, manufacturer or code"
              aria-describedby={`${id}-count`}
              spellCheck={false}
              autoComplete="off"
              className="h-10 min-w-0 flex-1 bg-transparent text-sm text-fg placeholder:text-placeholder focus-visible:outline-none"
            />
          </div>
          <p id={`${id}-count`} className="sr-only" aria-live="polite">
            {total} ship{total === 1 ? "" : "s"} found
          </p>

          <div className="min-h-0 flex-1 overflow-y-auto py-1">
            <ShipRow
              ship={null}
              selected={selected === null}
              onSelect={() => choose(null)}
            />
            {favorites.length > 0 && <GroupLabel>Favourites</GroupLabel>}
            <ShipList ships={favorites} selectedId={selected?.id ?? null} onSelect={choose} />
            {others.length > 0 && favorites.length > 0 && <GroupLabel>All ships</GroupLabel>}
            <ShipList ships={others} selectedId={selected?.id ?? null} onSelect={choose} />
            {total === 0 && <p className="px-3 py-4 text-center text-sm text-dim">No ship matches “{query.trim()}”.</p>}
          </div>
          <p className="border-t border-line px-3 py-1.5 text-[11px] text-dim">
            Informational only · data from the Star Citizen Wiki
          </p>
        </div>
      )}
    </div>
  );
}

function GroupLabel({ children }: { children: string }) {
  return <p className="label-tech px-3 pt-2 pb-1">{children}</p>;
}

function ShipList({
  ships,
  selectedId,
  onSelect,
}: {
  ships: ShipEntry[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  if (ships.length === 0) return null;
  return (
    <ul className="flex flex-col">
      {ships.map((ship) => (
        <li key={ship.id}>
          <ShipRow ship={ship} selected={ship.id === selectedId} onSelect={() => onSelect(ship.id)} />
        </li>
      ))}
    </ul>
  );
}

function ShipRow({ ship, selected, onSelect }: { ship: ShipEntry | null; selected: boolean; onSelect: () => void }) {
  return (
    <div className="group flex items-center pr-1.5">
      <button
        type="button"
        data-ship-option=""
        aria-current={selected ? "true" : undefined}
        onClick={onSelect}
        className={[
          "flex min-h-9 min-w-0 flex-1 items-center gap-2 px-3 py-1 text-left text-sm transition-colors duration-100 hover:bg-raised focus-visible:bg-raised focus-visible:outline-offset-[-2px]",
          selected ? "text-fg" : "text-muted hover:text-fg",
        ].join(" ")}
      >
        <Check className={["size-3.5 shrink-0", selected ? "text-accent" : "invisible"].join(" ")} aria-hidden="true" />
        {ship ? (
          <>
            <span className="min-w-0 flex-1">
              <span className="block truncate">{ship.name}</span>
              <span className="block truncate text-xs text-dim">{ship.manufacturer}</span>
            </span>
            <span className="shrink-0 font-mono text-xs text-dim tabular-nums">{ship.cargoScu.toLocaleString("en-US")} SCU</span>
          </>
        ) : (
          <span className="min-w-0 flex-1 truncate italic">No ship</span>
        )}
      </button>
      {ship && (
        <button
          type="button"
          aria-pressed={ship.isFavorite}
          aria-label={ship.isFavorite ? `Remove ${ship.name} from favourites` : `Add ${ship.name} to favourites`}
          title={ship.isFavorite ? "Remove from favourites" : "Add to favourites"}
          data-star={ship.id}
          onClick={(e) => {
            // The row moves between Favourites and All ships: keep focus on its star.
            const panel = e.currentTarget.closest('[role="dialog"]');
            void toggleFavorite(ship.id, !ship.isFavorite);
            requestAnimationFrame(() =>
              panel?.querySelector<HTMLElement>(`[data-star="${CSS.escape(ship.id)}"]`)?.focus(),
            );
          }}
          className="grid size-8 shrink-0 place-items-center rounded-sm text-dim transition-colors duration-100 hover:bg-raised hover:text-accent"
        >
          <Star
            className={["size-3.5", ship.isFavorite ? "fill-accent text-accent" : ""].join(" ")}
            aria-hidden="true"
          />
        </button>
      )}
    </div>
  );
}
