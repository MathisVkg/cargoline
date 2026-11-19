import { useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Plus, X, type LucideIcon } from "lucide-react";
import { buildOptions, moveActive, type CatalogOption, type Suggestion } from "./catalogSearch";

interface CatalogComboboxProps {
  label: string;
  /** Show the label above the input (otherwise it is screen-reader only). */
  showLabel?: boolean;
  value: string;
  onChange: (value: string) => void;
  /** Suggestions (bundled list + user entries + names typed elsewhere in the form). */
  suggestions: readonly Suggestion[];
  /** Noun used in "Add new …", e.g. "location" or "commodity". */
  noun: string;
  error?: string;
  /** Amber "check" hint (shown when there is no error). */
  hint?: string;
  placeholder?: string;
  icon?: LucideIcon;
  /** Tailwind text color class for the leading icon. */
  tone?: string;
}

interface Placement {
  left: number;
  width: number;
  top?: number;
  bottom?: number;
  maxHeight: number;
}

const LIST_MAX = 260;

/**
 * Searchable "type or pick" field (ARIA combobox + listbox). Free text is
 * always kept; picking a suggestion fills in its spelling.
 * The popup is fixed-positioned so the modal's scroll area cannot clip it,
 * and it stays inside the dialog subtree so it remains interactive.
 */
