// Implementation of the User Timing API Level 3 plus performance.now().
// https://w3c.github.io/user-timing/
// https://w3c.github.io/hr-time/
//
// Based on the implementation in Deno (https://github.com/denoland/deno/blob/main/ext/web/15_performance.js)
// Copyright 2018-2026 the Deno authors. MIT license.

import { EventTarget } from "./02_event.js";
import {
  requiredArguments,
  throwNotImplementedMethodError,
} from "./helpers.js";
import { performOp } from "udf-syscall-ffi";

const illegalConstructorKey = Symbol("illegalConstructorKey");

function callPerformanceNow(): number {
  return performOp("performance_now") as number;
}

function callPerformanceTimeOrigin(): number {
  return performOp("performance_time_origin") as number;
}

// Capture the DOM interface types before the class declarations shadow them.
type IPerformance = globalThis.Performance;
type IPerformanceEntry = globalThis.PerformanceEntry;
type IPerformanceMark = globalThis.PerformanceMark;
type IPerformanceMeasure = globalThis.PerformanceMeasure;

class Performance extends EventTarget implements IPerformance {
  #performanceEntries: IPerformanceEntry[] = [];
  #timeOrigin: number | undefined;

  constructor(key: typeof illegalConstructorKey | undefined = undefined) {
    if (key !== illegalConstructorKey) {
      throw new TypeError("Illegal constructor");
    }

    super();
  }

  now(): number {
    return callPerformanceNow();
  }

  get timeOrigin(): number {
    if (this.#timeOrigin === undefined) {
      this.#timeOrigin = callPerformanceTimeOrigin();
    }
    return this.#timeOrigin;
  }

  toJSON() {
    return { timeOrigin: this.timeOrigin };
  }

