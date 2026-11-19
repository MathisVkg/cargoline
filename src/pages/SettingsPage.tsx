import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { RefreshCw } from "lucide-react";
import { DisabledButton, PageContainer, PageHeader, Tag } from "../components/ui";
import { getSettings, updateCaptureSettings } from "../features/capture/api";
import type { CaptureSettingsInput, CaptureSource } from "../features/capture/types";
import { describeError, toCommandError } from "../features/contracts/api";

export function SettingsPage() {
  return (
    <PageContainer>
      <div className="mx-auto max-w-3xl">
        <PageHeader
          index="04"
          title="Settings"
          description="OCR Capture settings are saved on this computer. The other settings are placeholders in this version."
        />
        <div className="flex flex-col gap-6">
          <GameSection />
          <OcrSection />
          <UpdatesSection />
          <AboutSection />
        </div>
      </div>
    </PageContainer>
  );
}

function GameSection() {
  return (
    <SettingsSection index="01" title="Game">
      <Row label="Game.log monitoring" hint="Optional. Never required for managing contracts.">
        <Switch label="Game.log monitoring" />
      </Row>
      <Row label="Game.log file path">
        <DisabledText label="Game.log file path" placeholder="Not configured" />
      </Row>
      <Row label="Ship catalog" hint="Community-sourced, bundled offline.">
        <span className="text-sm text-dim">Not bundled yet · 0 ships</span>
      </Row>
    </SettingsSection>
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

function OcrSection() {
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
      <Row label="Capture shortcut" hint="Works while Cargo Line is focused. Not available yet.">
        <DisabledText label="Capture shortcut" placeholder="Not configured" />
      </Row>
      <Row label="Automatic capture on accept" hint="Uses Game.log events. Not available yet.">
        <Switch label="Automatic capture on accept" />
      </Row>
    </SettingsSection>
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
      <Row label="Ship data attribution" hint="Community sources will be credited here.">
        <Tag>No data bundled</Tag>
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

function DisabledText({ label, placeholder }: { label: string; placeholder: string }) {
  return (
    <input
      type="text"
      disabled
      aria-label={label}
      placeholder={placeholder}
      className="h-9 w-full min-w-0 rounded-sm border border-line bg-raised px-3 text-sm text-dim placeholder:text-dim disabled:cursor-not-allowed sm:w-64"
    />
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
