import { throwNotImplementedMethodError } from "./helpers.js";
import { performOp } from "udf-syscall-ffi";
import inspect from "object-inspect";

// Each variant holds a setter's value unmodified: Ada's setters, behind the op,
// apply the spec's preprocessing (such as stripping tabs and newlines) and leave
// the URL unchanged when the value is invalid.
type Update =
  | {
      hash: string;
    }
  | {
      hostname: string;
    }
  | {
      href: string;
    }
  | {
      password: string;
    }
  | {
      port: string;
    }
  | {
      protocol: string;
    }
  | {
      pathname: string;
    }
  | {
      search: string;
    }
  | {
      searchParams: [string, string][];
    }
  | {
      username: string;
    };

// WebIDL's USVString conversion: a template literal rather than `String()` so
// that a Symbol throws a `TypeError`, and lone surrogates become U+FFFD, which
// the URL ops' UTF-8 arguments could not carry anyway.
const toUSVString = (value: unknown) => `${value}`.toWellFormed();

// Private symbols for URL to poke at the internals of URLSearchParams
const _searchParamPairs = Symbol("_searchParamPairs");
const _urlObjectUpdate = Symbol("_urlObjectUpdate");

class URLSearchParamsIterator<T> implements IterableIterator<T> {
  #params: URLSearchParams;
  #index = 0;
  #select: (key: string, value: string) => T;

  constructor(
    params: URLSearchParams,
    select: (key: string, value: string) => T,
  ) {
    this.#params = params;
    this.#select = select;
  }

  next(): IteratorResult<T> {
    if (this.#index >= this.#params[_searchParamPairs].length) {
      return { value: undefined, done: true };
    }
    // Mutations and URL updates can replace the entire parameter list.
    const [key, value] = this.#params[_searchParamPairs][this.#index++];
    return { value: this.#select(key, value), done: false };
  }

  [Symbol.iterator](): IterableIterator<T> {
    return this;
  }
}

Object.setPrototypeOf(
  URLSearchParamsIterator.prototype,
  Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())),
);
Object.defineProperty(URLSearchParamsIterator.prototype, "next", {
  value: URLSearchParamsIterator.prototype.next,
  writable: true,
  enumerable: true,
  configurable: true,
});
Object.defineProperty(URLSearchParamsIterator.prototype, Symbol.toStringTag, {
  value: "URLSearchParams Iterator",
  enumerable: false,
  writable: false,
  configurable: true,
});

class URLSearchParams {
  [_searchParamPairs]: [string, string][];
  // Reference back to the parent URL's `#updateUrl` method
  [_urlObjectUpdate]?: (update: { searchParams: [string, string][] }) => void;
  constructor(
    init?:
      | Iterable<Iterable<string>>
      | Record<string, string>
      | string
      | URLSearchParams,
  ) {
    this[_searchParamPairs] = [];
    let iteratorMethod: any;
    if (init === undefined) {
      this[_searchParamPairs] = [];
    } else if (typeof init === "string") {
      const usvInit = init.toWellFormed();
      const queryString = usvInit.startsWith("?") ? usvInit.slice(1) : usvInit;
      this[_searchParamPairs] = performOp(
        "url/getUrlSearchParamPairs",
        queryString,
      );
    } else if (
      init !== null &&
      (typeof init === "object" || typeof init === "function") &&
      typeof (iteratorMethod = (init as any)[Symbol.iterator]) === "function"
    ) {
      // WebIDL sequence conversion reads @@iterator once; a getter may return
      // a different value (or throw) on a second read.
      const iterator: Iterator<Iterable<string>> = Reflect.apply(
        iteratorMethod,
        init,
        [],
      );
      for (const rawPair of { [Symbol.iterator]: () => iterator }) {
        const pair = [...rawPair];
        if (pair.length !== 2) {
          throw new TypeError(
            "Failed to construct 'URLSearchParams': Failed to construct 'URLSearchParams': Sequence initializer must only contain pair elements",
          );
        }
        this.append(pair[0]!, pair[1]!);
      }
    } else {
      // WebIDL record conversion: only enumerable own properties are read,
      // and keys become USVStrings (Symbol keys throw), so keys that differ
      // only in lone surrogates collapse into one entry, keeping the first
      // key's position and the last key's value.
      const record = new Map<string, string>();
      for (const key of Reflect.ownKeys(init)) {
        if (Object.getOwnPropertyDescriptor(init, key)?.enumerable) {
          record.set(toUSVString(key), toUSVString((init as any)[key]));
        }
      }
      for (const [key, value] of record) {
        this.append(key, value);
      }
    }
  }

