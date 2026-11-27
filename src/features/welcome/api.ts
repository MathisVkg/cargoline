import { call } from "../contracts/api";

/** Records that this data folder has seen the welcome guide (it then never opens by itself). */
export const markWelcomeSeen = () => call<void>("mark_welcome_seen");

/** Settings → About asks the app shell to show the guide again (the shell owns the modals). */
export const SHOW_WELCOME_EVENT = "cargo-line:show-welcome";
export const requestWelcome = () => window.dispatchEvent(new Event(SHOW_WELCOME_EVENT));
