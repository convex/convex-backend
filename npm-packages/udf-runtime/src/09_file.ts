// The initial implementation were taken from Deno.
// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.
// https://github.com/denoland/deno/blob/main/LICENSE.md

import { ReadableStream } from "./06_streams";

async function* toIterator(
  parts: (BlobReference | BlobStreamReference | Blob)[],
): AsyncGenerator<Uint8Array> {
  for (const part of parts) {
    if (part instanceof Blob) {
      yield* part.stream();
    } else if (part instanceof BlobReference) {
      yield new Uint8Array(part.arrayBuffer());
    } else if (part instanceof BlobStreamReference) {
      yield* part.stream();
    } else {
      throw new Error("unrecognized part");
    }
  }
}

class BlobReference {
  #data: ArrayBuffer;

  constructor(data: ArrayBuffer) {
    this.#data = data;
  }

  static fromUint8Array(data: Uint8Array) {
    // Copy the data to freeze it, in case `data` gets mutated later
    return new BlobReference(data.slice().buffer);
  }

  slice(start: number, end: number): BlobReference {
    return new BlobReference(this.#data.slice(start, end));
  }

  arrayBuffer(): ArrayBuffer {
    return this.#data.slice();
  }

  get size() {
    return this.#data.byteLength;
  }
}

class BlobStreamReference {
  private _stream: ReadableStream<Uint8Array> | null;
  private _size: number;

  constructor(stream: ReadableStream<Uint8Array>, size: number) {
    this._stream = stream;
    this._size = size;
  }

  slice(start: number, end: number): BlobStreamReference {
    if (this._stream === null) {
      throw new TypeError("Can't re-read streaming Blob");
    }

    const size = end - start;
    const [original, toSlice] = this._stream.tee();
    this._stream = original;

    const reader = toSlice.getReader();
    let bytesRead = 0;
    const sliced = new ReadableStream({
      type: "bytes",
      async pull(controller) {
        while (true) {
          const { value, done } = await reader.read();
          if (done || bytesRead >= end) return controller.close();
          const valueSlice = value.slice(
            Math.max(0, start - bytesRead),
            end - bytesRead,
          );
          bytesRead += value.length;
          if (valueSlice.byteLength > 0) {
            return controller.enqueue(valueSlice);
          }
        }
      },
    });
    return new BlobStreamReference(sliced, size);
  }

  stream(): ReadableStream<Uint8Array> {
    if (this._stream === null) {
      // TODO: Blobs aren't really supposed to be streaming
      throw new TypeError("Can't re-read streaming Blob");
    }
    const stream = this._stream;
    this._stream = null;
    return stream;
  }

