// The entry point for the guest's web-API globals, bundled by
// `bundle-web-globals.mjs` and evaluated by the guest before a deployment's
// modules are.
//
// These come from `udf-runtime`, the same sources the V8 runtime installs.
import { setupDate } from "udf-runtime/src/00_date.js";
import { setupMisc } from "udf-runtime/src/00_misc";
import { setupWeakRefs } from "udf-runtime/src/00_weakref";
import { setupConsole } from "udf-runtime/src/02_console";
import { setupStructuredClone } from "udf-runtime/src/02_structured_clone";
import { setupEvent } from "udf-runtime/src/02_event";
import { setupTimers } from "udf-runtime/src/02_timers";
import { setupStreams } from "udf-runtime/src/06_streams";
import { setupBlob } from "udf-runtime/src/09_file";
import { setupAbortSignal } from "udf-runtime/src/03_abort_signal";
import { setupAsyncHooks } from "udf-runtime/src/04_async_hooks";
import { setupHeaders } from "udf-runtime/src/20_headers";
import { setupFormData } from "udf-runtime/src/21_formdata";
import { setupRequest } from "udf-runtime/src/23_request";
import { setupResponse } from "udf-runtime/src/23_response";
import { setupPerformance } from "udf-runtime/src/27_performance";
import { setupJsSyscall } from "udf-runtime/src/js_syscall";
import { setupTextEncodingStreams } from "./text-encoding-streams";

setupDate(globalThis);
setupMisc(globalThis);
setupWeakRefs(globalThis);
setupConsole(globalThis);
setupStructuredClone(globalThis);
setupEvent(globalThis);
setupTimers(globalThis);
setupStreams(globalThis);
setupTextEncodingStreams(globalThis);
setupAbortSignal(globalThis);
setupAsyncHooks(globalThis);
setupBlob(globalThis);
setupHeaders(globalThis);
setupFormData(globalThis);
setupRequest(globalThis);
setupResponse(globalThis);
setupPerformance(globalThis);
setupJsSyscall(globalThis);

// TODO: `fetch` in actions, which needs the host to carry the request and
// response bodies' streams. Until then actions fail the way queries do.
globalThis.fetch = async function fetch(_input: unknown) {
  throw new TypeError(
    "Can't use fetch() in queries and mutations. Please consider using an action. See https://docs.convex.dev/functions/actions for more details.",
  );
};
