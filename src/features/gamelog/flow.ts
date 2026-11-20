// Pure state machine for Game.log events and the Add Contract modal (Task 10). No React, no IPC.
//
// - Accepted + automatic capture, modal closed → open Add Contract attached to the event.
// - Accepted + automatic capture, modal open → queue (arrival order); the next opens on close.
// - Accepted, automatic capture off → a notice "Accepted in game: <title> · Add contract".
// - Ended (active contract) → a notice with Mark complete / Abandon. Never acts on its own,
//   except a MISSION_STATE_COMPLETED end while "Complete contracts finished in game" is on
//   (Task 11 Part B): batched (~2 s) into the Ask dialog, or finalised automatically with Undo.
//   Each MissionId acts once; any other end state stays a notice.
// - The capture shortcut: closed → open with capture; Add → recapture; Edit → nothing.
import type { AcceptedEvent, FirstObjective, GameLogEvent } from "./types.ts";
import { MISSION_STATE_COMPLETED } from "./types.ts";

export interface ModalState {
  open: boolean;
  kind: "add" | "edit" | null;
  /** The in-game contract this form is for (its MissionId is saved with it). */
  event: AcceptedEvent | null;
  /** Opened automatically by a Game.log accept: saved with source `auto_capture`. */
  auto: boolean;
  /** Capture even if "Capture when opening Add Contract" is off (log event, shortcut). */
  forceCapture: boolean;
  /** Changes on every open (remounts the modal). */
  session: number;
}

export type FlowNotice =
  | { id: string; kind: "accepted"; event: AcceptedEvent }
  | { id: string; kind: "ended"; missionId: string; contractId: number; state: string };

/** A contract the game reports as completed, waiting for the Ask dialog or the automatic run. */
export interface Completion {
  missionId: string;
  contractId: number;
  mode: "ask" | "automatic";
}

export interface CompletionState {
  /** MissionIds already acted on (each acts once). */
  acted: string[];
  /** Collected during the ~2 s batching window (or while another modal is open). */
  waiting: Completion[];
  /** The Ask dialog, when open. New completions are added to it. */
  dialog: Completion[] | null;
  /** The automatic finalisation in progress. `batch` changes on every run. */
  running: { batch: number; items: Completion[] } | null;
  /** Number of automatic runs started (the next batch id). */
  batches: number;
  /** Another confirmation dialog is open on the page (the Ask dialog waits). */
  blocking: boolean;
}

/** "C06 completed from game" with Undo (automatic) or without (after Ask). */
export interface CompletionBanner {
  id: number;
  contractIds: number[];
  undo: boolean;
}

export interface FlowState {
  modal: ModalState;
  queue: AcceptedEvent[];
  notices: FlowNotice[];
  /** MissionIds already handled (dedupe of accepts). */
  handled: string[];
  /** Notice ids dismissed or acted on: never shown again. */
  dismissed: string[];
  /** Incremented to ask the open Add modal to recapture. */
  recapture: number;
  completion: CompletionState;
  banner: CompletionBanner | null;
  /** Incremented when contracts changed outside the page (it reloads its list). */
  listVersion: number;
}

export type FlowAction =
  | { type: "gameLog"; event: GameLogEvent }
  | { type: "openAdd"; forceCapture?: boolean }
  | { type: "openFromNotice"; noticeId: string }
  | { type: "editOpened" }
  | { type: "closed" }
  | { type: "captureNow"; missionId: string }
  | { type: "dismiss"; noticeId: string }
  | { type: "saved"; missionId: string | null }
  | { type: "shortcut" }
  /** The batching window ended: open the Ask dialog / start the automatic run when possible. */
  | { type: "completionDue" }
  /** Another confirmation dialog opened or closed on the page. */
  | { type: "blocking"; open: boolean }
  /** The Ask dialog closed: `completed` were finalised; every other item becomes a notice. */
  | { type: "completionDialogDone"; completed: number[]; now: number }
  /** The automatic run finished: `completed` were finalised; `failed` become notices. */
  | { type: "autoCompleted"; batch: number; completed: number[]; failed: Completion[]; now: number }
  | { type: "bannerDismiss"; id: number }
  /** Undo succeeded. */
  | { type: "undone"; id: number }
  /** Contracts were finalised, restored or cleared on a page: every page reloads its list. */
  | { type: "contractsChanged" };

const CLOSED: ModalState = { open: false, kind: null, event: null, auto: false, forceCapture: false, session: 0 };

