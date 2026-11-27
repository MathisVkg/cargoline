// Startup timing marks from the frontend. Off unless the app was started with
// CARGO_LINE_STARTUP_TRACE=1: the first batch asks the backend, and when tracing is off nothing
// more is recorded or sent. Marks are names only (no data).
import { invoke } from "@tauri-apps/api/core";

export interface TraceMark {
  name: string;
  /** Wall-clock time, Unix epoch ms. */
  at: number;
}

type Send = (marks: TraceMark[]) => Promise<boolean>;

/** A small batching recorder; `send` reports whether tracing is on. */
export function createTracer(send: Send, now: () => number, schedule: (fn: () => void) => void) {
  let on: boolean | null = null; // unknown until the first answer
  let queue: TraceMark[] = [];
  let scheduled = false;
  const seen = new Set<string>();

  const flush = () => {
    scheduled = false;
    if (on === false || queue.length === 0) return;
    const batch = queue;
    queue = [];
    send(batch)
      .then((enabled) => {
        on = enabled;
        if (!enabled) queue = [];
      })
      .catch(() => {
        on = false; // outside Tauri, or an older backend
        queue = [];
      });
  };

  const mark = (name: string) => {
    if (on === false) return;
    queue.push({ name, at: now() });
    if (!scheduled) {
      scheduled = true;
      schedule(flush);
    }
  };

  return {
    mark,
    /** Only the first time this name is used. */
    markOnce(name: string) {
      if (seen.has(name)) return;
      seen.add(name);
      mark(name);
    },
    /** For tests: true, false, or null while unknown. */
    state: () => on,
  };
}

const tracer = createTracer(
  (marks) => invoke<boolean>("trace_marks", { marks }),
  () => performance.timeOrigin + performance.now(),
  (fn) => window.setTimeout(fn, 100),
);

export const traceMark = tracer.mark;
export const traceOnce = tracer.markOnce;
