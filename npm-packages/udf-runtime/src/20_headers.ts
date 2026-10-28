import { performOp } from "udf-syscall-ffi";
import inspect from "object-inspect";

type HeadersInit = [string, string][] | Record<string, string> | Headers;

class Headers {
  private _headersList: [string, string][];

  [Symbol.iterator]() {
    return this._headersList[Symbol.iterator]();
  }

  private _normalizeName(name: string, errorPrefix: string): string {
    const normalizedName = performOp("headers/normalizeName", name);
    if (normalizedName === null) {
      throw new TypeError(`${errorPrefix}: Invalid name`);
    }
    return normalizedName;
  }

  constructor(init?: HeadersInit) {
    this._headersList = [];
    if (init === undefined) {
      // do nothing
    } else if (init instanceof Array) {
      init.forEach((pair) => {
        if (pair.length !== 2) {
          throw TypeError("Failed to construct 'Headers': Invalid value");
        }
        const [name, value] = pair;
        this.append(name, String(value));
      });
    } else if (init instanceof Headers) {
      for (const [key, value] of init.entries()) {
        this.append(key, String(value));
      }
    } else {
      for (const key in init) {
        this.append(key, String(init[key]));
      }
    }
  }

  append(name: string, value: string) {
    const normalizedName = this._normalizeName(
      name,
      "Failed to execute 'append' on 'Headers",
    );
    this._headersList.push([normalizedName, String(value)]);
  }

  delete(name: string) {
    const normalizedName = this._normalizeName(
      name,
      "Failed to execute 'delete' on 'Headers",
    );
    this._headersList = this._headersList.filter(([key]) => {
      return key !== normalizedName;
    });
  }

  entries() {
    return this._headersList[Symbol.iterator]();
  }

  forEach(callbackFn: (value: string, key: string, parent: Headers) => void) {
    this._headersList.forEach(([key, value]) => {
      callbackFn(value, key, this);
    });
  }

  get(name: string) {
    const normalizedName = this._normalizeName(
      name,
      "Failed to execute 'get' on 'Headers",
    );
    const values: string[] = [];
    for (const [key, value] of this._headersList) {
      if (key === normalizedName) {
        values.push(value);
      }
    }
    if (values.length === 0) {
      return null;
    }
    return values.join(", ");
  }

  has(name: string): boolean {
    const normalizedName = this._normalizeName(
      name,
      "Failed to execute 'has' on 'Headers",
    );
    return (
      this._headersList.find(([key]) => key === normalizedName) !== undefined
    );
  }

  keys(): IterableIterator<string> {
    return this._headersList.map(([key]) => key)[Symbol.iterator]();
  }

  set(name: string, value: string) {
    const normalizedName = this._normalizeName(
      name,
      "Failed to execute 'set' on 'Headers",
    );
    this.delete(normalizedName);
    this.append(normalizedName, value);
  }

  values(): IterableIterator<string> {
    return this._headersList.map(([, value]) => value)[Symbol.iterator]();
  }

  inspect() {
    const headers = {};
    for (const header of this._headersList) {
      headers[header[0]] = header[1];
    }
    return `Headers ${inspect(headers)}`;
  }

  getSetCookie() {
    const normalizedName = this._normalizeName(
      "set-cookie",
      "Failed to execute 'getSetCookie' on 'Headers",
    );
    const values: string[] = [];
    for (const [key, value] of this._headersList) {
      if (key === normalizedName) {
        values.push(value);
      }
    }
    return values;
  }
}

Object.defineProperty(Headers.prototype, Symbol.toStringTag, {
  value: "Headers",
  enumerable: false,
  writable: false,
  configurable: true,
});

export const setupHeaders = (global: any) => {
  global.Headers = Headers;
};
