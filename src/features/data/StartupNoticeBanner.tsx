import { useEffect, useState } from "react";
import { AlertTriangle, CircleCheck, X } from "lucide-react";
import { takeStartupNotice, type StartupNotice } from "./api";

/** The one-time notice after a restore at startup (Task 17): success or "nothing changed". */
export function StartupNoticeBanner() {
  const [notice, setNotice] = useState<StartupNotice | null>(null);
  useEffect(() => {
    let cancelled = false;
    let tries = 0;
    // The database thread may still be swapping/opening: ask a few times, then stop.
    const ask = () =>
      takeStartupNotice()
        .then((n) => {
          if (cancelled) return;
          if (n) setNotice(n);
          else if (++tries < 5) window.setTimeout(ask, 1000);
        })
        .catch(() => undefined);
    void ask();
    return () => {
      cancelled = true;
    };
  }, []);
  if (!notice) return null;
  return (
    <div className="mx-auto max-w-5xl px-4 pt-3 sm:px-6">
      <div
        role={notice.ok ? "status" : "alert"}
        className={[
          "flex items-start gap-2 border-l-2 bg-surface px-3 py-2 text-sm",
          notice.ok ? "border-pickup" : "border-danger",
        ].join(" ")}
      >
        {notice.ok ? (
          <CircleCheck className="mt-0.5 size-4 shrink-0 text-pickup" aria-hidden="true" />
        ) : (
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-danger" aria-hidden="true" />
        )}
        <span className="min-w-0 flex-1 break-words text-fg">{notice.message}</span>
        <button
          type="button"
          onClick={() => setNotice(null)}
          aria-label="Dismiss notice"
          className="grid size-6 shrink-0 place-items-center rounded-sm text-dim hover:bg-raised hover:text-fg"
        >
          <X className="size-3.5" aria-hidden="true" />
        </button>
      </div>
    </div>
  );
}