  clearMarks(markName?: string) {
    if (markName !== undefined) {
      this.#performanceEntries = this.#performanceEntries.filter(
        (entry) => !(entry.name === markName && entry.entryType === "mark"),
      );
    } else {
      this.#performanceEntries = this.#performanceEntries.filter(
        (entry) => entry.entryType !== "mark",
      );
    }
  }

  clearMeasures(measureName?: string) {
    if (measureName !== undefined) {
      this.#performanceEntries = this.#performanceEntries.filter(
        (entry) =>
          !(entry.name === measureName && entry.entryType === "measure"),
      );
    } else {
      this.#performanceEntries = this.#performanceEntries.filter(
        (entry) => entry.entryType !== "measure",
      );
    }
  }

  clearResourceTimings() {
    this.#performanceEntries = this.#performanceEntries.filter(
      (entry) => entry.entryType !== "resource",
    );
  }

  setResourceTimingBufferSize(_maxSize: number) {
    requiredArguments(
      arguments.length,
      1,
      "Failed to execute 'setResourceTimingBufferSize' on 'Performance'",
    );

    // This is a noop in the Convex runtime as we don't have resource timing entries
  }

  getEntries() {
    return this.#filterByNameType();
  }

  getEntriesByName(name: string, type?: string) {
    const prefix = "Failed to execute 'getEntriesByName' on 'Performance'";
    requiredArguments(arguments.length, 1, prefix);

    return this.#filterByNameType(name, type);
  }

  getEntriesByType(type: string) {
    const prefix = "Failed to execute 'getEntriesByType' on 'Performance'";
    requiredArguments(arguments.length, 1, prefix);

    return this.#filterByNameType(undefined, type);
  }

  mark(markName: string, markOptions?: PerformanceMarkOptions) {
    const prefix = "Failed to execute 'mark' on 'Performance'";
    requiredArguments(arguments.length, 1, prefix);

    // 3.1.1.1 If the global object is a Window object and markName uses the
    // same name as a read only attribute in the PerformanceTiming interface,
    // throw a SyntaxError. - not implemented
    const entry = new PerformanceMark(markName, markOptions);
    this.#performanceEntries.push(entry);
    return entry;
  }

  measure(
    measureName: string,
    startOrMeasureOptions?:
      | string
      | {
          start?: string | number;
          end?: string | number;
          duration?: number;
          detail?: any;
        },
    endMark?: string,
  ) {
    const prefix = "Failed to execute 'measure' on 'Performance'";
    requiredArguments(arguments.length, 1, prefix);

    if (
      startOrMeasureOptions &&
      typeof startOrMeasureOptions === "object" &&
      Object.keys(startOrMeasureOptions).length > 0
    ) {
      if (endMark) {
        throw new TypeError('Options cannot be passed with "endMark"');
      }
      if (
        "start" in startOrMeasureOptions &&
        "duration" in startOrMeasureOptions &&
        "end" in startOrMeasureOptions
      ) {
        throw new TypeError(
          'Cannot specify "start", "end", and "duration" together in options',
        );
      }
    }
    let endTime: number;
    if (endMark) {
      endTime = this.#convertMarkToTimestamp(endMark);
    } else if (
      typeof startOrMeasureOptions === "object" &&
      startOrMeasureOptions.end !== undefined
    ) {
      endTime = this.#convertMarkToTimestamp(startOrMeasureOptions.end);
    } else if (
      typeof startOrMeasureOptions === "object" &&
      startOrMeasureOptions.start !== undefined &&
      startOrMeasureOptions.duration !== undefined
    ) {
      const start = this.#convertMarkToTimestamp(startOrMeasureOptions.start);
      const duration = this.#convertMarkToTimestamp(
        startOrMeasureOptions.duration,
      );
      endTime = start + duration;
    } else {
      endTime = callPerformanceNow();
    }

    let startTime: number;
    if (
      typeof startOrMeasureOptions === "object" &&
      startOrMeasureOptions.start !== undefined
    ) {
      startTime = this.#convertMarkToTimestamp(startOrMeasureOptions.start);
    } else if (
      typeof startOrMeasureOptions === "object" &&
      startOrMeasureOptions.end !== undefined &&
      startOrMeasureOptions.duration !== undefined
    ) {
      const end = this.#convertMarkToTimestamp(startOrMeasureOptions.end);
      const duration = this.#convertMarkToTimestamp(
        startOrMeasureOptions.duration,
      );
      startTime = end - duration;
    } else if (typeof startOrMeasureOptions === "string") {
      startTime = this.#convertMarkToTimestamp(startOrMeasureOptions);
    } else {
      startTime = 0;
    }
    const entry = new PerformanceMeasure(
      measureName,
      startTime,
      endTime - startTime,
      typeof startOrMeasureOptions === "object"
        ? (startOrMeasureOptions.detail ?? null)
        : null,
      illegalConstructorKey,
    );
    this.#performanceEntries.push(entry);
    return entry;
  }

  get eventCounts(): EventCounts {
    return throwNotImplementedMethodError("get eventCounts", "Performance");
  }

  get timing(): PerformanceTiming {
    return throwNotImplementedMethodError("get timing", "Performance");
  }

  get navigation(): PerformanceNavigation {
    return throwNotImplementedMethodError("get navigation", "Performance");
  }

  onresourcetimingbufferfull: ((this: IPerformance, ev: Event) => any) | null =
    null;

  #convertMarkToTimestamp(mark: string | number) {
    if (typeof mark === "string") {
      const entry = this.#findMostRecent(mark, "mark");
      if (!entry) {
        throw new DOMException(`Cannot find mark: "${mark}"`, "SyntaxError");
      }
      return entry.startTime;
    }
    if (mark < 0) {
      throw new TypeError(`Mark cannot be negative: received ${mark}`);
    }
    return mark;
  }

  #findMostRecent(name?: string, type?: string) {
    for (let i = this.#performanceEntries.length - 1; i >= 0; --i) {
      const entry = this.#performanceEntries[i];
      if (entry.name === name && entry.entryType === type) {
        return entry;
      }
    }
  }

  #filterByNameType(name?: string, type?: string) {
    return this.#performanceEntries.filter(
      (entry) =>
        (name !== undefined ? entry.name === name : true) &&
        (type !== undefined ? entry.entryType === type : true),
    );
  }
}

Object.defineProperty(Performance.prototype, Symbol.toStringTag, {
  value: "Performance",
  enumerable: false,
  writable: false,
  configurable: true,
});

class PerformanceEntry implements IPerformanceEntry {
  #name: string;
  #entryType: string;
  #startTime: number;
  #duration: number;

  get name(): string {
    return this.#name;
  }
  get entryType(): string {
    return this.#entryType;
  }
  get startTime(): number {
    return this.#startTime;
  }
  get duration(): number {
    return this.#duration;
  }

