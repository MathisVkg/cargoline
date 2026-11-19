import { PAGES, type PageId } from "../navigation";

interface BottomNavProps {
  current: PageId;
  onNavigate: (page: PageId) => void;
}

/**
 * Fixed bottom navigation: exactly four equal sections spanning the full
 * window width. Each whole quarter is the click target and carries the
 * active state.
 */
export function BottomNav({ current, onNavigate }: BottomNavProps) {
  return (
    <nav aria-label="Primary" className="border-t border-line bg-surface">
      <ul className="grid w-full grid-cols-4 divide-x divide-line">
        {PAGES.map(({ id, label, icon: Icon }) => {
          const active = id === current;
          return (
            <li key={id} className="min-w-0">
              <button
                type="button"
                onClick={() => onNavigate(id)}
                aria-current={active ? "page" : undefined}
                className={[
                  "relative flex h-14 w-full min-w-0 flex-col items-center justify-center gap-0.5 px-2 transition-colors duration-150",
                  "focus-visible:-outline-offset-2",
                  active ? "bg-raised text-fg" : "text-dim hover:bg-raised/60 hover:text-muted",
                ].join(" ")}
              >
                <span
                  aria-hidden="true"
                  className={["absolute inset-x-0 top-0 h-0.5", active ? "bg-accent" : "bg-transparent"].join(" ")}
                />
                <Icon className={["size-5 shrink-0", active ? "text-accent" : ""].join(" ")} aria-hidden="true" />
                <span className="max-w-full truncate text-xs font-medium tracking-wide">{label}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
