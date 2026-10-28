// Implementation of Timer spec defined at
// https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html

import { throwNotImplementedMethodError } from "./helpers";
import { performAsyncOp } from "./syscall";

const ACTIVE_TIMERS = new Map();

let nextTimerId = 1;

const setTimeout = (
  handler: (...args: any[]) => void,
  timeoutMs: number,
  ...args: any[]
): number => {
  return timerInitialization("setTimeout", handler, timeoutMs, args, false);
};

const setInterval = (
  handler: (...args: any[]) => void,
  timeoutMs: number,
  ...args: any[]
): number => {
  return timerInitialization("setInterval", handler, timeoutMs, args, true);
};

const clearTimeout = (id: number) => {
  ACTIVE_TIMERS.delete(id);
};
const clearInterval = (id: number) => {
  ACTIVE_TIMERS.delete(id);
};

const timerInitialization = (
  name: string,
  handler: (...args: any[]) => void,
  timeoutMs: number,
  args: any[],
  repeat: boolean,
  previousId?: number,
): number => {
  if (typeof handler === "string") {
    throwNotImplementedMethodError("code string argument", name);
  }
  const id = previousId ?? nextTimerId++;
  // TODO(CX-4532) calculate nesting level and use it to lower bound timeout.
  timeoutMs = timeoutMs ? Number(timeoutMs) : 0;
  if (timeoutMs < 0) {
    timeoutMs = 0;
  }
  const task = () => {
    if (!ACTIVE_TIMERS.has(id)) {
      return;
    }
    handler(...args);
    if (!ACTIVE_TIMERS.has(id)) {
      return;
    }
    if (repeat) {
      timerInitialization(name, handler, timeoutMs, args, true, id);
    } else {
      ACTIVE_TIMERS.delete(id);
    }
  };
  runAfterTimeout(name, timeoutMs, task, id);
  return id;
};

const runAfterTimeout = (
  name: string,
  timeoutMs: number,
  task: () => void,
  timerKey: number,
) => {
  const startTime = Date.now();
  ACTIVE_TIMERS.set(timerKey, startTime + timeoutMs);
  void performAsyncOp("sleep", name, timeoutMs).then(() => {
    // TODO(CX-4534) Wait until any invocations of this algorithm,
    // that started before this one, and whose milliseconds is equal to or less
    // than this one's, have completed.

    task();
  });
};

export const setupTimers = (global) => {
  global.setTimeout = setTimeout;
  global.setInterval = setInterval;
  global.clearTimeout = clearTimeout;
  global.clearInterval = clearInterval;
};
