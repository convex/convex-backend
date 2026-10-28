type AsyncContextKey = AsyncLocalStorage<unknown>;
type AsyncContextData = ReadonlyMap<AsyncContextKey, unknown> | undefined;

let _getCPED: (() => unknown) | undefined;
let _setCPED: ((v: unknown) => void) | undefined;

function cloneAsyncContext(
  context: AsyncContextData,
): Map<AsyncContextKey, unknown> {
  return context ? new Map(context) : new Map();
}

function getAsyncContext(): AsyncContextData {
  const value = _getCPED!();
  return value instanceof Map ? (value as AsyncContextData) : undefined;
}

function setAsyncContext(context: AsyncContextData): void {
  _setCPED!(context);
}

export class AsyncLocalStorage<T = unknown> {
  #disabled = false;

  static bind<F extends (...args: unknown[]) => unknown>(fn: F): F {
    if (typeof fn !== "function") {
      throw new TypeError("fn must be a function");
    }
    const boundSnapshot = AsyncLocalStorage.snapshot();
    return ((...args: unknown[]) => boundSnapshot(fn, ...args)) as unknown as F;
  }

  static snapshot(): <R, TArgs extends unknown[]>(
    fn: (...args: TArgs) => R,
    ...args: TArgs
  ) => R {
    const context = getAsyncContext();
    return <R, TArgs extends unknown[]>(
      fn: (...args: TArgs) => R,
      ...args: TArgs
    ): R => {
      const prev = getAsyncContext();
      setAsyncContext(context);
      try {
        return fn(...args);
      } finally {
        setAsyncContext(prev);
      }
    };
  }

  enterWith(store: T): void {
    this.#disabled = false;

    const context = getAsyncContext();
    if (context?.has(this) && Object.is(context.get(this), store)) {
      return;
    }

    const nextContext = cloneAsyncContext(context);
    nextContext.set(this, store);
    setAsyncContext(nextContext);
  }

  exit<R, TArgs extends unknown[]>(
    callback: (...args: TArgs) => R,
    ...args: TArgs
  ): R {
    return this.run(undefined as T, callback, ...args);
  }

  run<R, TArgs extends unknown[]>(
    store: T,
    callback: (...args: TArgs) => R,
    ...args: TArgs
  ): R {
    const wasDisabled = this.#disabled;
    this.#disabled = false;

    const previousContext = getAsyncContext();
    if (
      !wasDisabled &&
      previousContext?.has(this) &&
      Object.is(previousContext.get(this), store)
    ) {
      return callback(...args);
    }

    const nextContext = cloneAsyncContext(previousContext);
    nextContext.set(this, store);
    setAsyncContext(nextContext);

    try {
      return callback(...args);
    } finally {
      setAsyncContext(previousContext);
    }
  }

  disable(): void {
    if (this.#disabled) return;
    this.#disabled = true;

    const context = getAsyncContext();
    if (context?.has(this)) {
      const nextContext = cloneAsyncContext(context);
      nextContext.delete(this);
      setAsyncContext(nextContext.size > 0 ? nextContext : undefined);
    }
  }

  getStore(): T | undefined {
    if (this.#disabled) return undefined;

    const context = getAsyncContext();
    return context?.get(this) as T | undefined;
  }
}

export class AsyncResource {
  readonly type: string;
  #snapshot: AsyncContextData;

  constructor(type: string, _options?: { triggerAsyncId?: number } | number) {
    if (typeof type !== "string") {
      throw new TypeError("type must be a string");
    }

    this.type = type;
    this.#snapshot = getAsyncContext();
  }

  runInAsyncScope<R, This, TArgs extends unknown[]>(
    fn: (this: This, ...args: TArgs) => R,
    thisArg?: This,
    ...args: TArgs
  ): R {
    const prev = getAsyncContext();
    setAsyncContext(this.#snapshot);
    try {
      return fn.apply(thisArg as This, args);
    } finally {
      setAsyncContext(prev);
    }
  }

  bind<F extends (...args: unknown[]) => unknown>(fn: F, thisArg?: unknown): F {
    if (typeof fn !== "function") {
      throw new TypeError("fn must be a function");
    }
    const snapshot = this.#snapshot;
    const bound = function (this: unknown, ...args: unknown[]) {
      const prev = getAsyncContext();
      setAsyncContext(snapshot);
      try {
        return fn.apply(thisArg ?? this, args);
      } finally {
        setAsyncContext(prev);
      }
    };
    return bound as F;
  }

  static bind<F extends (...args: unknown[]) => unknown>(
    fn: F,
    type?: string,
    thisArg?: unknown,
  ): F {
    type = type || fn.name || "bound-anonymous-fn";
    return new AsyncResource(type).bind(fn, thisArg);
  }
}

export function setupAsyncHooks(global: typeof globalThis): void {
  const convex = (global as Record<string, any>).Convex;
  _getCPED = convex.getContinuationPreservedEmbedderData;
  _setCPED = convex.setContinuationPreservedEmbedderData;
  delete convex.getContinuationPreservedEmbedderData;
  delete convex.setContinuationPreservedEmbedderData;

  Object.defineProperty(global, "AsyncLocalStorage", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: AsyncLocalStorage,
  });
  Object.defineProperty(global, "AsyncResource", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: AsyncResource,
  });
}
