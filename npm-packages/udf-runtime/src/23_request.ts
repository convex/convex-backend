import { throwNotImplementedMethodError } from "./helpers.js";
import { Blob } from "./09_file.js";
import { BodyInit, convertBodyInit, extractBody } from "./22_body.js";
import inspect from "object-inspect";
import { parseFormData, FormData } from "./21_formdata.js";
import {
  constructStreamId,
  extractStream,
  ReadableStream,
} from "./06_streams.js";

function isValidMethod(m: string) {
  return (
    m === "DELETE" ||
    m === "GET" ||
    m === "HEAD" ||
    m === "OPTIONS" ||
    m === "PATCH" ||
    m === "POST" ||
    m === "PUT"
  );
}

export interface RequestInit {
  /** A string to set request's method. */
  method?: string;
  /** A Headers object, an object literal, or an array of two-item arrays to set request's headers. */
  headers?: HeadersInit;
  /** A BodyInit object or null to set request's body. */
  body?: BodyInit | null;
  /** A string to indicate whether the request will use CORS, or will be restricted to same-origin URLs. Sets request's mode. */
  //mode?: RequestMode;
  /** A string indicating whether credentials will be sent with the request always, never, or only when sent to a same-origin URL. Sets request's credentials. */
  //credentials?: RequestCredentials;
  /** A string indicating how the request will interact with the browser's cache to set request's cache. */
  //cache?: RequestCache;
  /** A string indicating whether request follows redirects, results in an error upon encountering a redirect, or returns the redirect (in an opaque fashion). Sets request's redirect. */
  redirect?: RequestRedirect;
  /** A string whose value is a same-origin URL, "about:client", or the empty string, to set request's referrer. */
  //referrer?: string;
  /** A referrer policy to set request's referrerPolicy. */
  //referrerPolicy?: ReferrerPolicy;
  /** A cryptographic hash of the resource to be fetched by request. Sets request's integrity. */
  //integrity?: string;
  /** A boolean to set request's keepalive. */
  //keepalive?: boolean;
  /** An AbortSignal to set request's signal. */
  signal?: AbortSignal | null;
  //priority?: "high" | "low" | "auto";
}

const _contentLength = Symbol("[[contentLength]]");

const validateURL = (s: string | URL) => {
  const url = new URL(s);
  const protocol = url.protocol;
  if (protocol !== "http:" && protocol !== "https:") {
    throw new TypeError(
      `Unsupported URL scheme -- http and https are supported (scheme was ${protocol.slice(0, protocol.length - 1)})`,
    );
  }
  return url.href;
};

export class Request {
  private readonly _headers: Headers;
  private readonly _url: string;
  private readonly _method: string;
  private _bodyStream: ReadableStream | null = null;
  private _bodyUsed = false;
  private _signal: AbortSignal;
  [_contentLength]: number | null;

