import { Blob } from "./09_file.js";
import { parseFormData, FormData } from "./21_formdata.js";
import { BodyInit, convertBodyInit, extractBody } from "./22_body.js";
import inspect from "object-inspect";
import {
  constructStreamId,
  extractStream,
  ReadableStream,
} from "./06_streams.js";

const _contentLength = Symbol("[[contentLength]]");
export const _redirected = Symbol("[[redirected]]");
const _responseType = Symbol("[[responseType]]");
const NULL_BODY_STATUSES = [204, 205, 304];

export class Response {
  private _status: number;
  private _statusText: string;
  private _headers: Headers;
  private _bodyStream: ReadableStream | null = null;
  private _bodyUsed = false;
  private _url: string;
  [_contentLength]: number | null;
  [_redirected]: boolean;
  [_responseType]: ResponseType;

  static error() {
    return new Response(null, { status: 500 });
  }

  static redirect(url: string | URL, status?: number) {
    const location = typeof url === "string" ? url : url.href;

    return new Response(null, {
      status: status ?? 302,
      headers: new Headers({ location }),
    });
  }

  static json(
    data: any,
    init?: {
      status?: number;
      statusText?: string;
      headers?: [string, string][] | Headers | Record<string, string>;
      url?: string;
    },
  ) {
    let body = "";
    if (data === undefined) {
      throw new TypeError(
        "Failed to execute 'json' on 'Response': The data is not JSON serializable",
      );
    }
    try {
      body = JSON.stringify(data);
    } catch {
      throw new TypeError(
        "Failed to execute 'json' on 'Response': The data is not JSON serializable",
      );
    }
    const res = new Response(body, init);
    res.headers.set("content-type", "application/json");
    return res;
  }

  constructor(
    body?: BodyInit | null,
    options?: {
      status?: number;
      statusText?: string;
      headers?: [string, string][] | Headers | Record<string, string>;
      url?: string;
    },
  ) {
    // WebIDL converts `body` before `init` or any constructor steps.
    const convertedBody =
      body === undefined || body === null ? null : convertBodyInit(body);

    let status = options?.status === undefined ? 200 : options.status;
    if (typeof status === "string") {
      // This coerces the string to a number (and is different from `parseInt` which allows trailing characters after a valid number)
      status = +status;
    }
    if (
      typeof status !== "number" ||
      Number.isNaN(status) ||
      !Number.isInteger(status) ||
      status < 200 ||
      status > 599
    ) {
      throw new RangeError(
        "Failed to construct 'Response': The status provided is outside the range [200, 599].",
      );
    }
    this._status = status;
    this._statusText = options?.statusText ?? "";
    this._headers = new Headers(options?.headers ?? []);
    this._url = options?.url ?? "";
    this[_contentLength] = null;
    this[_redirected] = false;
    this[_responseType] = "default";

    if (this._headers.get("content-length") !== null) {
      this[_contentLength] = Number(this._headers.get("content-length"));
    }

    if (convertedBody !== null) {
      if (NULL_BODY_STATUSES.includes(this._status)) {
        throw new TypeError("Response with null body status cannot have body");
      }
      const { stream, contentType, contentLength } = extractBody(convertedBody);
      if (this._headers.get("content-type") === null && contentType !== null) {
        this._headers.set("content-type", contentType);
      }
      if (contentLength !== null) {
        this[_contentLength] = contentLength;
      }
      this._bodyStream = stream;
    } else {
      this._bodyStream = null;
    }
  }

  private _markBodyUsed(method: string) {
    if (this._bodyUsed) {
      throw new TypeError(
        `Failed to execute '${method}' on 'Response': body stream already read`,
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

  get bodyUsed() {
    return this._bodyUsed;
  }

  get headers() {
    return this._headers;
  }

  get ok() {
    return this._status >= 200 && this._status <= 299;
  }

  get status() {
    return this._status;
  }

  get statusText() {
    return this._statusText;
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
    const clonedResponse = new Response(null, {
      status: this._status,
      statusText: this._statusText,
      headers: clonedHeaderPairs,
    });
    clonedResponse._bodyStream = bodyStreamB;
    this._bodyStream = bodyStreamA;
    return clonedResponse;
  }

  get url(): string {
    return this._url;
  }

  get type() {
    // TODO: There are more types, but we haven't implemented any of the
    // functionality that would result in them
    // https://developer.mozilla.org/en-US/docs/Web/API/Response/type.
    return this[_responseType];
  }

  inspect() {
    const properties = {
      bodyUsed: this._bodyUsed,
      headers: this._headers,
      ok: this.ok,
      status: this.status,
      statusText: this.statusText,
      url: this._url,
    };
    return `Response ${inspect(properties)}`;
  }

  toJSON() {
    return {};
  }

  // ---------------------------------------------------------------
  // Begin unimplemented functions
  // ---------------------------------------------------------------

  get redirected() {
    return this[_redirected];
  }
}

Object.defineProperty(Response.prototype, Symbol.toStringTag, {
  value: "Response",
  enumerable: false,
  writable: false,
  configurable: true,
});

export const convexJsonFromResponse = ({ response }: { response: any }) => {
  if (!(response instanceof Response)) {
    throw new Error("HTTP actions must return a Response");
  }
  const streamId = constructStreamId(response.body);
  const headerPairs = [...response.headers.entries()];
  if (
    response[_contentLength] !== null &&
    !response.headers.has("content-length")
  ) {
    headerPairs.push(["content-length", String(response[_contentLength])]);
  }
  return {
    headerPairs,
    status: response.status,
    streamId,
    url: response.url !== "" ? response.url : undefined,
  };
};

export const responseFromConvexObject = (
  convexObject: Record<string, any>,
  mapBodyError?: (e: unknown) => unknown,
  requestMethod?: string,
) => {
  const status = Number(convexObject.status);
  const body =
    requestMethod !== "HEAD" &&
    !NULL_BODY_STATUSES.includes(status) &&
    convexObject.streamId
      ? extractStream(convexObject.streamId, mapBodyError)
      : null;
  const response = new Response(body, {
    status,
    statusText: convexObject.statusText,
    headers: convexObject.headerPairs,
    url: convexObject.url,
  });
  response[_responseType] = "basic";
  return response;
};

export const setupResponse = (global: any) => {
  global.Response = Response;
};
