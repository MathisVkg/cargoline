import { AlertTriangle, Copy, ScanText } from "lucide-react";
import type { ReactNode } from "react";
import { duplicateHint, narrowWindowBanner, screenBanner } from "./prefill";
import type { ContractDetail } from "../contracts/types";
import type { ParsedContract } from "./types";

/** Non-blocking notes at the top of the form about the reading that filled it. */
export function ReadingBanners({
  applied,
  duplicate,
  frame = null,
}: {
  applied: ParsedContract | null;
  duplicate: ContractDetail | null;
  /** The captured frame's size: a frame narrower than 16:10 gets the narrow-window warning. */
  frame?: { width: number | null; height: number | null } | null;
}) {
  const screen = applied ? screenBanner(applied) : null;
  const narrow = narrowWindowBanner(frame);
  const problems = applied?.problems ?? [];
  if (!screen && !narrow && problems.length === 0 && !duplicate) return null;
  return (
    <div className="mb-3 flex flex-col gap-2">
      {narrow && (
        <Note icon={AlertTriangle} tone="warning" role="status">
          {narrow}
        </Note>
      )}
      {screen && (
        <Note icon={AlertTriangle} tone="warning" role="status">
          {screen}
        </Note>
      )}
      {problems.length > 0 && (
        <Note icon={AlertTriangle} tone="warning">
          <span className="font-medium">Couldn't read everything</span>
          <ul className="mt-0.5 list-disc pl-4 text-muted">
            {problems.map((p, i) => (
              <li key={i} className="break-words">
                {p}
              </li>
            ))}
          </ul>
        </Note>
      )}
      {duplicate && (
        <Note icon={Copy} tone="info" role="status">
          {duplicateHint(duplicate)}
        </Note>
      )}
    </div>
  );
}

/** "Text read from screenshot — Apply to form", shown when the user had already typed. */
export function ApplyReadingBar({
  confirming,
  onApply,
  onConfirm,
  onCancel,
}: {
  confirming: boolean;
  onApply: () => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div
      role="status"
      className="flex flex-wrap items-center gap-2 rounded-sm border border-accent/40 bg-accent-soft px-3 py-2 text-sm text-fg"
    >
      <ScanText className="size-4 shrink-0 text-accent" aria-hidden="true" />
      <span className="min-w-0 flex-1 break-words">
        {confirming ? "Replace everything you entered with the text read from the screenshot?" : "Text read from screenshot"}
      </span>
      {confirming ? (
        <span className="flex items-center gap-2">
          <BarButton onClick={onCancel}>Keep my entries</BarButton>
          <BarButton onClick={onConfirm} primary>
            Replace
          </BarButton>
        </span>
      ) : (
        <BarButton onClick={onApply} primary>
          Apply to form
        </BarButton>
      )}
    </div>
  );
}

function BarButton({ onClick, primary, children }: { onClick: () => void; primary?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        primary
          ? "h-7 rounded-sm bg-accent px-3 text-xs font-semibold text-(color:--color-canvas) transition-[filter] duration-150 hover:brightness-110"
          : "h-7 rounded-sm border border-line px-3 text-xs font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg"
      }
    >
      {children}
    </button>
  );
}

function Note({
  icon: Icon,
  tone,
  role,
  children,
}: {
  icon: typeof AlertTriangle;
  tone: "warning" | "info";
  role?: "status";
  children: ReactNode;
}) {
  return (
    <div
      role={role}
      className={[
        "flex items-start gap-2 rounded-sm border px-3 py-2 text-xs",
        tone === "warning" ? "border-accent/40 bg-accent-soft text-fg" : "border-line bg-raised text-muted",
      ].join(" ")}
    >
      <Icon className={`mt-0.5 size-3.5 shrink-0 ${tone === "warning" ? "text-accent" : "text-dim"}`} aria-hidden="true" />
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}
