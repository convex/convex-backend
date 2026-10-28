import { performOp } from "udf-syscall-ffi";

export function setupTemporal(global: any) {
  const Temporal = global.Temporal;

  const fromEpochMilliseconds = Temporal.Instant.fromEpochMilliseconds;
  // Native Now methods read V8's platform clock independently of Date.now.
  // Override them to return the frozen UDF time and record that time was observed.
  const instant = () => fromEpochMilliseconds(performOp("now"));
  const zonedDateTimeISO = (timeZone: any = "UTC") =>
    instant().toZonedDateTimeISO(timeZone);
  const plainDateTimeISO = (timeZone: any = "UTC") =>
    zonedDateTimeISO(timeZone).toPlainDateTime();
  const plainDateISO = (timeZone: any = "UTC") =>
    zonedDateTimeISO(timeZone).toPlainDate();
  const plainTimeISO = (timeZone: any = "UTC") =>
    zonedDateTimeISO(timeZone).toPlainTime();

  Temporal.Now = Object.defineProperties(
    {},
    {
      instant: {
        value: instant,
        writable: true,
        configurable: true,
      },
      timeZoneId: {
        value: Temporal.Now.timeZoneId,
        writable: true,
        configurable: true,
      },
      plainDateTimeISO: {
        value: plainDateTimeISO,
        writable: true,
        configurable: true,
      },
      zonedDateTimeISO: {
        value: zonedDateTimeISO,
        writable: true,
        configurable: true,
      },
      plainDateISO: {
        value: plainDateISO,
        writable: true,
        configurable: true,
      },
      plainTimeISO: {
        value: plainTimeISO,
        writable: true,
        configurable: true,
      },
      [Symbol.toStringTag]: {
        value: "Temporal.Now",
        configurable: true,
      },
    },
  );
}

// `Intl.DateTimeFormat`'s `format` and `formatToParts` format the current time
// when called without a date. V8 reads that from its platform clock rather than
// `Date.now`, which would bypass the frozen UDF time.
export function patchDateTimeFormat(global: any, now: () => number) {
  const proto = global.Intl.DateTimeFormat.prototype;
  const nativeFormatGetter = Object.getOwnPropertyDescriptor(
    proto,
    "format",
  )!.get!;
  const boundFormat = Symbol("boundFormat");
  const descriptor = {
    get(this: any) {
      const nativeFormat = nativeFormatGetter.call(this);
      // Cache the returned function, as required by the Intl specification (to
      // satisfy `dtf.format === dtf.format`)
      return (nativeFormat[boundFormat] ??= (date?: any) =>
        nativeFormat(date === undefined ? now() : date));
    },
    enumerable: false,
    configurable: true,
  };
  Object.defineProperty(descriptor.get, "name", { value: "get format" });
  Object.defineProperty(proto, "format", descriptor);

  const nativeFormatToParts = proto.formatToParts;
  const { formatToParts } = {
    formatToParts(this: any, date?: any) {
      return nativeFormatToParts.call(this, date === undefined ? now() : date);
    },
  };
  Object.defineProperty(proto, "formatToParts", {
    value: formatToParts,
    writable: true,
    enumerable: false,
    configurable: true,
  });
}
