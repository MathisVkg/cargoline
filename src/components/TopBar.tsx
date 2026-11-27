import { ArrowUpCircle } from "lucide-react";
import { ShipSelector } from "./ShipSelector";
import { useUpdateStatus } from "../features/update/api";
import { UPDATE_MARKER_TEXT, markerVisible } from "../features/update/model";

/** A quiet "Update available" line: opens Settings → Updates. Never a pop-up. */
function UpdateMarker({ onOpen }: { onOpen: () => void }) {
  const [status] = useUpdateStatus(false);
  if (!markerVisible(status)) return null;
  return (
    <button
      type="button"
      onClick={onOpen}
      data-update-marker
      title={`Version ${status?.offer?.version} is available`}
      className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-sm px-2 text-xs font-medium text-accent hover:bg-raised"
    >
      <ArrowUpCircle className="size-3.5" aria-hidden="true" />
      <span className="hidden sm:inline">{UPDATE_MARKER_TEXT}</span>
    </button>
  );
}

export function TopBar({ onOpenUpdates }: { onOpenUpdates: () => void }) {
  return (
    <header className="flex h-14 min-w-0 items-center justify-between gap-4 border-b border-line bg-surface px-4 sm:px-6">
      <div className="flex min-w-0 items-center gap-3">
        <LogoMark />
        <div className="flex min-w-0 items-baseline gap-3">
          <span className="truncate text-[15px] font-semibold tracking-wide text-fg">Cargo Line</span>
          <span className="label-tech hidden truncate md:inline">Hauling contract manager</span>
        </div>
      </div>
      <div className="flex min-w-0 items-center gap-2">
        <UpdateMarker onOpen={onOpenUpdates} />
        <ShipSelector />
      </div>
    </header>
  );
}

function LogoMark() {
  return (
    <svg viewBox="0 0 24 24" className="size-6 shrink-0 text-accent" aria-hidden="true">
      <path
        d="M12 3.5 19.5 7.8v8.4L12 20.5l-7.5-4.3V7.8zM4.5 7.8 12 12.1l7.5-4.3M12 12.1v8.4"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinejoin="round"
      />
    </svg>
  );
}
