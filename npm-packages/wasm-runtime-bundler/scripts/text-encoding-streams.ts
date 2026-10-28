// `TextEncoderStream` and `TextDecoderStream` (Encoding Standard §9.2, §9.3),
// built on the native `TextEncoder`/`TextDecoder`

let TransformStreamImpl: typeof TransformStream;
let TextEncoderImpl: typeof TextEncoder;
let TextDecoderImpl: typeof TextDecoder;

class TextDecoderStream {
  #decoder: TextDecoder;
  #transform: TransformStream<BufferSource, string>;

  constructor(label: string = "utf-8", options: TextDecoderOptions = {}) {
    this.#decoder = new TextDecoderImpl(String(label), options);
    this.#transform = new TransformStreamImpl({
      transform: (chunk, controller) => {
        // decode() treats undefined as empty input, but stream chunks must be BufferSources.
        if (chunk === undefined) {
          throw new TypeError(
            "TextDecoderStream requires a BufferSource chunk",
          );
        }
        const decoded = this.#decoder.decode(chunk, { stream: true });
        if (decoded) {
          controller.enqueue(decoded);
        }
      },
      flush: (controller) => {
        const final = this.#decoder.decode();
        if (final) {
          controller.enqueue(final);
        }
      },
    });
  }

  get encoding(): string {
    return this.#decoder.encoding;
  }

  get fatal(): boolean {
    return this.#decoder.fatal;
  }

  get ignoreBOM(): boolean {
    return this.#decoder.ignoreBOM;
  }

  get readable(): ReadableStream<string> {
    return this.#transform.readable;
  }

  get writable(): WritableStream<BufferSource> {
    return this.#transform.writable;
  }
}

class TextEncoderStream {
  // A trailing high surrogate is held back until the next chunk shows whether
  // a low surrogate follows it.
  #pendingHighSurrogate: string | null = null;
  #encoder = new TextEncoderImpl();
  #transform: TransformStream<string, Uint8Array>;

  constructor() {
    this.#transform = new TransformStreamImpl({
      transform: (chunk, controller) => {
        chunk = String(chunk);
        if (this.#pendingHighSurrogate !== null) {
          chunk = this.#pendingHighSurrogate + chunk;
          this.#pendingHighSurrogate = null;
        }
        if (chunk === "") {
          return;
        }
        const lastCodeUnit = chunk.charCodeAt(chunk.length - 1);
        if (0xd800 <= lastCodeUnit && lastCodeUnit <= 0xdbff) {
          this.#pendingHighSurrogate = chunk.slice(-1);
          chunk = chunk.slice(0, -1);
        }
        if (chunk) {
          controller.enqueue(this.#encoder.encode(chunk));
        }
      },
      flush: (controller) => {
        if (this.#pendingHighSurrogate !== null) {
          controller.enqueue(new Uint8Array([0xef, 0xbf, 0xbd]));
        }
      },
    });
  }

  get encoding(): string {
    return "utf-8";
  }

  get readable(): ReadableStream<Uint8Array> {
    return this.#transform.readable;
  }

  get writable(): WritableStream<string> {
    return this.#transform.writable;
  }
}

for (const cls of [TextDecoderStream, TextEncoderStream]) {
  Object.defineProperty(cls.prototype, Symbol.toStringTag, {
    value: cls.name,
    configurable: true,
  });
}

export const setupTextEncodingStreams = (global: any) => {
  // Capture globals at setup time in case they are overridden at runtime
  TransformStreamImpl = global.TransformStream;
  TextEncoderImpl = global.TextEncoder;
  TextDecoderImpl = global.TextDecoder;
  // Interface objects are writable and configurable but not enumerable.
  for (const cls of [TextDecoderStream, TextEncoderStream]) {
    Object.defineProperty(global, cls.name, {
      value: cls,
      writable: true,
      configurable: true,
    });
  }
};