  get size() {
    return this._size;
  }
}

function iteratorToReadableStream(
  iterator: AsyncIterator<Uint8Array>,
): ReadableStream<Uint8Array> {
  return new ReadableStream({
    type: "bytes",
    async pull(controller) {
      while (true) {
        const { value, done } = await iterator.next();
        if (done) return controller.close();
        if (value.byteLength > 0) {
          return controller.enqueue(value);
        }
      }
    },
  });
}

const NORMALIZE_PATTERN = new RegExp(/^[\x20-\x7E]*$/);

type BlobPart = string | ArrayBufferView | ArrayBuffer | Blob;

// Brand checks for the [[ArrayBufferData]] slot: the prototype getters throw
// on anything that isn't a real (non-shared, resp. shared) buffer, which
// `instanceof` can't distinguish from an object with a spoofed prototype.
function isArrayBuffer(value: unknown): value is ArrayBuffer {
  try {
    Object.getOwnPropertyDescriptor(
      ArrayBuffer.prototype,
      "byteLength",
    )!.get!.call(value);
    return true;
  } catch {
    return false;
  }
}

function isSharedArrayBuffer(value: unknown): value is SharedArrayBuffer {
  // Looked up per call: V8 leaves `SharedArrayBuffer` out of snapshots (it's
  // one of the "experimental" globals installed only when a context is
  // created), so it's undefined while this module is evaluated into the
  // snapshot but defined by the time user code runs.
  if (typeof SharedArrayBuffer === "undefined") {
    return false;
  }
  try {
    Object.getOwnPropertyDescriptor(
      SharedArrayBuffer.prototype,
      "byteLength",
    )!.get!.call(value);
    return true;
  } catch {
    return false;
  }
}

/**
 * Converts a JS value to the WebIDL union `(BufferSource or Blob or USVString)`.
 * `BufferSource` lacks `[AllowShared]`, so shared buffers are rejected rather
 * than falling back to the string member.
 */
export function convertBlobPart(value: unknown): BlobPart {
  if (value instanceof Blob || isArrayBuffer(value)) {
    return value;
  }
  if (ArrayBuffer.isView(value)) {
    if (isSharedArrayBuffer(value.buffer)) {
      throw new TypeError(
        "An ArrayBufferView backed by a SharedArrayBuffer is not allowed",
      );
    }
    return value;
  }
  if (isSharedArrayBuffer(value)) {
    throw new TypeError("A SharedArrayBuffer is not allowed");
  }
  // A template literal (unlike `String()`) throws a TypeError for symbols, as
  // WebIDL's ToString requires.
  return `${value as any}`;
}

// https://webidl.spec.whatwg.org/#js-sequence
function convertBlobParts(value: unknown): BlobPart[] {
  if (
    value === null ||
    (typeof value !== "object" && typeof value !== "function")
  ) {
    throw new TypeError("Blob parts must be a sequence");
  }
  const parts: BlobPart[] = [];
  // `for...of` throws a TypeError if `value` has no `Symbol.iterator`.
  for (const part of value as Iterable<unknown>) {
    parts.push(convertBlobPart(part));
  }
  return parts;
}

export class Blob {
  private _parts: (BlobReference | BlobStreamReference | Blob)[];
  private _size: number;
  private _type: string;

  constructor(blobParts?: Iterable<BlobPart>, options?: BlobPropertyBag) {
    // WebIDL converts `blobParts`, then the members of `options` in
    // lexicographic order (`endings`, then `type`).
    const convertedParts =
      blobParts === undefined ? [] : convertBlobParts(blobParts);
    if (
      options !== undefined &&
      options !== null &&
      typeof options !== "object" &&
      typeof options !== "function"
    ) {
      throw new TypeError("Blob options must be an object");
    }
    const endings =
      options?.endings === undefined ? "transparent" : `${options.endings}`;
    if (endings !== "transparent" && endings !== "native") {
      throw new TypeError(
        `'${endings}' is not a valid value for enumeration EndingType`,
      );
    }
    const type = options?.type === undefined ? "" : `${options.type}`;

    const { parts, size } = Blob._processBlobParts(convertedParts, endings);
    this._parts = parts;
    this._size = size;
    this._type = Blob._normalizeType(type);
  }

  get size(): number {
    return this._size;
  }

  get type(): string {
    return this._type;
  }

  slice(start?: number, end?: number, contentType?: string): Blob {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const O = this;
    let relativeStart: number;
    if (start === undefined) {
      relativeStart = 0;
    } else {
      if (start < 0) {
        relativeStart = Math.max(O.size + start, 0);
      } else {
        relativeStart = Math.min(start, O.size);
      }
    }
    let relativeEnd;
    if (end === undefined) {
      relativeEnd = O.size;
    } else {
      if (end < 0) {
        relativeEnd = Math.max(O.size + end, 0);
      } else {
        relativeEnd = Math.min(end, O.size);
      }
    }

    const span = Math.max(relativeEnd - relativeStart, 0);
    const blobParts: (BlobReference | BlobStreamReference | Blob)[] = [];
    let added = 0;

    const parts = this._parts;
    for (let i = 0; i < parts.length; ++i) {
      const part = parts[i];
      // don't add the overflow to new blobParts
      if (added >= span) {
        // Could maybe be possible to remove variable `added`
        // and only use relativeEnd?
        break;
      }
      const size = part.size;
      if (relativeStart && size <= relativeStart) {
        // Skip the beginning and change the relative
        // start & end position as we skip the unwanted parts
        relativeStart -= size;
        relativeEnd -= size;
      } else {
        const chunk = part.slice(
          relativeStart,
          Math.min(part.size, relativeEnd),
        );
        added += chunk.size;
        relativeEnd -= part.size;
        blobParts.push(chunk);
        relativeStart = 0; // All next sequential parts should start at 0
      }
    }

    let relativeContentType: string;
    if (contentType === undefined) {
      relativeContentType = "";
    } else {
      relativeContentType = Blob._normalizeType(`${contentType}`);
    }

    const blob = new Blob([], { type: relativeContentType });
    blob._parts = blobParts;
    blob._size = span;
    return blob;
  }

