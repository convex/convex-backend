import { performOp } from "udf-syscall-ffi";

/**
 * Patch `Math.random` with the deterministic RNG, proxy `process.env` through
 * the environment-variable op, and alias `self`.
 *
 * Call this before the other setup functions: those may call into 3rd party
 * libraries we bundle, which may then retain references to globals we modify.
 */
export function setupMisc(global) {
  // V8's deprecated RegExp statics expose mutable state from the last match.
  for (const name of [
    "$&",
    "$'",
    "$+",
    "$1",
    "$2",
    "$3",
    "$4",
    "$5",
    "$6",
    "$7",
    "$8",
    "$9",
    "$_",
    "$`",
    "input",
    "lastMatch",
    "lastParen",
    "leftContext",
    "rightContext",
  ]) {
    delete global.RegExp[name];
  }

  delete global.Math.random;
  global.Math.random = function () {
    return performOp("random");
  };

  const handler = {
    get(_target: any, prop: any, receiver: any) {
      if (typeof prop === "string") {
        const value = performOp("environmentVariables/get", prop);
        // Map null to undefined in case other libraries check explicitly for undefined
        // Note serde Value enum in rust does not have an undefined variant, only null.
        if (value === null) {
          if (prop === "inspect") {
            return () => "[process.env]";
          }
          return undefined;
        }
        return value;
      } else {
        return Reflect.get(_target, prop, receiver);
      }
    },
  };
  const env = new Proxy({}, handler);
  global.process = { env };

  // defined in browsers and required by the WinterCG Minimum Common Web Platform API draft
  // https://common-min-api.proposal.wintercg.org/
  global.self = global;

  // Nonstandard and deprecated
  delete global.Intl?.v8BreakIterator;
}
