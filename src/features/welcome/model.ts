// The welcome guide shown once on first launch (and from Settings → About). Pure texts and rules.

export const WELCOME_TITLE = "Welcome to Cargo Line";
export const WELCOME_INTRO = "Cargo Line tracks your hauling contracts, offline, on your PC.";
export const WELCOME_STEPS = [
  "Pick your ship in the top bar.",
  "Optional: connect the Game.log and set the capture shortcut in Settings.",
  "Add a contract from a screenshot, or by hand, with Add Contract.",
  "Tick Pick Up and Turn In as you go; Cargo shows what goes where.",
  "Complete the contract when you're done; History shows your earnings.",
] as const;
export const WELCOME_PRIMARY = "Got it";
export const WELCOME_SECONDARY = "Open Settings";
export const SHOW_WELCOME_LABEL = "Show the welcome guide";

/**
 * Whether the guide opens by itself at startup: only while this data folder hasn't seen it.
 * It is marked as seen as soon as it opens, so every way of closing it counts (Got it, Open
 * Settings, Esc, the capture shortcut, even closing the window).
 */
export function welcomeOnStartup(settings: { welcome_seen?: boolean } | null): boolean {
  return settings !== null && settings.welcome_seen !== true;
}
