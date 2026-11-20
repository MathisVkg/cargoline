import { useId, useState, type ReactNode } from "react";
import { ChevronDown, type LucideIcon } from "lucide-react";

/** Shared presentational primitives for page shells. */

export function PageContainer({ children, wide = false }: { children: ReactNode; wide?: boolean }) {
  return (
    <div className={["mx-auto w-full px-4 py-6 sm:px-6 sm:py-8", wide ? "max-w-[1600px]" : "max-w-5xl"].join(" ")}>
      {children}
    </div>
  );
}

interface PageHeaderProps {
  index: string;
  title: string;
  description: string;
  actions?: ReactNode;
}

export function PageHeader({ index, title, description, actions }: PageHeaderProps) {
  return (
    <header className="mb-6 flex flex-wrap items-end justify-between gap-4 border-b border-line pb-5">
      <div className="min-w-0 max-w-2xl">
        <p className="label-tech mb-1">
          <span className="text-accent">{index}</span> / {title}
        </p>
        <h1 className="text-2xl font-semibold tracking-tight break-words text-fg">{title}</h1>
        <p className="mt-1 text-muted break-words">{description}</p>
      </div>
      {actions && <div className="flex flex-wrap items-center gap-3">{actions}</div>}
    </header>
  );
}

export function Panel({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <section className={["min-w-0 rounded-sm border border-line bg-surface", className].join(" ")}>{children}</section>;
}

export function PanelHeader({ title, meta }: { title: string; meta?: ReactNode }) {
  return (
    <div className="flex min-w-0 items-center justify-between gap-3 border-b border-line px-4 py-3">
      <h2 className="truncate text-sm font-semibold tracking-wide text-fg">{title}</h2>
      {meta && <div className="shrink-0">{meta}</div>}
    </div>
  );
}

interface CollapsiblePanelProps {
  title: string;
  meta?: ReactNode;
  defaultOpen?: boolean;
  /** Controlled mode: the caller keeps the state (e.g. for the session). */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  children: ReactNode;
}

/** Panel whose body can be collapsed independently of other panels. */
export function CollapsiblePanel({ title, meta, defaultOpen = true, open: controlled, onOpenChange, children }: CollapsiblePanelProps) {
  const [own, setOwn] = useState(defaultOpen);
  const open = controlled ?? own;
  const setOpen = (change: (v: boolean) => boolean) => {
    const next = change(open);
    if (controlled === undefined) setOwn(next);
    onOpenChange?.(next);
  };
  const bodyId = useId();
  return (
    <Panel>
      <h2 className="m-0">
        <button
          type="button"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={() => setOpen((v) => !v)}
          className={[
            "flex w-full min-w-0 items-center gap-3 px-4 py-3 text-left transition-colors duration-150 hover:bg-raised",
            "focus-visible:-outline-offset-2",
            open ? "border-b border-line" : "",
          ].join(" ")}
        >
          <ChevronDown
            className={["size-4 shrink-0 text-dim transition-transform duration-150", open ? "" : "-rotate-90"].join(" ")}
            aria-hidden="true"
          />
          <span className="min-w-0 flex-1 truncate text-sm font-semibold tracking-wide text-fg">{title}</span>
          {meta && <span className="shrink-0">{meta}</span>}
        </button>
      </h2>
      <div id={bodyId} hidden={!open}>
        {children}
      </div>
    </Panel>
  );
}

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  children?: ReactNode;
  compact?: boolean;
}

export function EmptyState({ icon: Icon, title, children, compact = false }: EmptyStateProps) {
  return (
    <div className={["flex flex-col items-center text-center", compact ? "px-4 py-8" : "px-6 py-14"].join(" ")}>
      <div className="mb-3 grid size-10 place-items-center rounded-sm border border-line bg-raised">
        <Icon className="size-5 text-dim" aria-hidden="true" />
      </div>
      <p className="font-medium text-fg">{title}</p>
      {children && <div className="mt-1 max-w-md text-sm text-muted break-words">{children}</div>}
    </div>
  );
}

export function Tag({ children }: { children: ReactNode }) {
  return (
    <span className="inline-flex items-center rounded-sm border border-line px-1.5 py-0.5 font-mono text-[10px] tracking-[0.1em] whitespace-nowrap text-dim uppercase">
      {children}
    </span>
  );
}

export function DisabledButton({ icon: Icon, children, note }: { icon?: LucideIcon; children: ReactNode; note?: string }) {
  return (
    <button
      type="button"
      disabled
      title={note}
      className="inline-flex h-9 items-center gap-2 rounded-sm border border-line bg-raised px-3 text-sm font-medium text-dim disabled:cursor-not-allowed"
    >
      {Icon && <Icon className="size-4 shrink-0" aria-hidden="true" />}
      <span className="whitespace-nowrap">{children}</span>
      {note && <span className="sr-only"> — {note}</span>}
    </button>
  );
}
