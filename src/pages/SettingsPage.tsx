import { useEffect, useId, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { RefreshCw } from "lucide-react";
import { DisabledButton, PageContainer, PageHeader, Tag } from "../components/ui";

export function SettingsPage() {
  return (
    <PageContainer>
      <div className="mx-auto max-w-3xl">
        <PageHeader
          index="04"
          title="Settings"
          description="Settings are placeholders in this version and cannot be changed yet."
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

function OcrSection() {
  const delayId = useId();
  return (
    <SettingsSection index="02" title="OCR Capture">
      <Row label="Capture source">
        <select
          disabled
          aria-label="Capture source"
          className="h-9 w-full rounded-sm border border-line bg-raised px-2 text-sm text-dim disabled:cursor-not-allowed sm:w-64"
        >
          <option>Star Citizen window</option>
          <option>Full display</option>
        </select>
      </Row>
      <Row label="Capture shortcut" hint="Works while Cargo Line is focused.">
        <DisabledText label="Capture shortcut" placeholder="Not configured" />
      </Row>
      <Row label="Automatic capture on accept" hint="Uses Game.log events. Off by default.">
        <Switch label="Automatic capture on accept" />
      </Row>
      <Row label="Capture delay" hint="0.0 – 5.0 seconds, 0.1 s steps.">
        <div className="flex w-full items-center gap-3 sm:w-64">
          <input
            id={delayId}
            type="range"
            min={0}
            max={5}
            step={0.1}
            defaultValue={0}
            disabled
            aria-label="Capture delay in seconds"
            className="min-w-0 flex-1 accent-line-strong disabled:cursor-not-allowed"
          />
          <span className="font-mono text-xs whitespace-nowrap text-dim">— s</span>
        </div>
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

function Switch({ label }: { label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={false}
      aria-label={label}
      disabled
      className="relative h-5 w-9 rounded-full border border-line bg-raised disabled:cursor-not-allowed"
    >
      <span aria-hidden="true" className="absolute top-0.5 left-0.5 size-3.5 rounded-full bg-line-strong" />
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
