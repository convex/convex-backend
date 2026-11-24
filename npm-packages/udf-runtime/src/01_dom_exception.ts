// Defined in WebIDL 4.3.
// https://webidl.spec.whatwg.org/#idl-DOMException
const INDEX_SIZE_ERR = 1;
const DOMSTRING_SIZE_ERR = 2;
const HIERARCHY_REQUEST_ERR = 3;
const WRONG_DOCUMENT_ERR = 4;
const INVALID_CHARACTER_ERR = 5;
const NO_DATA_ALLOWED_ERR = 6;
const NO_MODIFICATION_ALLOWED_ERR = 7;
const NOT_FOUND_ERR = 8;
const NOT_SUPPORTED_ERR = 9;
const INUSE_ATTRIBUTE_ERR = 10;
const INVALID_STATE_ERR = 11;
const SYNTAX_ERR = 12;
const INVALID_MODIFICATION_ERR = 13;
const NAMESPACE_ERR = 14;
const INVALID_ACCESS_ERR = 15;
const VALIDATION_ERR = 16;
const TYPE_MISMATCH_ERR = 17;
const SECURITY_ERR = 18;
const NETWORK_ERR = 19;
const ABORT_ERR = 20;
const URL_MISMATCH_ERR = 21;
const QUOTA_EXCEEDED_ERR = 22;
const TIMEOUT_ERR = 23;
const INVALID_NODE_TYPE_ERR = 24;
const DATA_CLONE_ERR = 25;

// Defined in WebIDL 2.8.1.
// https://webidl.spec.whatwg.org/#dfn-error-names-table
/** @type {Record<string, number>} */
// the prototype should be null, to prevent user code from looking
// up Object.prototype properties, such as "toString"
const nameToCodeMapping: Record<string, number> = Object.create(null, {
  IndexSizeError: { value: INDEX_SIZE_ERR },
  HierarchyRequestError: { value: HIERARCHY_REQUEST_ERR },
  WrongDocumentError: { value: WRONG_DOCUMENT_ERR },
  InvalidCharacterError: { value: INVALID_CHARACTER_ERR },
  NoModificationAllowedError: { value: NO_MODIFICATION_ALLOWED_ERR },
  NotFoundError: { value: NOT_FOUND_ERR },
  NotSupportedError: { value: NOT_SUPPORTED_ERR },
  InUseAttributeError: { value: INUSE_ATTRIBUTE_ERR },
  InvalidStateError: { value: INVALID_STATE_ERR },
  SyntaxError: { value: SYNTAX_ERR },
  InvalidModificationError: { value: INVALID_MODIFICATION_ERR },
  NamespaceError: { value: NAMESPACE_ERR },
  InvalidAccessError: { value: INVALID_ACCESS_ERR },
  TypeMismatchError: { value: TYPE_MISMATCH_ERR },
  SecurityError: { value: SECURITY_ERR },
  NetworkError: { value: NETWORK_ERR },
  AbortError: { value: ABORT_ERR },
  URLMismatchError: { value: URL_MISMATCH_ERR },
  QuotaExceededError: { value: QUOTA_EXCEEDED_ERR },
  TimeoutError: { value: TIMEOUT_ERR },
  InvalidNodeTypeError: { value: INVALID_NODE_TYPE_ERR },
  DataCloneError: { value: DATA_CLONE_ERR },
});

class DOMException {
  #message: string;
  #name: string;

