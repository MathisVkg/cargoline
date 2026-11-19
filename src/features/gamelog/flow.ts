// Pure state machine for Game.log events and the Add Contract modal (Task 10). No React, no IPC.
//
// - Accepted + automatic capture, modal closed → open Add Contract attached to the event.
// - Accepted + automatic capture, modal open → queue (arrival order); the next opens on close.
// - Accepted, automatic capture off → a notice "Accepted in game: <title> · Add contract".
// - Ended (active contract) → a notice with Mark complete / Abandon. Never acts on its own.
// - The capture shortcut: closed → open with capture; Add → recapture; Edit → nothing.
import type { AcceptedEvent, FirstObjective, GameLogEvent } from "./types.ts";

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
  | { type: "shortcut" };

const CLOSED: ModalState = { open: false, kind: null, event: null, auto: false, forceCapture: false, session: 0 };

export const initialFlow: FlowState = { modal: CLOSED, queue: [], notices: [], handled: [], dismissed: [], recapture: 0 };

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
      if (!state.modal.open) return openAdd({ ...state, handled }, event, true, true);
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
      return state.modal.open ? state : openAdd(state, null, false, action.forceCapture ?? false);
    case "openFromNotice": {
      const notice = state.notices.find((n) => n.id === action.noticeId);
      if (!notice || notice.kind !== "accepted" || state.modal.open) return state;
      const rest = { ...state, notices: state.notices.filter((n) => n.id !== notice.id), dismissed: [...state.dismissed, notice.id] };
      // The user chose to add it: normal capture (per the setting), not an automatic one.
      return openAdd(rest, notice.event, false, false);
    }
    case "editOpened":
      return state.modal.open ? state : { ...state, modal: { ...CLOSED, open: true, kind: "edit", session: state.modal.session + 1 } };
    case "closed": {
      const closed = { ...state, modal: { ...CLOSED, session: state.modal.session } };
      const [next, ...queue] = state.queue;
      return next ? openAdd({ ...closed, queue }, next, true, true) : closed;
    }
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
    case "shortcut":
      if (!state.modal.open) return openAdd(state, null, false, true);
      if (state.modal.kind === "add") return { ...state, recapture: state.recapture + 1 };
      return state; // Edit mode: nothing
  }
}

/** "Accepted in game: <title>" for the queue bar, with "+N more waiting". */
export function queueSummary(queue: readonly AcceptedEvent[]): { first: AcceptedEvent; more: number } | null {
  return queue.length ? { first: queue[0], more: queue.length - 1 } : null;
}
