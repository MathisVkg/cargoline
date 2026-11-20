import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { RefreshCw } from "lucide-react";
import { DisabledButton, PageContainer, PageHeader, Tag } from "../components/ui";
import { getSettings, updateCaptureSettings } from "../features/capture/api";
import type { AppSettings, CaptureSettingsInput, CaptureSource, CompleteFromGameMode } from "../features/capture/types";
import { getShipCatalogInfo } from "../features/ships/api";
import { catalogLine } from "../features/ships/model";
import { describeError, toCommandError } from "../features/contracts/api";
import {
  announceSettingsChanged,
  browseForGameLog,
  detectGameLogPath,
  gameLogBrowseStart,
  getGameLogStatus,
  onGameLogStatus,
  updateCaptureShortcut,
  updateGameLogSettings,
} from "../features/gamelog/api";
import type { FoundLog, GameLogSettingsInput, WatchStatus } from "../features/gamelog/types";
import { choiceLabel, detectOutcome } from "../features/gamelog/detect";
import { shortcutFromKey, shortcutProblem } from "../features/gamelog/shortcut";

export function SettingsPage() {
  const game = useGameLogSettings();
  return (
    <PageContainer>
      <div className="mx-auto max-w-3xl">
        <PageHeader
          index="04"
          title="Settings"
          description="Game and OCR Capture settings are saved on this computer. Updates are a placeholder in this version."
        />
        <div className="flex flex-col gap-6">
          <GameSection game={game} />
          <OcrSection game={game} />
          <UpdatesSection />
          <AboutSection />
        </div>
      </div>
    </PageContainer>
  );
}

type GameLogSettings = ReturnType<typeof useGameLogSettings>;

const DEFAULT_GAME_LOG: GameLogSettingsInput = {
  monitoring: false,
  path: null,
  autoCapture: false,
  completeFromGame: false,
  completeFromGameMode: "ask",
};

function gameLogInput(s: AppSettings): GameLogSettingsInput {
  return {
    monitoring: s.game_log_monitoring_enabled,
    path: s.game_log_path,
    autoCapture: s.auto_capture_on_accept,
    completeFromGame: s.complete_from_game,
    completeFromGameMode: s.complete_from_game_mode,
  };
}