  constructor(
    // Since this constructor is hidden, I’m setting default values
    // so that `PerformanceEntry.length` (# of expected arguments) === 0
    name: string | undefined = undefined,
    entryType: string | undefined = undefined,
    startTime: number | undefined = undefined,
    duration: number | undefined = undefined,
    key: typeof illegalConstructorKey | undefined = undefined,
  ) {
    if (key !== illegalConstructorKey) {
      throw new TypeError("Illegal constructor");
    }

    if (
      name === undefined ||
      entryType === undefined ||
      startTime === undefined ||
      duration === undefined
    ) {
      throw new Error("Internal error: missing mandatory parameters");
    }

    this.#name = name;
    this.#entryType = entryType;
    this.#startTime = startTime;
    this.#duration = duration;
  }

  toJSON() {
    return {
      name: this.#name,
      entryType: this.#entryType,
      startTime: this.#startTime,
      duration: this.#duration,
    };
  }
}

Object.defineProperty(PerformanceEntry.prototype, Symbol.toStringTag, {
  value: "PerformanceEntry",
  enumerable: false,
  writable: false,
  configurable: true,
});

class PerformanceMark extends PerformanceEntry implements IPerformanceMark {
  #detail: any;

  get detail() {
    return this.#detail;
  }

  get entryType() {
    return "mark";
  }

  constructor(name: string, options?: { startTime?: number; detail?: any }) {
    const prefix = "Failed to construct 'PerformanceMark'";
    requiredArguments(arguments.length, 1, prefix);
    // WebIDL dictionary conversion: only undefined, null, and objects
    // (including functions) are accepted.
    if (
      options !== undefined &&
      options !== null &&
      typeof options !== "object" &&
      typeof options !== "function"
    ) {
      throw new TypeError(
        `${prefix}: The provided value is not of type 'PerformanceMarkOptions'.`,
      );
    }

    const { detail = null, startTime = callPerformanceNow() } = options ?? {};

    super(name, "mark", startTime, 0, illegalConstructorKey);

    if (startTime < 0) {
      throw new TypeError(
        `Cannot construct PerformanceMark: startTime cannot be negative, received ${startTime}`,
      );
    }
    this.#detail = structuredClone(detail);
  }

  toJSON() {
    return {
      name: this.name,
      entryType: this.entryType,
      startTime: this.startTime,
      duration: this.duration,
      detail: this.detail,
    };
  }
}

Object.defineProperty(PerformanceMark.prototype, Symbol.toStringTag, {
  value: "PerformanceMark",
  enumerable: false,
  writable: false,
  configurable: true,
});

class PerformanceMeasure
  extends PerformanceEntry
  implements IPerformanceMeasure
{
  #detail: any;

  get detail() {
    return this.#detail;
  }

  get entryType() {
    return "measure";
  }

  constructor(
    // Since this constructor is hidden, I’m setting default values
    // so that `PerformanceMeasure.length` (# of expected arguments) === 0
    name: string | undefined = undefined,
    startTime: number | undefined = undefined,
    duration: number | undefined = undefined,
    detail: any = undefined,
    key: typeof illegalConstructorKey | undefined = undefined,
  ) {
    if (key !== illegalConstructorKey) {
      throw new TypeError("Illegal constructor");
    }

    if (
      name === undefined ||
      startTime === undefined ||
      duration === undefined
    ) {
      throw new Error("Internal error: missing mandatory parameters");
    }

    super(name, "measure", startTime, duration, key);
    this.#detail = structuredClone(detail);
  }

  toJSON() {
    return {
      name: this.name,
      entryType: this.entryType,
      startTime: this.startTime,
      duration: this.duration,
      detail: this.detail,
    };
  }
}

Object.defineProperty(PerformanceMeasure.prototype, Symbol.toStringTag, {
  value: "PerformanceMeasure",
  enumerable: false,
  writable: false,
  configurable: true,
});

export const setupPerformance = (global: any) => {
  Object.defineProperty(global, "Performance", {
    value: Performance,
    enumerable: false,
    configurable: true,
  });
  Object.defineProperty(global, "PerformanceEntry", {
    value: PerformanceEntry,
    enumerable: false,
    configurable: true,
  });
  Object.defineProperty(global, "PerformanceMark", {
    value: PerformanceMark,
    enumerable: false,
    configurable: true,
  });
  Object.defineProperty(global, "PerformanceMeasure", {
    value: PerformanceMeasure,
    enumerable: false,
    configurable: true,
  });
  const performanceImpl = new Performance(illegalConstructorKey);
  Object.defineProperty(global, "performance", {
    value: performanceImpl,
    enumerable: true,
    configurable: true,
    writable: true,
  });
};