export function CatalogCombobox({
  label,
  showLabel = false,
  value,
  onChange,
  suggestions,
  noun,
  error,
  hint,
  placeholder,
  icon: Icon,
  tone = "text-dim",
}: CatalogComboboxProps) {
  const shownHint = error ? undefined : hint;
  const id = useId();
  const listId = `${id}-list`;
  const errorId = `${id}-error`;
  const inputRef = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const [placement, setPlacement] = useState<Placement | null>(null);

  const options = useMemo(() => buildOptions(suggestions, value), [suggestions, value]);

  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const rect = inputRef.current?.getBoundingClientRect();
      if (!rect) return;
      const below = window.innerHeight - rect.bottom - 8;
      const above = rect.top - 8;
      const flip = below < Math.min(LIST_MAX, 160) && above > below;
      setPlacement({
        left: rect.left,
        width: rect.width,
        ...(flip ? { bottom: window.innerHeight - rect.top + 2 } : { top: rect.bottom + 2 }),
        maxHeight: Math.max(96, Math.min(LIST_MAX, flip ? above : below)),
      });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true); // capture: any scrolling ancestor
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open]);

  useLayoutEffect(() => {
    if (open && active >= 0) document.getElementById(`${id}-opt-${active}`)?.scrollIntoView({ block: "nearest" });
  }, [open, active, id]);

  const openList = () => {
    setOpen(true);
    setActive(-1);
  };
  const close = () => {
    setOpen(false);
    setActive(-1);
  };
  const choose = (option: CatalogOption) => {
    onChange(option.kind === "existing" ? option.suggestion.name : option.name);
    close();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp": {
        event.preventDefault();
        if (!open) setOpen(true);
        setActive((current) => moveActive(open ? current : -1, options.length, event.key === "ArrowDown" ? 1 : -1));
        break;
      }
      case "Enter":
        if (open) {
          event.preventDefault(); // never submit the form from an open list
          if (active >= 0 && options[active]) choose(options[active]);
          else close();
        }
        break;
      case "Escape":
        if (open) {
          event.preventDefault(); // close the list, not the dialog
          event.stopPropagation();
          close();
        }
        break;
      case "Tab":
        close();
        break;
    }
  };

  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <label htmlFor={id} className={showLabel ? "text-[11px] font-medium text-muted" : "sr-only"}>
        {label}
      </label>
      <div className="relative min-w-0">
        {Icon && <Icon className={["pointer-events-none absolute top-2 left-2 size-4", tone].join(" ")} aria-hidden="true" />}
        <input
          ref={inputRef}
          id={id}
          type="text"
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={listId}
          aria-activedescendant={open && active >= 0 ? `${id}-opt-${active}` : undefined}
          aria-invalid={error ? true : undefined}
          aria-describedby={error || shownHint ? errorId : undefined}
          value={value}
          placeholder={placeholder}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => {
            onChange(e.target.value);
            setOpen(true);
            setActive(-1);
          }}
          onFocus={openList}
          onClick={() => !open && openList()}
          onBlur={close}
          onKeyDown={onKeyDown}
          className={[
            "h-8 w-full min-w-0 rounded-sm border bg-raised pr-7 text-sm text-fg placeholder:text-placeholder",
            Icon ? "pl-7" : "pl-2",
            "transition-colors duration-150 focus-visible:outline-offset-0 disabled:opacity-60",
            error ? "border-danger" : shownHint ? "border-accent" : "border-line hover:border-line-strong",
          ].join(" ")}
        />
        {value !== "" && (
          <button
            type="button"
            tabIndex={-1}
            aria-label={`Clear ${label}`}
            title="Clear"
            onMouseDown={(e) => e.preventDefault()} // keep focus in the input
            onClick={() => {
              onChange("");
              inputRef.current?.focus();
              openList();
            }}
            className="absolute top-1 right-1 grid size-6 place-items-center rounded-sm text-dim hover:bg-surface hover:text-fg"
          >
            <X className="size-3.5" aria-hidden="true" />
          </button>
        )}

        {open && placement && (
          <ul
            id={listId}
            role="listbox"
            aria-label={`${label} suggestions`}
            style={{
              left: placement.left,
              width: Math.max(placement.width, 260),
              top: placement.top,
              bottom: placement.bottom,
              maxHeight: placement.maxHeight,
            }}
            className="fixed z-50 overflow-y-auto rounded-sm border border-line-strong bg-surface py-1 text-sm shadow-xl"
            onMouseDown={(e) => e.preventDefault()} // clicking an option must not blur the input first
          >
            {options.length === 0 && (
              <li role="presentation" className="px-3 py-2 text-dim">
                {suggestions.length === 0 ? `No saved ${noun}s yet. Type a name to add one.` : `No matching ${noun}s.`}
              </li>
            )}
            {options.map((option, i) => (
              <li
                key={option.kind === "existing" ? `e:${option.suggestion.name}` : `c:${option.name}`}
                id={`${id}-opt-${i}`}
                role="option"
                aria-selected={i === active}
                onClick={() => choose(option)}
                onMouseMove={() => i !== active && setActive(i)}
                className={[
                  "flex cursor-pointer items-baseline gap-2 px-3 py-1.5 break-words",
                  i === active ? "bg-raised text-fg" : "text-muted",
                ].join(" ")}
              >
                {option.kind === "create" ? (
                  <>
                    <Plus className="size-3.5 shrink-0 translate-y-0.5 text-accent" aria-hidden="true" />
                    <span className="min-w-0">
                      Add new {noun} “<span className="text-fg">{option.name}</span>”
                    </span>
                  </>
                ) : (
                  <>
                    <span className="min-w-0">{option.suggestion.name}</span>
                    {option.suggestion.detail && (
                      <span className="ml-auto shrink-0 pl-2 text-xs text-dim">{option.suggestion.detail}</span>
                    )}
                  </>
                )}
              </li>
            ))}
            {options.length > 0 && options.every((o) => o.kind === "create") && (
              <li role="presentation" className="px-3 pt-1 pb-1.5 text-xs text-dim">
                No saved {noun} matches.
              </li>
            )}
          </ul>
        )}
      </div>
      {error && (
        <p id={errorId} className="text-xs break-words text-danger">
          {error}
        </p>
      )}
      {shownHint && (
        <p id={errorId} className="text-xs break-words text-accent">
          {shownHint}
        </p>
      )}
    </div>
  );
}