  constructor(input: string | URL | Request, options?: RequestInit) {
    if (input === undefined) {
      throw new TypeError("Request URL is undefined");
    }
    // WebIDL converts the arguments before any constructor steps run: `input`
    // (a USVString unless it's a Request), then `init.body`.
    const inputUrl = input instanceof Request ? null : `${input}`;
    const rawBody = options?.body;
    const initBody =
      rawBody === undefined || rawBody === null
        ? null
        : convertBodyInit(rawBody);

    this[_contentLength] = null;
    // By default, the signal never aborts.
    this._signal = new AbortSignal();
    if (input instanceof Request) {
      // Copy initial values from the request. Options can still override them.
      this._url = validateURL(input.url);
      this._method = input.method;
      this._headers = new Headers(input.headers);
      this._signal = input.signal;
      // TODO(presley): https://developer.mozilla.org/en-US/docs/Web/API/Request/Request
      // * If this object exists on another origin to the constructor call, the Request.referrer is stripped out.
      // * If this object has a Request.mode of navigate, the mode value is converted to same-origin.
    } else {
      this._url = validateURL(inputUrl!);
      // Use default values.
      this._method = "GET";
      this._headers = new Headers([]);
    }

    if (typeof options?.method === "string") {
      if (isValidMethod(options.method.toUpperCase())) {
        this._method = options.method.toUpperCase();
      } else {
        this._method = options.method;
      }
    }
    if (options?.headers !== undefined) {
      this._headers = new Headers(options?.headers);
    }
    if (options?.signal !== undefined && options.signal !== null) {
      this._signal = options.signal;
    }

    if (initBody !== null) {
      if (this._method === "GET" || this._method === "HEAD") {
        throw new TypeError(
          "Failed to construct 'Request': Request with GET/HEAD method cannot have body.",
        );
      }

      const { stream, contentType, contentLength } = extractBody(initBody);
      if (this._headers.get("content-type") === null && contentType !== null) {
        this._headers.set("content-type", contentType);
      }
      if (contentLength !== null) {
        this[_contentLength] = contentLength;
      } else if (this._headers.get("content-length") !== null) {
        this[_contentLength] = Number(this._headers.get("content-length"));
      }
      this._bodyStream = stream;
    } else if (input instanceof Request) {
      // We don't consume the body in the input, if there are overrides.
      this._bodyStream = input._bodyStream;
      input._markBodyUsed("new Request()");
    } else {
      this._bodyStream = null;
    }
  }

  private _markBodyUsed(method: string) {
    if (this._bodyUsed) {
      throw new TypeError(
        `Failed to execute '${method}' on 'Request': body stream already read`,
      );
    }
    // Apparently, using the body multiple times if there is no body is fine.
    if (this._bodyStream !== null) {
      this._bodyUsed = true;
    }
  }

  private async _blob() {
    if (this._bodyStream === null) {
      return Promise.resolve(new Blob());
    }
    const type = this.headers.get("content-type") ?? "";
    if (this[_contentLength] !== null) {
      return Blob.fromStream(this._bodyStream, this[_contentLength], type);
    }
    const reader = this._bodyStream.getReader();
    const chunks: Uint8Array[] = [];
    const read = async (): Promise<void> => {
      const { done, value } = await reader.read();
      if (!done && value) {
        chunks.push(value);
        return read();
      }
    };
    await read();
    return new Blob(chunks, { type });
  }

  get headers() {
    return this._headers;
  }

  get url() {
    return this._url;
  }

  get method() {
    return this._method;
  }

  get bodyUsed() {
    return this._bodyUsed;
  }

  async blob(): Promise<Blob> {
    this._markBodyUsed("blob");
    const blob = await this._blob();
    return blob;
  }

  async text(): Promise<string> {
    this._markBodyUsed("text");
    const blob = await this._blob();
    return blob.text();
  }

  async json(): Promise<any> {
    this._markBodyUsed("json");
    const blob = await this._blob();
    const text = await blob.text();
    return JSON.parse(text);
  }

  async arrayBuffer(): Promise<ArrayBuffer> {
    this._markBodyUsed("arrayBuffer");
    const blob = await this._blob();
    return blob.arrayBuffer();
  }

  async formData(): Promise<FormData> {
    this._markBodyUsed("formData");
    const blob = await this._blob();
    return parseFormData(blob, this._headers.get("content-type"));
  }

  get body() {
    return this._bodyStream;
  }

  clone() {
    const clonedHeaderPairs: [string, string][] = [];
    this._headers.forEach((headerValue, headerName) =>
      clonedHeaderPairs.push([headerName, headerValue]),
    );

    const [bodyStreamA, bodyStreamB] =
      this._bodyStream !== null ? this._bodyStream.tee() : [null, null];

    const clonedRequest = new Request(this._url, {
      method: this._method,
      headers: clonedHeaderPairs,
    });
    clonedRequest._bodyStream = bodyStreamB;
    this._bodyStream = bodyStreamA;
    return clonedRequest;
  }