const NO_COMPLETIONS: CompletionState = { acted: [], waiting: [], dialog: null, running: null, batches: 0, blocking: false };

export const initialFlow: FlowState = {
  modal: CLOSED,
  queue: [],
  notices: [],
  handled: [],
  dismissed: [],
  recapture: 0,
  completion: NO_COMPLETIONS,
  banner: null,
  listVersion: 0,
};

/** The Ask dialog counts as a modal for the Add Contract flow (they never stack). */
const askOpen = (state: FlowState) => state.completion.dialog !== null;

/** Completions the user did not confirm go back to the normal "completed in game" notices. */
function asNotices(state: FlowState, items: readonly Completion[]): FlowNotice[] {
  const notices = [...state.notices];
  for (const c of items) {
    const id = `ended:${c.missionId}`;
    if (state.dismissed.includes(id) || notices.some((n) => n.id === id)) continue;
    notices.push({ id, kind: "ended", missionId: c.missionId, contractId: c.contractId, state: MISSION_STATE_COMPLETED });
  }
  return notices;
}

/** After a modal closes, the next queued accept opens. */
function openQueued(state: FlowState): FlowState {
  if (state.modal.open || askOpen(state)) return state;
  const [next, ...queue] = state.queue;
  return next ? openAdd({ ...state, queue }, next, true, true) : state;
}

const openAdd = (state: FlowState, event: AcceptedEvent | null, auto: boolean, forceCapture: boolean): FlowState => ({
  ...state,
  modal: { open: true, kind: "add", event, auto, forceCapture, session: state.modal.session + 1 },
});

const withObjective = (event: AcceptedEvent, missionId: string, objective: FirstObjective): AcceptedEvent =>
  event.missionId === missionId && !event.objective ? { ...event, objective } : event;

function onGameLog(state: FlowState, event: GameLogEvent): FlowState {
  switch (event.kind) {
    case "accepted": {
      if (state.handled.includes(event.missionId)) return state;
      const handled = [...state.handled, event.missionId];
      if (!event.autoCapture) {
        return { ...state, handled, notices: [...state.notices, { id: `accepted:${event.missionId}`, kind: "accepted", event }] };
      }
      if (!state.modal.open && !askOpen(state)) return openAdd({ ...state, handled }, event, true, true);
      return { ...state, handled, queue: [...state.queue, event] };
    }
    case "objective": {
      const { missionId, objective } = event;
      return {
        ...state,
        modal: state.modal.event ? { ...state.modal, event: withObjective(state.modal.event, missionId, objective) } : state.modal,
        queue: state.queue.map((e) => withObjective(e, missionId, objective)),
        notices: state.notices.map((n) => (n.kind === "accepted" ? { ...n, event: withObjective(n.event, missionId, objective) } : n)),
      };
    }
    case "ended": {
      const mode = event.completeFromGame;
      if (event.state === MISSION_STATE_COMPLETED && (mode === "ask" || mode === "automatic")) {
        const c = state.completion;
        if (c.acted.includes(event.missionId)) return state; // each MissionId acts once
        const item: Completion = { missionId: event.missionId, contractId: event.contractId, mode };
        const acted = [...c.acted, event.missionId];
        if (item.mode === "ask" && c.dialog) {
          const dialog = c.dialog.some((d) => d.contractId === item.contractId) ? c.dialog : [...c.dialog, item];
          return { ...state, completion: { ...c, acted, dialog } };
        }
        return { ...state, completion: { ...c, acted, waiting: [...c.waiting, item] } };
      }
      const id = `ended:${event.missionId}`;
      if (state.dismissed.includes(id) || state.notices.some((n) => n.id === id)) return state;
      return {
        ...state,
        notices: [...state.notices, { id, kind: "ended", missionId: event.missionId, contractId: event.contractId, state: event.state }],
      };
    }
  }
}

