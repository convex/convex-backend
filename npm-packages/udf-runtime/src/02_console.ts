import inspect from "object-inspect";
import { performOp } from "udf-syscall-ffi";

function getMessage(args) {
  // TODO: Support string substitution.
  const serializedArgs = args.map((e) =>
    inspect(e, {
      // Our entire log line can't be more than 32768 bytes (MAX_LOG_LINE_LENGTH) so
      // keep string in here to no more than 32768 UTF-16 code units, and let
      // the backend truncate the whole log line if it is too long.
      maxStringLength: 32768,
      indent: 2,
      customInspect: true,
    }),
  );
  return serializedArgs;
}

function toString(value: unknown, defaultValue: string) {
  // `String()` yields a string for the op even when a label's `toString`
  // returns a non-string.
  return value === undefined ? defaultValue : String(value);
}

function consoleMessage(level, args) {
  performOp("console/message", level, getMessage(args));
}
// For historical web-compatibility reasons, the namespace object for console
// must have as its [[Prototype]] an empty object, created as if by
// ObjectCreate(%ObjectPrototype%), instead of %ObjectPrototype%.
const consoleImpl = Object.assign(Object.create({}), {
  debug: function (...args) {
    consoleMessage("DEBUG", args);
  },
  error: function (...args) {
    consoleMessage("ERROR", args);
  },
  info: function (...args) {
    consoleMessage("INFO", args);
  },
  log: function (...args) {
    consoleMessage("LOG", args);
  },
  warn: function (...args) {
    consoleMessage("WARN", args);
  },
  trace: function (...args) {
    const message = getMessage(args);
    const error = new Error();
    // This calls `prepareStackTrace` that populates `__frameData`
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    error.stack;
    // A runtime without `prepareStackTrace` (see `errors.ts`) sends no frames,
    // and its `console/trace` op supplies the stack itself.
    const frameJson = (error as any).__frameData;
    const frameData = frameJson === undefined ? [] : JSON.parse(frameJson);
    performOp("console/trace", message, frameData);
  },
  time: function (label: unknown) {
    const labelStr = toString(label, "default");
    performOp("console/timeStart", labelStr);
  },
  timeLog: function (label: unknown, ...args) {
    const labelStr = toString(label, "default");
    performOp("console/timeLog", labelStr, getMessage(args));
  },
  timeEnd: function (label: unknown) {
    const labelStr = toString(label, "default");
    performOp("console/timeEnd", labelStr);
  },
  // TODO: Implement the rest of the Console API.
});
Object.defineProperty(consoleImpl, Symbol.toStringTag, {
  value: "console",
  enumerable: false,
  writable: false,
  configurable: true,
});
export function setupConsole(global) {
  // Delete v8's console since it doesn't go anywhere. We'll eventually want to mirror our console
  // output to v8's console since apparently its output shows up in v8's debugger.
  delete global.console;
  Object.defineProperty(global, "console", {
    value: consoleImpl,
    enumerable: false,
    writable: true,
    configurable: true,
  });
}
