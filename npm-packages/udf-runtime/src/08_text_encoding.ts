// The initial implementation taken from Deno.
// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.
// https://github.com/denoland/deno/blob/main/LICENSE.md

import { performOp } from "udf-syscall-ffi";
import { copyBuffer } from "./helpers.js";
import inspect from "object-inspect";
import { TransformStream } from "./06_streams.js";

class TextEncoder {
  get encoding() {
    return "utf-8";
  }
  encode(text = "") {
    if (typeof text !== "string") text = (text as any).toString();
    return performOp("textEncoder/encode", text);
  }
  encodeInto(input: string, dest: Uint8Array) {
    if (!(dest instanceof Uint8Array)) {
      throw new TypeError(
        "Failed to execute 'encodeInto' on 'TextEncoder': parameter 2 is not of type 'Uint8Array'.",
      );
    }
    const space = dest.length;
    const output = performOp("textEncoder/encodeInto", input, space);
    const { bytes, read, written } = output;
    dest.set(bytes, 0);
    return { read, written };
  }
}

Object.defineProperty(TextEncoder.prototype, Symbol.toStringTag, {
  value: "TextEncoder",
  enumerable: false,
  writable: false,
  configurable: true,
});

class TextDecoder {
  #encoding: string;
  #fatal: boolean;
  #ignoreBOM: boolean;
  // Opaque handle to the Rust-side decoder state, created lazily on the first
  // streaming decode. The Rust state is garbage collected together with this
  // object.
  #decoder: object | null;
  constructor(label = "utf-8", options: TextDecoderOptions = {}) {
    const { label: encoding, error } = performOp(
      "textEncoder/normalizeLabel",
      label,
    );
    if (error) {
      throw new RangeError(error);
    }

    this.#encoding = encoding;
    this.#fatal = Boolean(options.fatal);
    this.#ignoreBOM = Boolean(options.ignoreBOM);
    this.#decoder = null;
  }
  get encoding() {
    return this.#encoding;
  }

  get fatal() {
    return this.#fatal;
  }

  get ignoreBOM() {
    return this.#ignoreBOM;
  }

  decode(buffer?: ArrayBufferView | ArrayBuffer, options?: TextDecodeOptions) {
    if (buffer === undefined) {
      return "";
    }

    let stream = false;
    if (options !== undefined) {
      stream = options.stream ?? false;
    }

    try {
      if (!stream && this.#decoder === null) {
        const { text, error } = performOp("textEncoder/decodeSingle", {
          bytes: copyBuffer(buffer),
          encoding: this.encoding,
          fatal: this.fatal,
          ignoreBOM: this.ignoreBOM,
        });
        if (error) {
          throw new TypeError(error);
        }
        return text;
      }

      if (this.#decoder === null) {
        this.#decoder = performOp(
          "textEncoder/newDecoder",
          this.#encoding,
          this.#fatal,
          this.#ignoreBOM,
        );
      }

      const { text, error } = performOp(
        "textEncoder/decode",
        copyBuffer(buffer),
        this.#decoder,
        stream,
      );
      if (error) {
        throw new TypeError(error);
      }
      return text;
    } finally {
      if (!stream) {
        this.#decoder = null;
      }
    }
  }
}

Object.defineProperty(TextDecoder.prototype, Symbol.toStringTag, {
  value: "TextDecoder",
  enumerable: false,
  writable: false,
  configurable: true,
});

const { atob, btoa } = {
  atob(encoded: string): string {
    if (arguments.length === 0) {
      throw new TypeError('The "input" argument must be specified');
    }
    const { decoded, error } = performOp("atob", String(encoded));
    if (error) {
      throw new DOMException(
        `Failed to execute 'atob': ${error}`,
        "InvalidCharacterError",
      );
    }
    return decoded;
  },
  btoa(text: string): string {
    if (arguments.length === 0) {
      throw new TypeError('The "input" argument must be specified');
    }
    const { encoded, error } = performOp("btoa", String(text));
    if (error) {
      throw new DOMException(
        `Failed to execute 'btoa': ${error}`,
        "InvalidCharacterError",
      );
    }
    return encoded;
  },
};

class TextDecoderStream {
  /** @type {TextDecoder} */
  #decoder;
  /** @type {TransformStream<BufferSource, string>} */
  #transform;

