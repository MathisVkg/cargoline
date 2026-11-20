import { useEffect, useRef, type Dispatch } from "react";
import { finalizeActiveContracts, listActiveContracts } from "../contracts/api";
import { autoIds } from "./completion";
import { CompleteFromGameDialog } from "./CompleteFromGameDialog";
import { BANNER_MS, COMPLETION_BATCH_MS, UNDO_MS, type FlowAction, type FlowState } from "./flow";

/**
 * Runs "Complete contracts finished in game" from the app shell, on any page (Task 11 Part B):
 * - batches completions arriving within ~2 s, then opens the Ask dialog (waiting while another
 *   modal is open) or finalises them automatically;
 * - expires the completion banner (30 s with Undo).
 * Never moves the window or takes the system focus.
 */
export function GameCompletions({ flow, dispatch }: { flow: FlowState; dispatch: Dispatch<FlowAction> }) {
  const c = flow.completion;
  const hasWaiting = c.waiting.length > 0;
  const busy = flow.modal.open || c.blocking || c.dialog !== null || c.running !== null;

  // The batching window starts with the first waiting completion (and again once nothing blocks).
  useEffect(() => {
    if (!hasWaiting) return;
    const timer = window.setTimeout(() => dispatch({ type: "completionDue" }), COMPLETION_BATCH_MS);
    return () => window.clearTimeout(timer);
  }, [hasWaiting, busy]);

  // The automatic run: only contracts still active, one transaction.
  const started = useRef<number | null>(null);
  useEffect(() => {
    const run = c.running;
    if (!run || started.current === run.batch) return;
    started.current = run.batch;
    void (async () => {
      try {
        const ids = autoIds(run.items, await listActiveContracts());
        if (ids.length) await finalizeActiveContracts(ids, "completed");
        dispatch({ type: "autoCompleted", batch: run.batch, completed: ids, failed: [], now: Date.now() });
      } catch {
        // Nothing changed (all or nothing): the normal notices let the user finish them by hand.
        dispatch({ type: "autoCompleted", batch: run.batch, completed: [], failed: run.items, now: Date.now() });
      }
    })();
  }, [c.running?.batch]);

  const banner = flow.banner;
  useEffect(() => {
    if (!banner) return;
    const timer = window.setTimeout(() => dispatch({ type: "bannerDismiss", id: banner.id }), banner.undo ? UNDO_MS : BANNER_MS);
    return () => window.clearTimeout(timer);
  }, [banner?.id]);

  if (!c.dialog) return null;
  return (
    <CompleteFromGameDialog
      items={c.dialog}
      onDone={(completed) => dispatch({ type: "completionDialogDone", completed, now: Date.now() })}
    />
  );
}
