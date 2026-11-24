import { performOp } from "udf-syscall-ffi";

export function setupDate(global: any) {
  // Patch `Date` with our own version that returns a consistent result.
  // We only patch the paths that refer to the current time because for all
  // other paths, we have already ensured determinism by pinning the system
  // time to UTC via the TZ environment variable.
  const originalDate = global.Date;
  delete global.Date;

  function Date(...args) {
    // `Date()` was called directly, not as a constructor.
    if (new.target === undefined) {
      return new originalDate(Date.now()).toString();
    }
    // Construct with `new.target` so subclasses (`class D extends Date`) get
    // their own prototype.
    return Reflect.construct(
      originalDate,
      args.length === 0 ? [Date.now()] : args,
      new.target,
    );
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