  /**
   * @param {string} label
   * @param {TextDecoderOptions} options
   */
  constructor(label = "utf-8", options = {}) {
    label = String(label);
    // TODO: validate options
    this.#decoder = new TextDecoder(label, options);
    this.#transform = new TransformStream({
      // The transform and flush functions need access to TextDecoderStream's
      // `this`, so they are defined as functions rather than methods.
      transform: (chunk, controller) => {
        try {
          const decoded = this.#decoder.decode(chunk, { stream: true });
          if (decoded) {
            controller.enqueue(decoded);
          }
          return Promise.resolve();
        } catch (err) {
          return Promise.reject(err);
        }
      },
      flush: (controller) => {
        try {
          const final = this.#decoder.decode();
          if (final) {
            controller.enqueue(final);
          }
          return Promise.resolve();
        } catch (err) {
          return Promise.reject(err);
        }
      },
      cancel: (_reason) => {
        try {
          const _ = this.#decoder.decode();
          return Promise.resolve();
        } catch (err) {
          return Promise.reject(err);
        }
      },
    });
  }

  /** @returns {string} */
  get encoding() {
    return this.#decoder.encoding;
  }

  /** @returns {boolean} */
  get fatal() {
    return this.#decoder.fatal;
  }

  /** @returns {boolean} */
  get ignoreBOM() {
    return this.#decoder.ignoreBOM;
  }

  /** @returns {ReadableStream<string>} */
  get readable() {
    return this.#transform.readable;
  }

  /** @returns {WritableStream<BufferSource>} */
  get writable() {
    return this.#transform.writable;
  }

  inspect() {
    const properties = {
      encoding: this.encoding,
      fatal: this.fatal,
      ignoreBOM: this.ignoreBOM,
      readable: this.readable,
      writable: this.writable,
    };
    return `TextDecoderStream ${inspect(properties)}`;
  }
}

Object.defineProperty(TextDecoderStream.prototype, Symbol.toStringTag, {
  value: "TextDecoderStream",
  enumerable: false,
  writable: false,
  configurable: true,
});

class TextEncoderStream {
  /** @type {string | null} */
  #pendingHighSurrogate: string | null = null;
  /** @type {TransformStream<string, Uint8Array>} */
  #transform;

  constructor() {
    this.#transform = new TransformStream({
      // The transform and flush functions need access to TextEncoderStream's
      // `this`, so they are defined as functions rather than methods.
      transform: (chunk: string, controller) => {
        try {
          if (chunk === "") {
            return Promise.resolve();
          }
          if (this.#pendingHighSurrogate !== null) {
            chunk = this.#pendingHighSurrogate + chunk;
          }
          const lastCodeUnit = chunk.charCodeAt(chunk.length - 1);
          if (0xd800 <= lastCodeUnit && lastCodeUnit <= 0xdbff) {
            this.#pendingHighSurrogate = chunk.slice(-1);
            chunk = chunk.slice(0, -1);
          } else {
            this.#pendingHighSurrogate = null;
          }
          if (chunk) {
            controller.enqueue(performOp("textEncoder/encode", chunk));
          }
          return Promise.resolve();
        } catch (err) {
          return Promise.reject(err);
        }
      },
      flush: (controller) => {
        try {
          if (this.#pendingHighSurrogate !== null) {
            controller.enqueue(new Uint8Array([0xef, 0xbf, 0xbd]));
          }
          return Promise.resolve();
        } catch (err) {
          return Promise.reject(err);
        }
      },
    });
  }

  /** @returns {string} */
  get encoding() {
    return "utf-8";
  }

  /** @returns {ReadableStream<Uint8Array>} */
  get readable() {
    return this.#transform.readable;
  }

  /** @returns {WritableStream<string>} */
  get writable() {
    return this.#transform.writable;
  }

  inspect() {
    const properties = {
      encoding: this.encoding,
      readable: this.readable,
      writable: this.writable,
    };
    return `TextEncoderStream ${inspect(properties)}`;
  }
}

Object.defineProperty(TextEncoderStream.prototype, Symbol.toStringTag, {
  value: "TextEncoderStream",
  enumerable: false,
  writable: false,
  configurable: true,
});

export const setupTextEncoding = (global: any) => {
  global.atob = atob;
  global.btoa = btoa;
  global.TextEncoder = TextEncoder;
  global.TextDecoder = TextDecoder;
  global.TextDecoderStream = TextDecoderStream;
  global.TextEncoderStream = TextEncoderStream;
};
