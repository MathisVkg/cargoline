import { convertFileSrc } from "@tauri-apps/api/core";
import { call } from "../contracts/api";
import type { AppSettings, CaptureInfo, CaptureSettingsInput, OcrResult } from "./types";

export function getSettings(): Promise<AppSettings> {
  return call<AppSettings>("get_settings");
}

/** Validates and saves the OCR Capture settings (all or nothing). */
export function updateCaptureSettings(input: CaptureSettingsInput): Promise<AppSettings> {
  return call<AppSettings>("update_capture_settings", { input });
}

/** Captures Star Citizen per the settings; `replaces` (the previous unsaved capture) is deleted on success. */
export function captureScreenshot(replaces: number | null): Promise<CaptureInfo> {
  return call<CaptureInfo>("capture_screenshot", { replaces });
}

/** Deletes an unsaved capture (row and file). Linked captures are kept. */
export function discardCapture(id: number): Promise<boolean> {
  return call<boolean>("discard_capture", { id });
}

/** Image URL served by the backend's `capture:` protocol (captures folder only). */
export function captureImageUrl(id: number): string {
  return convertFileSrc(String(id), "capture");
}

/** Reads the text of a stored capture. The text is returned only, never stored. */
export function recognizeCapture(captureId: number): Promise<OcrResult> {
  return call<OcrResult>("recognize_capture", { captureId });
}

/** Cancels a running recognition (form closed, or a recapture started). */
export function cancelRecognition(captureId: number): Promise<boolean> {
  return call<boolean>("cancel_recognition", { captureId });
}
