import { Blob, convertBlobPart } from "./09_file.js";
import { FormData, formDataToBlob } from "./21_formdata.js";
import { ReadableStream } from "./06_streams.js";

export type BodyInit =
  | string
  | ArrayBuffer
  | ArrayBufferView
  | Blob
  | FormData
  | URLSearchParams
  | ReadableStream;

/**
 * Converts a JS value to the WebIDL union
 * `(ReadableStream or Blob or BufferSource or FormData or URLSearchParams or USVString)`.
 */
export function convertBodyInit(value: unknown): BodyInit {
  if (
    value instanceof FormData ||
    value instanceof URLSearchParams ||
    value instanceof ReadableStream
  ) {
    return value;
  }
  return convertBlobPart(value);
}

/** https://fetch.spec.whatwg.org/#concept-bodyinit-extract */
export function extractBody(body: BodyInit): {
  stream: ReadableStream;
  contentType: string | null;
  // `null` when the length isn't known up front (a `ReadableStream` body).
  contentLength: number | null;
} {
  if (body instanceof ReadableStream) {
    return { stream: body, contentType: null, contentLength: null };
  }
  let blob: Blob;
  let contentType: string | null = null;
  if (body instanceof FormData) {
    blob = formDataToBlob(body);
    contentType = blob.type;
  } else if (body instanceof URLSearchParams) {
    blob = new Blob([body.toString()]);
    contentType = "application/x-www-form-urlencoded;charset=UTF-8";
  } else if (typeof body === "string") {
    blob = new Blob([body]);
    contentType = "text/plain;charset=UTF-8";
  } else if (body instanceof Blob) {
    blob = body;
    contentType = body.type === "" ? null : body.type;
  } else {
    blob = new Blob([body]);
  }
  return { stream: blob.stream(), contentType, contentLength: blob.size };
}