  inspect() {
    const properties = {
      bodyUsed: this._bodyUsed,
      headers: this._headers,
      method: this._method,
      url: this._url,
    };
    return `Request ${inspect(properties)}`;
  }

  toJSON() {
    return {};
  }

  // ---------------------------------------------------------------
  // Begin unimplemented functions
  // ---------------------------------------------------------------

  get destination() {
    return throwNotImplementedMethodError("get destination", "Request");
  }

  get referrer() {
    return throwNotImplementedMethodError("get referrer", "Request");
  }

  get referrerPolicy() {
    return throwNotImplementedMethodError("get referrerPolicy", "Request");
  }

  get mode() {
    return throwNotImplementedMethodError("get mode", "Request");
  }

  get credentials() {
    return throwNotImplementedMethodError("get credentials", "Request");
  }

  get cache() {
    return throwNotImplementedMethodError("get cache", "Request");
  }

  get redirect() {
    return throwNotImplementedMethodError("get redirect", "Request");
  }

  get integrity() {
    return throwNotImplementedMethodError("get integrity", "Request");
  }

  get keepalive() {
    return throwNotImplementedMethodError("get keepalive", "Request");
  }

  get isReloadNavigation() {
    return throwNotImplementedMethodError("get isReloadNavigation", "Request");
  }

  get isHistoryNavigation() {
    return throwNotImplementedMethodError("get isHistoryNavigation", "Request");
  }

  get signal() {
    return this._signal;
  }

  get duplex() {
    return throwNotImplementedMethodError("get duplex", "Request");
  }
}

Object.defineProperty(Request.prototype, Symbol.toStringTag, {
  value: "Request",
  enumerable: false,
  writable: false,
  configurable: true,
});

// Stream in Rust -> AbortSignal in JS
function extractAbortSignal(signalId: string) {
  const abortController = new AbortController();
  const stream = extractStream(signalId);
  void stream
    .getReader()
    .read()
    .then(() => abortController.abort());
  return abortController.signal;
}

export const requestFromConvexJson = ({
  convexJson,
}: {
  convexJson: Record<string, any>;
}) => {
  const stream =
    convexJson.streamId === null ? null : extractStream(convexJson.streamId);
  const signal = extractAbortSignal(convexJson.signal);
  const request = new Request(convexJson.url, {
    headers: convexJson.headerPairs,
    body: stream,
    method: convexJson.method,
    signal,
  });
  return request;
};

// AbortSignal in JS -> stream in Rust
function constructAbortSignalStreamId(signal: AbortSignal): string {
  return constructStreamId(
    new ReadableStream({
      start(controller) {
        signal.addEventListener("abort", () => controller.close());
      },
    }),
  );
}

export const convexV8ObjectFromRequest = async (request: Request) => {
  const streamId = request.body ? constructStreamId(request.body) : null;
  let headerPairs = [...request.headers.entries()];
  // Per https://fetch.spec.whatwg.org/#http-network-or-cache-fetch:
  // If httpRequest's body is null and httpRequest's method is `POST` or `PUT`, then set contentLengthHeaderValue to `0`.
  const bodylessPostOrPut =
    request.body === null &&
    (request.method === "POST" || request.method === "PUT");
  const contentLength = bodylessPostOrPut ? 0 : request[_contentLength];
  if (bodylessPostOrPut) {
    headerPairs = headerPairs.filter(([name]) => name !== "content-length");
  }
  if (
    contentLength !== null &&
    (bodylessPostOrPut || !request.headers.has("content-length"))
  ) {
    headerPairs.push(["content-length", String(contentLength)]);
  }
  const signal = constructAbortSignalStreamId(request.signal);
  return {
    url: request.url,
    headerPairs,
    method: request.method,
    streamId,
    signal,
  };
};

export const setupRequest = (global: any) => {
  global.Request = Request;
};
