import { performOp } from "udf-syscall-ffi";

export function setupDate(global: any) {
  // Patch `Date` with our own version that returns a consistent result.
  // We only patch the paths that refer to the current time because for all
  // other paths, we have already ensured determinism by pinning the system
  // time to UTC via the TZ environment variable.
  const originalDate = global.Date;
  delete global.Date;

  function Date(this: any, ...args) {
    // `Date()` was called directly, not as a constructor.
    if (!(this instanceof Date)) {
      const date = new (Date as any)();
      return date.toString();
    }
    if (args.length === 0) {
      const unixTsMs = Date.now();
      return new originalDate(unixTsMs);
    }
    return new originalDate(...args);
  }
  Date.now = function () {
    return performOp("now");
  };
  Date.parse = originalDate.parse;
  Date.UTC = originalDate.UTC;
  Date.prototype = originalDate.prototype;
  Date.prototype.constructor = Date;

  global.Date = Date;
}
