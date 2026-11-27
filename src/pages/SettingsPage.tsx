import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { ExternalLink, RefreshCw } from "lucide-react";
import { PageContainer, PageHeader } from "../components/ui";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  chooseBackupDestination,
  chooseBackupToRestore,
  exportBackup,
  getAboutInfo,
  getDataInfo,
  inspectBackup,
  openDataFolder,
  openLink,
  restoreBackup,
  type AboutInfo,
  type DataInfo,
} from "../features/data/api";
import {
  ABOUT_DESCRIPTION,
  aboutVersionLine,
  DATA_CREDIT,
  DATA_TERMS,
  LICENCE_TEXT,
  REPOSITORY_LABEL,
  RESTORE_QUESTION,
  SCREENSHOTS_NOT_INCLUDED,
  SETTINGS_DESCRIPTION,
  UNOFFICIAL,
  backupContents,
  catalogCreditLine,
  exportNotice,
  noticeBlocks,
  refusalMessage,
  shortenPath,
  type BackupInfo,
  type LinkKey,
} from "../features/data/model";
import { getSettings, updateCaptureSettings } from "../features/capture/api";
import { requestWelcome } from "../features/welcome/api";
import { SHOW_WELCOME_LABEL } from "../features/welcome/model";
import {
  cancelUpdateDownload,
  checkForUpdates,
  downloadUpdate,
  installUpdate,
  setUpdateAutomatic,
  skipUpdateVersion,
  useUpdateStatus,
} from "../features/update/api";
import {
  AUTOMATIC_HINT,
  AUTOMATIC_LABEL,
  FULL_NOTES_TEXT,
  INSTALL_HEADING,
  MANUAL_HINT,
  PORTABLE_HINT,
  UPDATES_SECTION_ID,
  checkResultText,
  downloadText,
  installConfirmText,
  lastCheckText,
  releaseDateText,
  updateAction,
  type UpdateStatus,
} from "../features/update/model";
import type { AppSettings, CaptureSettingsInput, CaptureSource, CompleteFromGameMode } from "../features/capture/types";
import { getShipCatalogInfo } from "../features/ships/api";
import { catalogLine } from "../features/ships/model";
import { loadShips, useShips } from "../features/ships/store";
import type { CatalogInfo } from "../features/ships/types";
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
          description={SETTINGS_DESCRIPTION}
        />
        <div className="flex flex-col gap-6">
          <GameSection game={game} />
          <OcrSection game={game} />
          <UpdatesSection />
          <DataSection />
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

