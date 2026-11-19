import { Flag, ScrollText, X } from "lucide-react";
import type { ContractDetail } from "../contracts/types";
import type { FinalAction } from "../contracts/finalize";
import type { FlowNotice } from "./flow";
import { noticeView } from "./notices";

/** Game.log notices on the Contracts page: dismissible, never acting on their own. */
export function GameLogNotices({
  notices,
  active,
  onAdd,
  onFinalize,
  onDismiss,
}: {
  notices: readonly FlowNotice[];
  active: readonly ContractDetail[];
  onAdd: (noticeId: string) => void;
  onFinalize: (noticeId: string, contractId: number, action: FinalAction) => void;
  onDismiss: (noticeId: string) => void;
}) {
  const shown = notices.map((n) => ({ notice: n, view: noticeView(n, active) })).filter((x) => x.view);
  if (shown.length === 0) return null;
  return (
    <ul aria-label="Game.log notices" className="mb-3 flex flex-col gap-1.5">
      {shown.map(({ notice, view }) => (
        <li
          key={notice.id}
          role="status"
          className="flex flex-wrap items-center gap-2 border-l-2 border-accent bg-surface px-3 py-1.5 text-sm"
        >
          {notice.kind === "accepted" ? (
            <ScrollText className="size-4 shrink-0 text-accent" aria-hidden="true" />
          ) : (
            <Flag className="size-4 shrink-0 text-accent" aria-hidden="true" />
          )}
          <span className="min-w-0 flex-1 break-words text-fg">{view!.text}</span>
          <button
            type="button"
            onClick={() =>
              notice.kind === "accepted" ? onAdd(notice.id) : onFinalize(notice.id, notice.contractId, view!.action as FinalAction)
            }
            className="h-7 rounded-sm border border-accent/50 px-2.5 text-xs font-medium text-fg transition-colors duration-150 hover:bg-accent-soft"
          >
            {view!.label}
          </button>
          <button
            type="button"
            onClick={() => onDismiss(notice.id)}
            aria-label="Dismiss notice"
            className="grid size-6 shrink-0 place-items-center rounded-sm text-dim hover:bg-raised hover:text-fg"
          >
            <X className="size-3.5" aria-hidden="true" />
          </button>
        </li>
      ))}
    </ul>
  );
}