export function reduceFlow(state: FlowState, action: FlowAction): FlowState {
  switch (action.type) {
    case "gameLog":
      return onGameLog(state, action.event);
    case "openAdd":
      return state.modal.open || askOpen(state) ? state : openAdd(state, null, false, action.forceCapture ?? false);
    case "openFromNotice": {
      const notice = state.notices.find((n) => n.id === action.noticeId);
      if (!notice || notice.kind !== "accepted" || state.modal.open) return state;
      const rest = { ...state, notices: state.notices.filter((n) => n.id !== notice.id), dismissed: [...state.dismissed, notice.id] };
      // The user chose to add it: normal capture (per the setting), not an automatic one.
      return openAdd(rest, notice.event, false, false);
    }
    case "editOpened":
      return state.modal.open ? state : { ...state, modal: { ...CLOSED, open: true, kind: "edit", session: state.modal.session + 1 } };
    case "closed":
      return openQueued({ ...state, modal: { ...CLOSED, session: state.modal.session } });
    case "captureNow": {
      const event = state.queue.find((e) => e.missionId === action.missionId);
      if (!event || state.modal.kind !== "add") return state;
      return {
        ...state,
        queue: state.queue.filter((e) => e !== event),
        modal: { ...state.modal, event, auto: true },
        recapture: state.recapture + 1,
      };
    }
    case "dismiss":
      return {
        ...state,
        notices: state.notices.filter((n) => n.id !== action.noticeId),
        dismissed: [...state.dismissed, action.noticeId],
      };
    case "saved": {
      if (!action.missionId) return state;
      const ids = [`accepted:${action.missionId}`];
      return { ...state, notices: state.notices.filter((n) => !ids.includes(n.id)), dismissed: [...state.dismissed, ...ids] };
    }
    case "completionDue": {
      const c = state.completion;
      if (c.waiting.length === 0) return state;
      let next: CompletionState = c;
      const auto = c.waiting.filter((w) => w.mode === "automatic");
      if (auto.length > 0 && !c.running) {
        next = { ...next, running: { batch: c.batches + 1, items: auto }, batches: c.batches + 1 };
      }
      // The Ask dialog waits while another modal is open.
      const ask = c.waiting.filter((w) => w.mode === "ask");
      const busy = state.modal.open || c.blocking || c.dialog !== null;
      if (ask.length > 0 && !busy) next = { ...next, dialog: ask };
      if (next === c) return state;
      const moved = new Set<Completion>([...(next.running?.items ?? []), ...(next.dialog ?? [])]);
      return { ...state, completion: { ...next, waiting: c.waiting.filter((w) => !moved.has(w)) } };
    }
    case "blocking":
      return state.completion.blocking === action.open
        ? state
        : { ...state, completion: { ...state.completion, blocking: action.open } };
    case "completionDialogDone": {
      const dialog = state.completion.dialog;
      if (!dialog) return state;
      const done = new Set(action.completed);
      const rest = dialog.filter((d) => !done.has(d.contractId));
      const any = action.completed.length > 0;
      return openQueued({
        ...state,
        notices: asNotices(state, rest),
        completion: { ...state.completion, dialog: null },
        banner: any ? { id: action.now, contractIds: action.completed, undo: false } : state.banner,
        listVersion: any ? state.listVersion + 1 : state.listVersion,
      });
    }
    case "autoCompleted": {
      if (state.completion.running?.batch !== action.batch) return state;
      const any = action.completed.length > 0;
      return {
        ...state,
        notices: asNotices(state, action.failed),
        completion: { ...state.completion, running: null },
        banner: any ? { id: action.now, contractIds: action.completed, undo: true } : state.banner,
        listVersion: state.listVersion + 1,
      };
    }
    case "bannerDismiss":
      return state.banner?.id === action.id ? { ...state, banner: null } : state;
    case "contractsChanged":
      return { ...state, listVersion: state.listVersion + 1 };
    case "undone":
      return state.banner?.id === action.id ? { ...state, banner: null, listVersion: state.listVersion + 1 } : state;
    case "shortcut":
      if (askOpen(state)) return state;
      if (!state.modal.open) return openAdd(state, null, false, true);
      if (state.modal.kind === "add") return { ...state, recapture: state.recapture + 1 };
      return state; // Edit mode: nothing
  }
}

/** "Accepted in game: <title>" for the queue bar, with "+N more waiting". */
export function queueSummary(queue: readonly AcceptedEvent[]): { first: AcceptedEvent; more: number } | null {
  return queue.length ? { first: queue[0], more: queue.length - 1 } : null;
}

/** "C06 completed from game" / "3 contracts completed from game". */
export function completionText(refs: readonly string[]): string {
  return refs.length === 1 ? `${refs[0]} completed from game` : `${refs.length} contracts completed from game`;
}

/** How long the banner stays: 30 s with Undo, 6 s after the Ask dialog. */
export const UNDO_MS = 30_000;
export const BANNER_MS = 6_000;
/** The batching window for completions arriving together. */
export const COMPLETION_BATCH_MS = 2_000;