  async text(): Promise<string> {
    const decoder = new TextDecoder();
    return this.arrayBuffer().then((array) => decoder.decode(array));
  }

  async arrayBuffer(): Promise<ArrayBuffer> {
    const bytes = new Uint8Array(this._size);
    const partIterator = toIterator(this._parts);
    let offset = 0;
    while (true) {
      const { value, done } = await partIterator.next();
      if (done) break;
      const byteLength = value.byteLength;
      if (byteLength > 0) {
        bytes.set(value, offset);
        offset += byteLength;
      }
    }
    return bytes.buffer;
  }

  stream(): ReadableStream<Uint8Array> {
    const partIterator = toIterator(this._parts);
    return iteratorToReadableStream(partIterator);
  }

  // https://w3c.github.io/FileAPI/#process-blob-parts
  private static _processBlobParts(
    parts: BlobPart[],
    endings: "transparent" | "native",
  ): { parts: (BlobReference | Blob)[]; size: number } {
    const processedParts: (BlobReference | Blob)[] = [];
    let size = 0;
    for (const element of parts) {
      if (element instanceof Blob) {
        size += element.size;
        processedParts.push(element);
        continue;
      }
      let bytes: Uint8Array;
      if (typeof element === "string") {
        // The native line ending is LF: the isolate runs on Linux.
        const text =
          endings === "native" ? element.replace(/\r\n?/g, "\n") : element;
        bytes = new TextEncoder().encode(text);
      } else if (ArrayBuffer.isView(element)) {
        bytes = new Uint8Array(
          element.buffer,
          element.byteOffset,
          element.byteLength,
        );
      } else {
        bytes = new Uint8Array(element);
      }
      size += bytes.byteLength;
      processedParts.push(BlobReference.fromUint8Array(bytes));
    }
    return { parts: processedParts, size };
  }

  private static _normalizeType(str: string): string {
    let normalizedType;
    if (!str || !NORMALIZE_PATTERN.test(str)) {
      normalizedType = "";
    } else {
      normalizedType = str;
    }
    return normalizedType.toLowerCase();
  }

  static fromStream(
    stream: ReadableStream<Uint8Array>,
    size: number,
    type?: string,
  ): Blob {
    const blob = new Blob([], { type });
    // When creating a Blob from a stream we want to lock the stream synchronously
    // so `Request.body.locked` is true, while still retaining the ability to return
    // an unlocked stream from `Blob.stream()`.
    const newStream = iteratorToReadableStream(stream[Symbol.asyncIterator]());
    blob._parts = [new BlobStreamReference(newStream, size)];
    blob._size = size;
    return blob;
  }

  inspect() {
    return `Blob { size: ${this.size}, type: "${this.type}" }`;
  }
}

Object.defineProperty(Blob.prototype, Symbol.toStringTag, {
  value: "Blob",
  enumerable: false,
  writable: false,
  configurable: true,
});

export class File extends Blob {
  private _fileName: string;
  private _lastModified: number;

  constructor(
    fileParts: BlobPart[],
    fileName: string,
    options?: FilePropertyBag,
  ) {
    super(fileParts, options);
    this._fileName = String(fileName);
    this._lastModified = options?.lastModified ?? Date.now();
  }

  get name() {
    return this._fileName;
  }

  get lastModified() {
    return this._lastModified;
  }
}

Object.defineProperty(File.prototype, Symbol.toStringTag, {
  value: "File",
  enumerable: false,
  writable: false,
  configurable: true,
});

export const setupBlob = (global: any) => {
  global.Blob = Blob;
  global.File = File;
};
