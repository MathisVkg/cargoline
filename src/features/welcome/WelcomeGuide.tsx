import { useEffect, useRef } from "react";
import { WELCOME_INTRO, WELCOME_PRIMARY, WELCOME_SECONDARY, WELCOME_STEPS, WELCOME_TITLE } from "./model";

/**
 * The welcome guide: one short modal, no images, no steps to click through. Mount to open. Focus
 * starts on "Got it"; Esc closes it like "Got it". Same look as the app's other dialogs.
 */
export function WelcomeGuide({ onClose, onOpenSettings }: { onClose: () => void; onOpenSettings: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const primaryRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    primaryRef.current?.focus();
    return () => dialog?.close();
  }, []);
  return (
    <dialog
      ref={ref}
      aria-labelledby="welcome-title"
      aria-describedby="welcome-intro"
      data-welcome-guide
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      className="m-auto w-[min(30rem,calc(100%-2rem))] max-w-none max-h-[calc(100%-2rem)] flex-col overflow-hidden rounded-sm border border-line-strong bg-surface p-0 text-fg shadow-2xl backdrop:bg-black/70 open:flex"
    >
      <div className="min-h-0 flex-1 overflow-y-auto px-4 pt-3.5 pb-3">
        <h2 id="welcome-title" className="text-[15px] font-semibold tracking-tight text-fg">
          {WELCOME_TITLE}
        </h2>
        <p id="welcome-intro" className="mt-2 text-sm break-words text-muted">
          {WELCOME_INTRO}
        </p>
        <ol className="mt-3 space-y-2 text-sm text-fg">
          {WELCOME_STEPS.map((step, i) => (
            <li key={step} className="flex gap-3">
              <span className="font-mono text-xs leading-5 text-accent">{String(i + 1).padStart(2, "0")}</span>
              <span className="min-w-0 break-words">{step}</span>
            </li>
          ))}
        </ol>
      </div>
      <div className="flex shrink-0 justify-end gap-2 border-t border-line px-4 py-2.5">
        <button
          type="button"
          onClick={onOpenSettings}
          className="h-9 rounded-sm border border-line px-4 text-sm font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg"
        >
          {WELCOME_SECONDARY}
        </button>
        <button
          ref={primaryRef}
          type="button"
          onClick={onClose}
          className="h-9 rounded-sm bg-accent px-4 text-sm font-semibold text-(color:--color-canvas) transition-[filter] duration-150 hover:brightness-110"
        >
          {WELCOME_PRIMARY}
        </button>
      </div>
    </dialog>
  );
}
