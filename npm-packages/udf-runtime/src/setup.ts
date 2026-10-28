import { setupURL } from "./00_url.js";
import { setupCrypto } from "./00_crypto.js";
import { setupDate } from "./00_date.js";
import { setupMisc } from "./00_misc.js";
import { patchDateTimeFormat, setupTemporal } from "./00_temporal.js";
import { setupWeakRefs } from "./00_weakref.js";
import { setupDOMException } from "./01_dom_exception.js";
import { setupConsole } from "./02_console";
import { setupEvent } from "./02_event";
import { setupTimers } from "./02_timers.js";
import { setupAbortSignal } from "./03_abort_signal.js";
import { setupAsyncHooks } from "./04_async_hooks.js";
import { setupStreams } from "./06_streams.js";
import { setupTextEncoding } from "./08_text_encoding.js";
import { setupBlob } from "./09_file.js";
import { setupHeaders } from "./20_headers.js";
import { setupFormData } from "./21_formdata.js";
import { setupRequest } from "./23_request.js";
import { setupResponse } from "./23_response.js";
import { setupFetch } from "./26_fetch.js";
import { setupPerformance } from "./27_performance.js";
import { setupSourceMapping } from "./errors.js";
import { setupJsSyscall } from "./js_syscall.js";
import { setupStructuredClone } from "./02_structured_clone.js";

/**
 * Set up the global object for a UDF context with deterministic Convex APIs.
 *
 * This initializes certain JS globals, and patches existing globals such as the
 * `Math.random` function, the `Date` object, and the `console` object.
 */
export function setup(global: any) {
  setupSourceMapping();
  setupDate(global);
  patchDateTimeFormat(global, global.Date.now);
  // V8 installs Temporal when deserializing a context, after snapshot setup.
  global.Convex.setupTemporal = () => setupTemporal(global);
  // NB: It's important we call into `setupMisc` before the other setup functions
  // since those may call into 3rd party libraries we bundle, which may then
  // retain references to globals we modify, like `Date` or `FinalizationRegistry`.
  setupMisc(global);
  setupWeakRefs(global);

  // These need to be set up in order of the numbers in their filenames (taken
  // from Deno) since later ones depend on the earlier ones.
  setupURL(global);
  setupCrypto(global);
  setupDOMException(global);
  setupConsole(global);
  setupEvent(global);
  setupStructuredClone(global);
  setupTimers(global);
  setupAbortSignal(global);
  setupAsyncHooks(global);
  setupStreams(global);
  setupTextEncoding(global);
  setupBlob(global);
  setupHeaders(global);
  setupFormData(global);
  setupRequest(global);
  setupResponse(global);
  setupFetch(global);
  setupPerformance(global);

  setupJsSyscall(global);
}
