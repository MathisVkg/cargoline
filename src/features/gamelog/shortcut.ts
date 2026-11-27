// Capture shortcut: recording and matching while Cargo Line has focus. Mirrors
// `validate_shortcut` in src-tauri/src/settings/models.rs.

export interface KeyLike {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/** Combinations the app or the web view already uses. */
export const RESERVED_SHORTCUTS = [
  "Ctrl+A", "Ctrl+C", "Ctrl+V", "Ctrl+X", "Ctrl+Z", "Ctrl+Y", "Ctrl+Shift+Z", "Ctrl+R", "Ctrl+Shift+R", "Ctrl+F",
  "Ctrl+P", "Ctrl+W", "Ctrl+S", "Alt+F4", "Ctrl+Shift+I", "Ctrl+Shift+J",
];

/**
 * The combination as text (`Ctrl+Shift+C`), or null while only modifiers are held.
 * Letters follow the keyboard layout (an AZERTY "A" is "A"); digits use their key position so
 * they work without Shift on layouts like AZERTY.
 */
export function shortcutFromKey(e: KeyLike): string | null {
  let key: string | null = null;
  if (/^Digit[0-9]$/.test(e.code)) key = e.code.slice(5);
  else if (/^F([1-9]|1[0-2])$/.test(e.key)) key = e.key;
  else if (e.key.length === 1 && /^[a-z]$/i.test(e.key)) key = e.key.toUpperCase();
  else if (/^Key[A-Z]$/.test(e.code) && e.key.length !== 1) key = e.code.slice(3);
  if (!key || e.metaKey) return null;
  return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", key].filter(Boolean).join("+");
}

/** Null when acceptable, otherwise the reason. */
export function shortcutProblem(shortcut: string): string | null {
  const parts = shortcut.split("+");
  const key = parts.at(-1) ?? "";
  const modifiers = parts.slice(0, -1);
  if (!/^([A-Z0-9]|F([1-9]|1[0-2]))$/.test(key)) return "Use a letter, a digit or F1–F12 with Ctrl or Alt.";
  if (!modifiers.includes("Ctrl") && !modifiers.includes("Alt")) return "Add Ctrl or Alt: a single key would be typed into fields.";
  if (RESERVED_SHORTCUTS.includes(shortcut)) return `${shortcut} is already used by the app.`;
  return null;
}

export function matchesShortcut(e: KeyLike, shortcut: string | null): boolean {
  return !!shortcut && shortcutFromKey(e) === shortcut;
}