  private _updateUrl() {
    if (this[_urlObjectUpdate] !== undefined) {
      this[_urlObjectUpdate]({
        searchParams: this[_searchParamPairs],
      });
    }
  }

  append(name: string, value: string): void {
    this[_searchParamPairs].push([toUSVString(name), toUSVString(value)]);
    this._updateUrl();
  }

  delete(name: string, value?: string) {
    const n = toUSVString(name);
    const v = value === undefined ? undefined : toUSVString(value);
    this[_searchParamPairs] = this[_searchParamPairs].filter(
      ([key, val]) => key !== n || (v !== undefined && val !== v),
    );
    this._updateUrl();
  }

  #createIterator<T>(
    select: (key: string, value: string) => T,
  ): IterableIterator<T> {
    return new URLSearchParamsIterator(this, select);
  }

  entries(): IterableIterator<[string, string]> {
    return this.#createIterator((key, value) => [key, value]);
  }

  forEach(
    callbackFn: (value: string, key: string, parent: URLSearchParams) => void,
  ) {
    this[_searchParamPairs].forEach(([key, value]) => {
      callbackFn(value, key, this);
    });
  }

  get(name: string): string | null {
    return this.getAll(name)[0] ?? null;
  }

  getAll(name: string): string[] {
    const n = toUSVString(name);
    const values: string[] = [];
    for (const [key, value] of this[_searchParamPairs]) {
      if (key === n) {
        values.push(value);
      }
    }
    return values;
  }

  has(name: string, value?: string): boolean {
    const n = toUSVString(name);
    const v = value === undefined ? undefined : toUSVString(value);
    return this[_searchParamPairs].some(
      ([key, val]) => key === n && (v === undefined || val === v),
    );
  }

  keys(): IterableIterator<string> {
    return this.#createIterator((key) => key);
  }

  set(name: string, value: string) {
    name = toUSVString(name);
    value = toUSVString(value);
    let found = false;
    this[_searchParamPairs] = this[_searchParamPairs].filter((pair) => {
      if (pair[0] !== name) {
        return true;
      }
      if (found) {
        return false;
      }
      found = true;
      pair[1] = value;
      return true;
    });
    if (!found) {
      this[_searchParamPairs].push([name, value]);
    }
    this._updateUrl();
  }

  get size(): number {
    return this[_searchParamPairs].length;
  }

  sort() {
    this[_searchParamPairs].sort((a, b) => {
      return a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0;
    });
    this._updateUrl();
  }

  toString() {
    return performOp("url/stringifyUrlSearchParams", this[_searchParamPairs]);
  }

  toJSON() {
    return {};
  }

  [Symbol.iterator](): IterableIterator<[string, string]> {
    return this.#createIterator((key, value) => [key, value]);
  }

  values(): IterableIterator<string> {
    return this.#createIterator((_key, value) => value);
  }

  inspect() {
    let inner = "";
    if (this[_searchParamPairs].length !== 0) {
      inner =
        " " +
        this[_searchParamPairs]
          .map(([k, v]) => `${inspect(k)} => ${inspect(v)}`)
          .join(", ") +
        " ";
    }
    return `${this.constructor.name} {${inner}}`;
  }
}

Object.defineProperty(URLSearchParams.prototype, Symbol.toStringTag, {
  value: "URLSearchParams",
  enumerable: false,
  writable: false,
  configurable: true,
});

type UrlInfo = {
  protocol: string;
  hash: string;
  host: string;
  hostname: string;
  pathname: string;
  port: string;
  search: string;
  href: string;
  username: string;
  password: string;
  origin: string;
};