/** "100 ships, 2 custom · game … · Star Citizen Wiki" (custom ships from the shared ship store). */
function CatalogLine() {
  const [info, setInfo] = useState<CatalogInfo | null>(null);
  const [failed, setFailed] = useState(false);
  const { ships } = useShips();
  const customCount = ships.filter((s) => s.isCustom).length;
  useEffect(() => {
    let cancelled = false;
    void loadShips();
    getShipCatalogInfo()
      .then((next) => !cancelled && setInfo(next))
      .catch(() => !cancelled && setFailed(true));
    return () => {
      cancelled = true;
    };
  }, []);
  if (failed) return <span className="text-sm text-danger">Ship catalog could not be loaded</span>;
  return <span className="text-sm break-words text-fg">{info ? catalogLine(info, customCount) : "—"}</span>;
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

/** Settings → Updates: version and mode, the automatic check, Check now, the offer. */
function UpdatesSection() {
  const [status, setStatus] = useUpdateStatus();
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const act = async (job: () => Promise<UpdateStatus | void>) => {
    setError(null);
    try {
      const next = await job();
      if (next) setStatus(next);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  const offer = status?.offer ?? null;
  const action = status ? updateAction(status) : "none";
  const resultText = status?.checking ? "Checking…" : checkResultText(status?.result ?? null);
  const progress = status ? downloadText(status.download) : null;
  return (
    <SettingsSection index="03" title="Updates" id={UPDATES_SECTION_ID}>
      <Row label="Current version">
        <span className="font-mono text-sm text-fg">{status ? aboutVersionLine(status.currentVersion, status.mode) : "—"}</span>
      </Row>
      <Row label={AUTOMATIC_LABEL} hint={AUTOMATIC_HINT}>
        <Switch
          label={AUTOMATIC_LABEL}
          checked={status?.automatic ?? false}
          disabled={!status}
          onChange={(on) => void act(() => setUpdateAutomatic(on))}
        />
      </Row>
      <Row label="Check for updates" hint={status ? lastCheckText(status.lastCheckMs) : undefined}>
        <SmallButton onClick={() => void act(checkForUpdates)} disabled={!status || status.checking}>
          <span className="inline-flex items-center gap-1.5">
            <RefreshCw className={["size-3.5", status?.checking ? "animate-spin" : ""].join(" ")} aria-hidden="true" />
            Check now
          </span>
        </SmallButton>
      </Row>
      {resultText && (
        <p role="status" className="px-4 py-3 text-sm break-words text-fg">
          {resultText}
        </p>
      )}
      {offer && status && (
        <div className="space-y-3 px-4 py-3">
          <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
            <p className="text-sm font-semibold text-fg">Version {offer.version}</p>
            {releaseDateText(offer.publishedAt) && <p className="text-xs text-dim">{releaseDateText(offer.publishedAt)}</p>}
            {status.skippedVersion === offer.version && <p className="text-xs text-dim">Skipped</p>}
          </div>
          {offer.notes && (
            // Plain text: React escapes it, nothing is parsed as Markdown or HTML.
            <p className="max-h-60 overflow-y-auto font-mono text-xs leading-relaxed break-words whitespace-pre-wrap text-muted" data-release-notes>
              {offer.notes}
            </p>
          )}
          {offer.notesTruncated && <LinkButton onClick={() => void openLink("release").catch(() => undefined)}>{FULL_NOTES_TEXT}</LinkButton>}
          {progress && (
            <p role="status" className="text-sm break-words text-fg">
              {progress}
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            {action === "download" && <SmallButton onClick={() => void act(downloadUpdate)}>Download and install</SmallButton>}
            {action === "downloading" && <SmallButton onClick={() => void act(cancelUpdateDownload)}>Cancel</SmallButton>}
            {action === "installNow" && <SmallButton onClick={() => setConfirming(true)}>Install now</SmallButton>}
            {action === "openPage" && (
              <SmallButton onClick={() => void act(() => openLink("release"))}>
                <span className="inline-flex items-center gap-1.5">
                  Open download page <ExternalLink className="size-3.5" aria-hidden="true" />
                </span>
              </SmallButton>
            )}
            {status.skippedVersion !== offer.version && (
              <SmallButton onClick={() => void act(() => skipUpdateVersion(offer.version))}>Skip this version</SmallButton>
            )}
          </div>
          {action === "openPage" && <p className="text-xs break-words text-dim">{status.mode === "portable" ? PORTABLE_HINT : MANUAL_HINT}</p>}
        </div>
      )}
      {error && (
        <p role="alert" className="px-4 py-3 text-sm break-words text-danger">
          {error}
        </p>
      )}
      {confirming && offer && (
        <ConfirmDialog
          id="install-update"
          heading={INSTALL_HEADING}
          confirmLabel="Install now"
          onCancel={() => setConfirming(false)}
          onConfirm={async () => {
            try {
              await installUpdate(); // the app closes
              return { ok: true };
            } catch (e) {
              return { ok: false, message: e instanceof Error ? e.message : String(e) };
            }
          }}
        >
          <p>{installConfirmText(offer.version)}</p>
        </ConfirmDialog>
      )}
    </SettingsSection>
  );
}

type DataNote = { tone: "success" | "error"; text: string };

/** Data folder, Export backup, Restore backup. */
function DataSection() {
  const [info, setInfo] = useState<DataInfo | null>(null);
  const [note, setNote] = useState<DataNote | null>(null);
  const [busy, setBusy] = useState(false);
  const [restoring, setRestoring] = useState<{ path: string; info: BackupInfo } | null>(null);
  useEffect(() => {
    let cancelled = false;
    getDataInfo()
      .then((d) => !cancelled && setInfo(d))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  const fail = (e: unknown) => setNote({ tone: "error", text: refusalMessage(toCommandError(e) as { kind: string; reason?: string; message?: string }) });

  const exportNow = async () => {
    setNote(null);
    const path = await chooseBackupDestination().catch(() => null);
    if (!path) return; // cancelled: nothing happens
    setBusy(true);
    try {
      const result = await exportBackup(path);
      setNote({ tone: "success", text: exportNotice(result.fileName, result.info.bytes) });
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const restoreStart = async () => {
    setNote(null);
    const path = await chooseBackupToRestore().catch(() => null);
    if (!path) return;
    setBusy(true);
    try {
      setRestoring({ path, info: await inspectBackup(path) });
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingsSection index="04" title="Data">
      <Row label="Data folder" hint="Your database, screenshots and safety copies.">
        <div className="flex w-full min-w-0 items-center gap-2 sm:w-96">
          <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted" title={info?.folder}>
            {info ? shortenPath(info.folder) : "—"}
          </span>
          <SmallButton onClick={() => void openDataFolder().catch(fail)} disabled={!info}>
            Open data folder
          </SmallButton>
        </div>
      </Row>
      <Row label="Export backup" hint={`A copy of your contracts, history, settings and ships. ${SCREENSHOTS_NOT_INCLUDED}`}>
        <SmallButton onClick={() => void exportNow()} disabled={busy}>
          Export backup…
        </SmallButton>
      </Row>
      <Row
        label="Restore backup"
        hint={`Replaces all current data. A safety copy is kept in the backups folder${info?.safetyCopies ? ` (${info.safetyCopies} so far)` : ""}.`}
      >
        <SmallButton onClick={() => void restoreStart()} disabled={busy}>
          Restore backup…
        </SmallButton>
      </Row>
      {note && (
        <p role={note.tone === "error" ? "alert" : "status"} className={`px-4 py-2 text-xs break-words ${note.tone === "error" ? "text-danger" : "text-pickup"}`}>
          {note.text}
        </p>
      )}
      {restoring && (
        <ConfirmDialog
          id="restore-backup"
          heading="Restore backup"
          confirmLabel="Restore and restart"
          danger
          onCancel={() => setRestoring(null)}
          onConfirm={async () => {
            try {
              await restoreBackup(restoring.path); // the app restarts
              return { ok: true };
            } catch (e) {
              return { ok: false, message: refusalMessage(toCommandError(e) as { kind: string; reason?: string; message?: string }) };
            }
          }}
        >
          <p>{RESTORE_QUESTION}</p>
          <p className="mt-2 text-xs text-dim">{backupContents(restoring.info)}</p>
        </ConfirmDialog>
      )}
    </SettingsSection>
  );
}

function AboutSection() {
  const [about, setAbout] = useState<AboutInfo | null>(null);
  const [notices, setNotices] = useState(false);
  useEffect(() => {
    let cancelled = false;
    getAboutInfo()
      .then((a) => !cancelled && setAbout(a))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  const link = (key: LinkKey) => () => void openLink(key).catch(() => undefined);
  return (
    <SettingsSection index="05" title="About">
      <div className="space-y-1 px-4 py-4">
        <p className="text-sm font-semibold text-fg">
          Cargo Line{" "}
          <span className="font-mono text-xs font-normal text-muted">{about ? aboutVersionLine(about.version, about.mode) : ""}</span>
        </p>
        <p className="text-sm break-words text-muted">{ABOUT_DESCRIPTION}</p>
      </div>
      <Row label="Licence" hint={LICENCE_TEXT}>
        <LinkButton onClick={link("license")}>View licence</LinkButton>
      </Row>
      <Row label="Third-party notices" hint="Tesseract OCR (Apache-2.0) and the libraries bundled with it.">
        <SmallButton onClick={() => setNotices(true)} disabled={!about}>
          View third-party notices
        </SmallButton>
      </Row>
      <div className="space-y-1 px-4 py-3">
        <p className="text-sm text-fg">Data credits</p>
        <p className="text-xs break-words text-muted">
          {DATA_CREDIT} {DATA_TERMS}
        </p>
        <ul className="text-xs text-dim">
          {about?.catalogs.map((c) => (
            <li key={c.name} className="font-mono">
              {catalogCreditLine(c)}
            </li>
          ))}
        </ul>
        <LinkButton onClick={link("wikiApi")}>api.star-citizen.wiki</LinkButton>
      </div>
      <p className="px-4 py-3 text-xs break-words text-dim">{UNOFFICIAL}</p>
      <Row label="Welcome guide" hint="The short guide shown on first launch.">
        <SmallButton onClick={requestWelcome}>{SHOW_WELCOME_LABEL}</SmallButton>
      </Row>
      <Row label="Source code">
        <LinkButton onClick={link("repository")}>{REPOSITORY_LABEL}</LinkButton>
      </Row>
      {notices && about && <NoticesDialog text={about.thirdPartyNotices} onClose={() => setNotices(false)} />}
    </SettingsSection>
  );
}

function LinkButton({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" onClick={onClick} className="inline-flex items-center gap-1 text-sm text-accent underline-offset-2 hover:underline">
      {children}
      <ExternalLink className="size-3.5" aria-hidden="true" />
    </button>
  );
}

/** THIRD_PARTY_NOTICES.md (embedded at build time) as plain text with simple formatting. */
function NoticesDialog({ text, onClose }: { text: string; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    closeRef.current?.focus();
    return () => dialog?.close();
  }, []);
  const blocks = noticeBlocks(text);
  return (
    <dialog
      ref={ref}
      aria-labelledby="notices-title"
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      className="m-auto h-[min(40rem,calc(100%-2rem))] w-[min(46rem,calc(100%-2rem))] max-w-none flex-col overflow-hidden rounded-sm border border-line-strong bg-surface p-0 text-fg shadow-2xl backdrop:bg-black/70 open:flex"
    >
      <div className="flex items-center gap-3 border-b border-line px-4 py-3">
        <h2 id="notices-title" className="min-w-0 flex-1 text-[15px] font-semibold">
          Third-party notices
        </h2>
        <button ref={closeRef} type="button" onClick={onClose} className="h-8 rounded-sm border border-line px-3 text-sm text-muted hover:border-line-strong hover:text-fg">
          Close
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3" tabIndex={0} aria-label="Notices text">
        {blocks.map((b, i) =>
          b.kind === "heading" ? (
            <p key={i} className={b.level <= 2 ? "mt-4 text-sm font-semibold text-fg first:mt-0" : "mt-3 text-xs font-semibold text-muted"}>
              {b.text}
            </p>
          ) : b.kind === "rule" ? (
            <hr key={i} className="my-3 border-line" />
          ) : (
            <pre key={i} className="mt-2 font-mono text-[11px] leading-relaxed break-words whitespace-pre-wrap text-muted">
              {b.text}
            </pre>
          ),
        )}
      </div>
    </dialog>
  );
}

/* ---------- local building blocks ---------- */

function SettingsSection({ index, title, id, children }: { index: string; title: string; id?: string; children: ReactNode }) {
  const headingId = useId();
  return (
    <section id={id} aria-labelledby={headingId} className="rounded-sm border border-line bg-surface">
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