  constructor(message: unknown = "", name: unknown = "Error") {
    // Coerce `message`/`name` to string
    this.#message = `${message}`;
    this.#name = `${name}`;

    const error = new Error(this.#message);
    error.name = "DOMException";
    Object.defineProperty(this, "stack", {
      // N.B.: this calls `prepareStackTrace` that populates `__frameData`.
      value: error.stack,
      writable: true,
      configurable: true,
    });

    Object.defineProperty(this, "__frameData", {
      value: (error as any).__frameData ?? [],
      writable: true,
      configurable: true,
    });
  }

  get name() {
    return this.#name;
  }

  get message() {
    return this.#message;
  }

  get code() {
    return nameToCodeMapping[this.#name] || 0;
  }
}

// Web IDL attributes are enumerable, unlike JavaScript class accessors.
for (const key of ["name", "message", "code"]) {
  Object.defineProperty(DOMException.prototype, key, { enumerable: true });
}
Object.defineProperty(DOMException.prototype, Symbol.toStringTag, {
  value: "DOMException",
  enumerable: false,
  writable: false,
  configurable: true,
});

const entries = Object.entries({
  INDEX_SIZE_ERR,
  DOMSTRING_SIZE_ERR,
  HIERARCHY_REQUEST_ERR,
  WRONG_DOCUMENT_ERR,
  INVALID_CHARACTER_ERR,
  NO_DATA_ALLOWED_ERR,
  NO_MODIFICATION_ALLOWED_ERR,
  NOT_FOUND_ERR,
  NOT_SUPPORTED_ERR,
  INUSE_ATTRIBUTE_ERR,
  INVALID_STATE_ERR,
  SYNTAX_ERR,
  INVALID_MODIFICATION_ERR,
  NAMESPACE_ERR,
  INVALID_ACCESS_ERR,
  VALIDATION_ERR,
  TYPE_MISMATCH_ERR,
  SECURITY_ERR,
  NETWORK_ERR,
  ABORT_ERR,
  URL_MISMATCH_ERR,
  QUOTA_EXCEEDED_ERR,
  TIMEOUT_ERR,
  INVALID_NODE_TYPE_ERR,
  DATA_CLONE_ERR,
});

for (let i = 0; i < entries.length; ++i) {
  const [key, value] = entries[i];
  const desc = { value, enumerable: true };
  Object.defineProperty(DOMException, key, desc);
  Object.defineProperty(DOMException.prototype, key, desc);
}

Object.setPrototypeOf(DOMException.prototype, Error.prototype);

export const setupDOMException = (global: any) => {
  Object.defineProperty(global, "DOMException", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: DOMException,
  });
};

function validateQuotaOption(value: unknown, member: string) {
  if (value === undefined) {
    return null;
  }
  const number = Number(value);
  if (!Number.isFinite(number)) {
    throw new TypeError(
      `Failed to construct 'QuotaExceededError': The provided ${member} value is non-finite.`,
    );
  }
  if (number < 0) {
    throw new RangeError(
      `Failed to construct 'QuotaExceededError': The ${member} value cannot be negative.`,
    );
  }
  return number;
}

/**
 * The WebIDL `QuotaExceededError` interface, which replaced the
 * "QuotaExceededError" `DOMException` name.
 * https://webidl.spec.whatwg.org/#quotaexceedederror
 *
 * It extends the global `DOMException` rather than the class above because the
 * wasm runtime's global `DOMException` is QuickJS's native one.
 */
export const setupQuotaExceededError = (global: any) => {
  class QuotaExceededError extends global.DOMException {
    #quota: number | null;
    #requested: number | null;

    constructor(message: unknown = "", options: unknown = {}) {
      if (
        options !== undefined &&
        options !== null &&
        typeof options !== "object" &&
        typeof options !== "function"
      ) {
        throw new TypeError(
          "Failed to construct 'QuotaExceededError': The provided value is not of type 'QuotaExceededErrorOptions'.",
        );
      }
      const { quota, requested } = (options ?? {}) as {
        quota?: unknown;
        requested?: unknown;
      };
      const validatedQuota = validateQuotaOption(quota, "quota");
      const validatedRequested = validateQuotaOption(requested, "requested");
      if (
        validatedQuota !== null &&
        validatedRequested !== null &&
        validatedRequested < validatedQuota
      ) {
        throw new RangeError(
          "Failed to construct 'QuotaExceededError': The requested value cannot be less than the quota value.",
        );
      }
      super(message, "QuotaExceededError");
      this.#quota = validatedQuota;
      this.#requested = validatedRequested;
    }

    get quota() {
      return this.#quota;
    }

    get requested() {
      return this.#requested;
    }
  }

  for (const key of ["quota", "requested"]) {
    Object.defineProperty(QuotaExceededError.prototype, key, {
      enumerable: true,
    });
  }
  Object.defineProperty(QuotaExceededError.prototype, Symbol.toStringTag, {
    value: "QuotaExceededError",
    enumerable: false,
    writable: false,
    configurable: true,
  });

  Object.defineProperty(global, "QuotaExceededError", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: QuotaExceededError,
  });
};