class URL {
  #urlInfo: UrlInfo;
  #searchParams: URLSearchParams;

  static canParse(url: string | URL, base?: string | URL): boolean {
    try {
      new URL(url, base);
      return true;
    } catch {
      return false;
    }
  }

  static parse(url: string | URL, base?: string | URL): URL | null {
    try {
      return new URL(url, base);
    } catch {
      return null;
    }
  }

  constructor(url: string | URL, base?: string | URL) {
    // Both arguments are USVStrings per WebIDL, so anything that isn't a URL
    // object is stringified before parsing.
    let baseHref: string | null = null;
    if (base !== undefined) {
      baseHref = base instanceof URL ? base.href : toUSVString(base);
    }
    const href = url instanceof URL ? url.href : toUSVString(url);
    this.#urlInfo = performOp("url/getUrlInfo", href, baseHref);
    // `search` keeps its `?`, and the string constructor strips exactly one,
    // so a query that itself begins with `?` keeps it.
    this.#searchParams = new URLSearchParams(this.#urlInfo.search);
    this.#searchParams[_urlObjectUpdate] = this.#updateUrl.bind(this);
  }

  get hash() {
    return this.#urlInfo.hash;
  }

  set hash(_hash: string) {
    this.#updateUrl({
      hash: toUSVString(_hash),
    });
  }

  get host() {
    return this.#urlInfo.host;
  }

  set host(_host: string) {
    throwNotImplementedMethodError("set host", "URL");
  }

  get hostname() {
    return this.#urlInfo.hostname;
  }

  set hostname(_hostname: string) {
    this.#updateUrl({
      hostname: toUSVString(_hostname),
    });
  }

  get href() {
    return this.#urlInfo.href;
  }

  set href(_href: string) {
    this.#updateUrl({
      href: toUSVString(_href),
    });
  }

  get origin() {
    return this.#urlInfo.origin;
  }

  get password() {
    return this.#urlInfo.password;
  }

  set password(password: string) {
    this.#updateUrl({
      password: toUSVString(password),
    });
  }

  get pathname() {
    return this.#urlInfo.pathname;
  }

  set pathname(_pathname: string) {
    this.#updateUrl({
      pathname: toUSVString(_pathname),
    });
  }

  get port() {
    return this.#urlInfo.port?.toString() ?? "";
  }

  set port(_port: string) {
    this.#updateUrl({
      port: toUSVString(_port),
    });
  }

  get protocol() {
    return this.#urlInfo.protocol;
  }

  set protocol(_protocol: string) {
    this.#updateUrl({
      protocol: toUSVString(_protocol),
    });
  }

  get search() {
    return this.#urlInfo.search;
  }

  set search(_search: string) {
    this.#updateUrl({
      search: toUSVString(_search),
    });
  }

  get searchParams() {
    return this.#searchParams;
  }

  get username() {
    return this.#urlInfo.username;
  }

  set username(username: string) {
    this.#updateUrl({
      username: toUSVString(username),
    });
  }

  toString() {
    return this.href;
  }

  toJSON() {
    return this.href;
  }

  #updateUrl(update: Update) {
    this.#urlInfo = performOp("url/updateUrlInfo", this.href, update);
    // Mutate the existing searchParams object
    const searchPairs = performOp(
      "url/getUrlSearchParamPairs",
      this.#urlInfo.search.slice(1),
    );
    this.#searchParams[_searchParamPairs] = searchPairs;
  }

  inspect() {
    const object = {
      href: this.href,
      origin: this.origin,
      protocol: this.protocol,
      username: this.username,
      password: this.password,
      host: this.host,
      hostname: this.hostname,
      port: this.port,
      pathname: this.pathname,
      hash: this.hash,
      search: this.search,
    };
    return `${this.constructor.name} ${inspect(object)}`;
  }
}

Object.defineProperty(URL.prototype, Symbol.toStringTag, {
  value: "URL",
  enumerable: false,
  writable: false,
  configurable: true,
});

export const setupURL = (global: any) => {
  global.URL = URL;
  global.URLSearchParams = URLSearchParams;
};
