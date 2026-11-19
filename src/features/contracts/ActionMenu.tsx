import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { LucideIcon } from "lucide-react";

export interface MenuAction {
  id: string;
  label: string;
  icon?: LucideIcon;
  danger?: boolean;
  disabled?: boolean;
  onSelect: () => void;
}

interface ActionMenuProps {
  /** Accessible name of the trigger, e.g. "Actions for C02 · test multiple". */
  label: string;
  /** Optional non-interactive heading shown at the top of the menu. */
  heading?: ReactNode;
  items: MenuAction[];
  disabled?: boolean;
  /** Trigger content (icon or chip text). */
  children: ReactNode;
  triggerClassName: string;
  title?: string;
}

interface Placement {
  top?: number;
  bottom?: number;
  right: number;
}

/**
 * Menu button + menu (WAI-ARIA menu pattern): Enter/Space/ArrowDown open it,
 * arrow keys / Home / End move, Escape closes and returns focus to the
 * trigger, Tab or a click outside closes. Fixed-positioned so scroll
 * containers cannot clip it.
 */
export function ActionMenu({ label, heading, items, disabled, children, triggerClassName, title }: ActionMenuProps) {
  const id = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [placement, setPlacement] = useState<Placement | null>(null);
  const enabled = items.filter((i) => !i.disabled);

  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) triggerRef.current?.focus();
  };

  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const rect = triggerRef.current?.getBoundingClientRect();
      if (!rect) return;
      const below = window.innerHeight - rect.bottom;
      const right = Math.max(8, window.innerWidth - rect.right);
      setPlacement(below < 180 && rect.top > below ? { bottom: window.innerHeight - rect.top + 4, right } : { top: rect.bottom + 4, right });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open]);

  // Focus the first enabled item when the menu opens.
  useEffect(() => {
    if (open && placement) menuRef.current?.querySelector<HTMLElement>('[role="menuitem"]:not([disabled])')?.focus();
  }, [open, placement]);

  // Close on a pointer press outside the trigger and the menu.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!menuRef.current?.contains(target) && !triggerRef.current?.contains(target)) close(false);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [open]);

  const focusItem = (delta: 1 | -1 | "first" | "last") => {
    const nodes = [...(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? [])];
    if (nodes.length === 0) return;
    const current = nodes.indexOf(document.activeElement as HTMLElement);
    const next =
      delta === "first" ? 0 : delta === "last" ? nodes.length - 1 : (current + delta + nodes.length) % nodes.length;
    nodes[next].focus();
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        focusItem(1);
        break;
      case "ArrowUp":
        event.preventDefault();
        focusItem(-1);
        break;
      case "Home":
        event.preventDefault();
        focusItem("first");
        break;
      case "End":
        event.preventDefault();
        focusItem("last");
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        close(true);
        break;
      case "Tab":
        close(false);
        break;
    }
  };

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-label={label}
        title={title ?? label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? `${id}-menu` : undefined}
        disabled={disabled || enabled.length === 0}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" && !open) {
            e.preventDefault();
            setOpen(true);
          }
        }}
        className={triggerClassName}
      >
        {children}
      </button>
      {open && placement && (
        <div
          ref={menuRef}
          id={`${id}-menu`}
          role="menu"
          aria-label={label}
          onKeyDown={onMenuKeyDown}
          style={{ top: placement.top, bottom: placement.bottom, right: placement.right }}
          className="fixed z-40 min-w-56 max-w-[min(22rem,calc(100vw-1rem))] rounded-sm border border-line-strong bg-surface py-1 text-sm shadow-xl"
        >
          {heading && (
            <div role="presentation" className="border-b border-line px-3 pt-1.5 pb-2">
              {heading}
            </div>
          )}
          {items.map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.id}
                type="button"
                role="menuitem"
                tabIndex={-1}
                disabled={item.disabled}
                onClick={() => {
                  close(true);
                  item.onSelect();
                }}
                className={[
                  "flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors duration-100 outline-none",
                  "hover:bg-raised focus:bg-raised disabled:cursor-not-allowed disabled:opacity-40",
                  item.danger ? "text-danger" : "text-fg",
                ].join(" ")}
              >
                {Icon && <Icon className="size-4 shrink-0" aria-hidden="true" />}
                <span className="min-w-0 break-words">{item.label}</span>
              </button>
            );
          })}
        </div>
      )}
    </>
  );
}