/** The Game group's stored values, the watcher status, and saving (all or nothing). */
function useGameLogSettings() {
  const [value, setValue] = useState<GameLogSettingsInput | null>(null);
  const [pathText, setPathText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<WatchStatus>({ state: "off" });

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    getSettings()
      .then((s) => {
        if (cancelled) return;
        setValue(gameLogInput(s));
        setPathText(s.game_log_path ?? "");
      })
      .catch((e) => !cancelled && setError(describeError(toCommandError(e))));
    getGameLogStatus()
      .then((st) => !cancelled && setStatus(st))
      .catch(() => undefined);
    onGameLogStatus((st) => setStatus(st))
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const save = (next: GameLogSettingsInput) => {
    updateGameLogSettings(next)
      .then((s) => {
        setValue(gameLogInput(s));
        setPathText(s.game_log_path ?? "");
        setError(null);
        announceSettingsChanged();
        return getGameLogStatus().then(setStatus);
      })
      .catch((e) => {
        const failure = toCommandError(e);
        setError(failure.kind === "validation" ? failure.issues.map((i) => i.message).join(" ") : describeError(failure));
      });
  };

  return { value, pathText, setPathText, error, setError, status, save };
}

function statusText(status: WatchStatus): { text: string; tone: string } {
  switch (status.state) {
    case "watching":
      return { text: "Watching", tone: "text-pickup" };
    case "fileNotFound":
      return { text: "File not found", tone: "text-accent" };
    case "unreadable":
      return { text: `Can't read the file (${status.reason})`, tone: "text-danger" };
    case "off":
      return { text: "Off", tone: "text-dim" };
  }
}

type Note = { tone: "info" | "success" | "warning"; text: string };

function GameSection({ game }: { game: GameLogSettings }) {
  const pathId = useId();
  const { value, pathText, setPathText, error, status, save } = game;
  const ready = value !== null;
  const current = value ?? DEFAULT_GAME_LOG;
  // Detect / Browse feedback, always shown next to the buttons (never silent).
  const [note, setNote] = useState<Note | null>(null);
  const [choices, setChoices] = useState<FoundLog[]>([]);
  const [busy, setBusy] = useState(false);

  const usePath = (path: string, text: string) => {
    setPathText(path);
    setChoices([]);
    setNote({ tone: "success", text });
    if (path !== current.path) save({ ...current, path });
  };
  const savePath = (path: string) => {
    const trimmed = path.trim();
    if ((trimmed || null) === current.path) return;
    save({ ...current, path: trimmed || null, monitoring: trimmed ? current.monitoring : false });
  };
  const detect = () => {
    setBusy(true);
    setChoices([]);
    setNote({ tone: "info", text: "Looking for Game.log…" });
    detectGameLogPath()
      .then((result) => {
        const outcome = detectOutcome(result);
        if (outcome.kind === "none") setNote({ tone: "warning", text: outcome.message });
        else if (outcome.kind === "one") {
          const same = outcome.log.path === current.path;
          usePath(outcome.log.path, same ? `${outcome.message} — already set` : outcome.message);
        } else {
          setChoices(outcome.choices);
          setNote({ tone: "info", text: outcome.message });
        }
      })
      .catch((e) => setNote({ tone: "warning", text: `Detect failed: ${describeError(toCommandError(e))}` }))
      .finally(() => setBusy(false));
  };
  const browse = () => {
    setBusy(true);
    gameLogBrowseStart(current.path ?? (pathText.trim() || null))
      .catch(() => null)
      .then((start) => browseForGameLog(start))
      .then((picked) => {
        if (picked) usePath(picked, "Selected");
      })
      .catch((e) => setNote({ tone: "warning", text: `Browse failed: ${describeError(toCommandError(e))}` }))
      .finally(() => setBusy(false));
  };
  const shown = statusText(status);
  const noteTone = { info: "text-muted", success: "text-pickup", warning: "text-accent" } as const;
  return (
    <SettingsSection index="01" title="Game">
      <Row label="Game.log monitoring" hint="Optional. Reads new lines only; never required for managing contracts.">
        <Switch
          label="Game.log monitoring"
          checked={current.monitoring}
          disabled={!ready}
          onChange={(on) => save({ ...current, path: pathText.trim() || current.path, monitoring: on })}
        />
      </Row>
      <Row label="Game.log file path" hint="Usually …\StarCitizen\LIVE\Game.log. Detect finds it; Browse picks it.">
        <div className="flex w-full flex-col gap-1 sm:w-96">
          <input
            id={pathId}
            type="text"
            value={pathText}
            disabled={!ready}
            onChange={(e) => setPathText(e.target.value)}
            onBlur={(e) => savePath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && savePath(e.currentTarget.value)}
            aria-label="Game.log file path"
            placeholder="Not configured"
            spellCheck={false}
            className="h-9 w-full min-w-0 rounded-sm border border-line bg-raised px-3 text-sm text-fg placeholder:text-placeholder disabled:cursor-not-allowed"
          />
          <div className="flex flex-wrap items-center gap-2">
            <SmallButton onClick={detect} disabled={!ready || busy}>
              Detect
            </SmallButton>
            <SmallButton onClick={browse} disabled={!ready || busy}>
              Browse…
            </SmallButton>
            {note && (
              <span role="status" aria-live="polite" className={`min-w-0 flex-1 text-xs break-words ${noteTone[note.tone]}`}>
                {note.text}
              </span>
            )}
          </div>
          {choices.length > 0 && (
            <ul aria-label="Game.log files found" className="flex flex-col gap-1">
              {choices.map((c, i) => (
                <li key={c.path} className="flex items-center gap-2 rounded-sm border border-line bg-raised px-2 py-1">
                  <span className="min-w-0 flex-1">
                    <span className="block text-xs text-fg">{choiceLabel(c, i === 0)}</span>
                    <span className="block truncate text-[11px] text-dim" title={c.path}>
                      {c.path}
                    </span>
                  </span>
                  <SmallButton onClick={() => usePath(c.path, `Using ${c.channel}`)}>Use</SmallButton>
                </li>
              ))}
            </ul>
          )}
          {error && (
            <p role="alert" className="text-xs break-words text-danger">
              {error}
            </p>
          )}
          <p className={`text-xs ${shown.tone}`} role="status" aria-live="polite">
            {shown.text}
          </p>
        </div>
      </Row>
      <Row label="Ship catalog" hint="Bundled offline; cargo capacities for the top-bar ship selector.">
        <CatalogLine />
      </Row>
    </SettingsSection>
  );
}

function CatalogLine() {
  const [text, setText] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let cancelled = false;
    getShipCatalogInfo()
      .then((info) => !cancelled && setText(catalogLine(info)))
      .catch(() => !cancelled && setFailed(true));
    return () => {
      cancelled = true;
    };
  }, []);
  if (failed) return <span className="text-sm text-danger">Ship catalog could not be loaded</span>;
  return <span className="text-sm break-words text-fg">{text ?? "—"}</span>;
}

