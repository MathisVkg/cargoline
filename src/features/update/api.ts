import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UpdateStatus } from "./model";

/** Sent by Rust whenever the status changes (check done, download progress). */
const STATUS_EVENT = "update-status";

/** The update commands answer with a plain string on failure. */
const message = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : String(e));

async function run<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw new Error(message(e));
  }
}

export const getUpdateStatus = () => run<UpdateStatus>("get_update_status");
export const checkForUpdates = () => run<UpdateStatus>("check_for_updates");
export const setUpdateAutomatic = (enabled: boolean) => run<UpdateStatus>("set_update_automatic", { enabled });
export const skipUpdateVersion = (version: string) => run<UpdateStatus>("skip_update_version", { version });
export const downloadUpdate = () => run<void>("download_update");
export const cancelUpdateDownload = () => run<void>("cancel_update_download");
/** Closes the app and runs the installer (the promise may never resolve). */
export const installUpdate = () => run<void>("install_update");

/**
 * The current status, kept up to date by the `update-status` event (no polling). With
 * `fetchNow: false` it waits for the first event (the top-bar marker: an offer only exists after
 * a check, and every check sends the event), so it adds no round trip at startup.
 */
export function useUpdateStatus(fetchNow = true): [UpdateStatus | null, (s: UpdateStatus) => void] {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listen<UpdateStatus>(STATUS_EVENT, (e) => !cancelled && setStatus(e.payload)).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    if (fetchNow) {
      getUpdateStatus()
        .then((s) => !cancelled && setStatus(s))
        .catch(() => undefined);
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [fetchNow]);
  return [status, setStatus];
}