function SmallButton({ onClick, disabled, children }: { onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="h-8 shrink-0 rounded-sm border border-line px-3 text-sm font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg disabled:opacity-50"
    >
      {children}
    </button>
  );
}

/** Click, then press a combination (Escape cancels). Saved at once; Clear removes it. */
function ShortcutRecorder() {
  const [saved, setSaved] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState<string>("");
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    getSettings()
      .then((s) => !cancelled && setSaved(s.capture_shortcut))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  const store = (shortcut: string | null) =>
    updateCaptureShortcut(shortcut)
      .then((s) => {
        setSaved(s.capture_shortcut);
        setProblem(null);
        announceSettingsChanged();
      })
      .catch((e) => {
        const failure = toCommandError(e);
        setProblem(failure.kind === "validation" ? failure.issues.map((i) => i.message).join(" ") : describeError(failure));
      });
  return (
    <div className="flex w-full flex-col gap-1 sm:w-64">
      <div className="flex gap-2">
        <button
          type="button"
          aria-label="Capture shortcut"
          aria-describedby={problem ? "shortcut-problem" : undefined}
          onClick={() => {
            setRecording(true);
            setHeld("");
            setProblem(null);
          }}
          onBlur={() => setRecording(false)}
          onKeyDown={(e) => {
            if (!recording) return;
            e.preventDefault();
            e.stopPropagation(); // never triggers the current shortcut while recording
            if (e.key === "Escape") return setRecording(false);
            const combo = shortcutFromKey(e);
            if (!combo) {
              setHeld([e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift"].filter(Boolean).join("+"));
              return;
            }
            const reason = shortcutProblem(combo);
            setRecording(false);
            if (reason) setProblem(`${combo}: ${reason}`);
            else void store(combo);
          }}
          className={[
            "h-9 min-w-0 flex-1 rounded-sm border bg-raised px-3 text-left font-mono text-sm",
            recording ? "border-accent text-accent" : "border-line text-fg hover:border-line-strong",
          ].join(" ")}
        >
          {recording ? held || "Press a combination…" : (saved ?? <span className="font-sans text-placeholder">Not configured</span>)}
        </button>
        <button
          type="button"
          onClick={() => void store(null)}
          disabled={!saved}
          className="h-9 shrink-0 rounded-sm border border-line px-3 text-sm font-medium text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg disabled:opacity-40"
        >
          Clear
        </button>
      </div>
      {problem && (
        <p id="shortcut-problem" className="text-xs break-words text-danger">
          {problem}
        </p>
      )}
    </div>
  );
}

const DEFAULT_CAPTURE: CaptureSettingsInput = { source: "star_citizen_window", delayMs: 1000, onAddContract: true };

/** Loads the capture settings, saves each change (debounced for the slider) and reports failures. */
function useCaptureSettings() {
  const [value, setValue] = useState<CaptureSettingsInput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const saved = useRef<CaptureSettingsInput>(DEFAULT_CAPTURE);
  const timer = useRef<number | undefined>(undefined);
  const pending = useRef<CaptureSettingsInput | null>(null);

  useEffect(() => {
    let cancelled = false;
    getSettings()
      .then((s) => {
        if (cancelled) return;
        const loaded = { source: s.capture_source, delayMs: s.capture_delay_ms, onAddContract: s.capture_on_add_contract };
        saved.current = loaded;
        setValue(loaded);
      })
      .catch((e) => !cancelled && setError(describeError(toCommandError(e))));
    return () => {
      cancelled = true;
      window.clearTimeout(timer.current);
      // Leaving the page right after moving the slider still saves it.
      if (pending.current) void updateCaptureSettings(pending.current).catch(() => undefined);
      pending.current = null;
    };
  }, []);

  const change = (next: CaptureSettingsInput, debounceMs = 0) => {
    setValue(next);
    window.clearTimeout(timer.current);
    pending.current = next;
    timer.current = window.setTimeout(() => {
      pending.current = null;
      updateCaptureSettings(next)
        .then((s) => {
          saved.current = { source: s.capture_source, delayMs: s.capture_delay_ms, onAddContract: s.capture_on_add_contract };
          setError(null);
        })
        .catch((e) => {
          setError(describeError(toCommandError(e)));
          setValue(saved.current); // show what is actually stored
        });
    }, debounceMs);
  };
  return { value, error, change };
}

function OcrSection({ game }: { game: GameLogSettings }) {
  const delayId = useId();
  const { value, error, change } = useCaptureSettings();
  const ready = value !== null;
  const current = value ?? DEFAULT_CAPTURE;
  return (
    <SettingsSection index="02" title="OCR Capture">
      {error && (
        <p role="alert" className="px-4 py-2 text-sm break-words text-danger">
          {error}
        </p>
      )}
      <Row label="Capture source" hint="Full display hides Cargo Line from the image during the capture.">
        <select
          disabled={!ready}
          aria-label="Capture source"
          value={current.source}
          onChange={(e) => change({ ...current, source: e.target.value as CaptureSource })}
          className="h-9 w-full rounded-sm border border-line bg-raised px-2 text-sm text-fg disabled:cursor-not-allowed disabled:text-dim sm:w-64"
        >
          <option value="star_citizen_window">Star Citizen window</option>
          <option value="full_display">Full display</option>
        </select>
      </Row>
      <Row label="Capture when opening Add Contract" hint="Takes a screenshot in the background after the delay.">
        <Switch
          label="Capture when opening Add Contract"
          checked={current.onAddContract}
          disabled={!ready}
          onChange={(on) => change({ ...current, onAddContract: on })}
        />
      </Row>
      <Row label="Capture delay" hint="0.0 – 5.0 seconds, 0.1 s steps.">
        <div className="flex w-full items-center gap-3 sm:w-64">
          <input
            id={delayId}
            type="range"
            min={0}
            max={5}
            step={0.1}
            value={current.delayMs / 1000}
            disabled={!ready}
            onChange={(e) => change({ ...current, delayMs: Math.round(Number(e.target.value) * 10) * 100 }, 300)}
            aria-label="Capture delay in seconds"
            aria-valuetext={`${(current.delayMs / 1000).toFixed(1)} seconds`}
            className="min-w-0 flex-1 accent-(--color-accent) disabled:cursor-not-allowed"
          />
          <span className="w-10 text-right font-mono text-xs whitespace-nowrap text-fg">
            {ready ? `${(current.delayMs / 1000).toFixed(1)} s` : "— s"}
          </span>
        </div>
      </Row>
      <Row
        label="Capture shortcut"
        hint="Only while Cargo Line is focused: opens Add Contract with a capture, or recaptures in an open form."
      >
        <ShortcutRecorder />
      </Row>
      <Row
        label="Automatic capture on accept"
        hint={
          game.value?.monitoring
            ? "When a hauling contract is accepted in game, Add Contract opens and captures."
            : "Needs Game.log monitoring (Game group)."
        }
      >
        <Switch
          label="Automatic capture on accept"
          checked={!!game.value?.autoCapture}
          disabled={!game.value?.monitoring}
          onChange={(on) => game.value && game.save({ ...game.value, autoCapture: on })}
        />
      </Row>
      <Row
        label="Complete contracts finished in game"
        hint={
          game.value?.monitoring
            ? "Only for contracts saved with their in-game mission. Abandoned or failed missions are never closed for you."
            : "Needs Game.log monitoring (Game group)."
        }
      >
        <Switch
          label="Complete contracts finished in game"
          checked={!!game.value?.completeFromGame}
          disabled={!game.value?.monitoring}
          onChange={(on) => game.value && game.save({ ...game.value, completeFromGame: on })}
        />
      </Row>
      {game.value?.monitoring && game.value.completeFromGame && (
        <CompleteModeChoice
          mode={game.value.completeFromGameMode}
          onChange={(mode) => game.value && game.save({ ...game.value, completeFromGameMode: mode })}
        />
      )}
    </SettingsSection>
  );
}

const COMPLETE_MODES: { id: CompleteFromGameMode; label: string; hint: string }[] = [
  { id: "ask", label: "Ask me first", hint: "Opens a confirmation listing the contracts the game reports as completed." },
  { id: "automatic", label: "Automatically", hint: "Marks them complete without asking. You can undo for 30 seconds." },
];

function CompleteModeChoice({ mode, onChange }: { mode: CompleteFromGameMode; onChange: (mode: CompleteFromGameMode) => void }) {
  const name = useId();
  return (
    <fieldset className="px-4 py-3 sm:pl-8">
      <legend className="sr-only">When the game reports a contract completed</legend>
      <div className="flex flex-col gap-2">
        {COMPLETE_MODES.map((m) => (
          <label key={m.id} className="flex cursor-pointer items-start gap-2.5">
            <input
              type="radio"
              name={name}
              value={m.id}
              checked={mode === m.id}
              onChange={() => onChange(m.id)}
              className="mt-0.5 size-4 shrink-0 accent-(--color-accent)"
            />
            <span className="min-w-0">
              <span className="block text-sm text-fg">{m.label}</span>
              <span className="block text-xs break-words text-dim">{m.hint}</span>
            </span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}

function UpdatesSection() {
  const version = useAppVersion();
  return (
    <SettingsSection index="03" title="Updates">
      <Row label="Current version">
        <span className="font-mono text-sm text-fg">{version ?? "—"}</span>
      </Row>
      <Row label="Check for updates" hint="Updates will always ask before installing.">
        <DisabledButton icon={RefreshCw} note="Update checks are not available yet">
          Check now
        </DisabledButton>
      </Row>
    </SettingsSection>
  );
}

function AboutSection() {
  return (
    <SettingsSection index="04" title="About">
      <div className="space-y-3 px-4 py-4 text-sm text-muted">
        <p className="break-words">
          Cargo Line is an offline-first hauling contract manager for Star Citizen. It runs entirely on this
          computer, with no account or server.
        </p>
        <p className="break-words">
          Cargo Line is an unofficial fan project. It is not affiliated with or endorsed by Cloud Imperium Games.
          Star Citizen® is a registered trademark of Cloud Imperium Rights LLC.
        </p>
      </div>
      <Row label="Licenses & legal notices">
        <Tag>Pending</Tag>
      </Row>
      <Row label="Ship data attribution" hint="Unofficial; not affiliated with Cloud Imperium Games.">
        <span className="text-sm break-words text-muted">Ship data from the Star Citizen Wiki API (api.star-citizen.wiki)</span>
      </Row>
    </SettingsSection>
  );
}

/* ---------- local building blocks ---------- */

function SettingsSection({ index, title, children }: { index: string; title: string; children: ReactNode }) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="rounded-sm border border-line bg-surface">
      <div className="flex items-center gap-3 border-b border-line px-4 py-3">
        <span className="font-mono text-xs text-accent">{index}</span>
        <h2 id={headingId} className="text-sm font-semibold tracking-wide text-fg">
          {title}
        </h2>
      </div>
      <div className="divide-y divide-line">{children}</div>
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center sm:justify-between sm:gap-6">
      <div className="min-w-0">
        <p className="text-sm text-fg break-words">{label}</p>
        {hint && <p className="text-xs text-dim break-words">{hint}</p>}
      </div>
      <div className="flex min-w-0 shrink-0 items-center">{children}</div>
    </div>
  );
}

/** A switch; without `onChange` it is a disabled placeholder. */
function Switch({
  label,
  checked = false,
  disabled,
  onChange,
}: {
  label: string;
  checked?: boolean;
  disabled?: boolean;
  onChange?: (checked: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled ?? !onChange}
      onClick={() => onChange?.(!checked)}
      className={`relative h-5 w-9 rounded-full border transition-colors duration-150 disabled:cursor-not-allowed ${
        checked ? "border-accent bg-accent/30" : "border-line bg-raised"
      }`}
    >
      <span
        aria-hidden="true"
        className={`absolute top-0.5 size-3.5 rounded-full transition-[left] duration-150 ${
          checked ? "left-[1.1rem] bg-accent" : "left-0.5 bg-line-strong"
        }`}
      />
    </button>
  );
}

/** Reads the real application version from Tauri; stays empty in a plain browser. */
function useAppVersion() {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    getVersion()
      .then((v) => !cancelled && setVersion(v))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  return version;
}
