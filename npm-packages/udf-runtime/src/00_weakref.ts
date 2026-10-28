// Capture these in case they are overridden
const originalTypeError = TypeError;
const originalWeakSet = WeakSet;
const originalWeakSetAdd = WeakSet.prototype.add;
const originalWeakSetDelete = WeakSet.prototype.delete;
const originalSymbolKeyFor = Symbol.keyFor;

export function setupWeakRefs(global) {
  // Patch `WeakRef` with a noop implementation since it externalizes non-deterministic GC decisions.
  delete global.WeakRef;
  Object.defineProperty(global, "WeakRef", {
    value: WeakRef,
    writable: true,
    enumerable: false,
    configurable: true,
  });

  // Patch `FinalizationRegistry` with our own version that never runs cleanup
  // callbacks.
  delete global.FinalizationRegistry;
  Object.defineProperty(global, "FinalizationRegistry", {
    value: FinalizationRegistry,
    writable: true,
    enumerable: false,
    configurable: true,
  });
}

// Implementation of
// https://tc39.es/ecma262/multipage/managing-memory.html#sec-finalization-registry-objects
// whose cleanup callback is never called.
class FinalizationRegistry {
  #unregisterTokens: WeakSet<any> = new originalWeakSet();

  constructor(callbackFn: (heldValue: any) => void) {
    if (typeof callbackFn !== "function") {
      throw new originalTypeError("cleanup must be callable");
    }
  }

  register(target, heldValue, unregisterToken = undefined) {
    // default `undefined` so that `FinalizationRegistry.prototype.register.length === 2`

    const unregisterTokens = this.#unregisterTokens; // also checks the brand on `this`
    if (!CanBeHeldWeakly(target)) {
      throw new originalTypeError("target must be an object");
    }
    if (target === heldValue) {
      throw new originalTypeError("target and holdings must not be same");
    }
    if (unregisterToken !== undefined) {
      if (!CanBeHeldWeakly(unregisterToken)) {
        throw new originalTypeError("unregisterToken must be an object");
      }
      // Hold the token so that `unregister` returns the correct value.
      originalWeakSetAdd.call(unregisterTokens, unregisterToken);
    }
  }

  unregister(unregisterToken) {
    const unregisterTokens = this.#unregisterTokens;
    if (!CanBeHeldWeakly(unregisterToken)) {
      throw new originalTypeError("unregisterToken must be an object");
    }
    return originalWeakSetDelete.call(unregisterTokens, unregisterToken);
  }
}

// Implementation of https://tc39.es/ecma262/multipage/managing-memory.html#sec-weak-ref-objects
// that is just a strong reference under the hood.
class WeakRef {
  #target: any;

  constructor(target) {
    if (!CanBeHeldWeakly(target)) {
      throw new originalTypeError("target must be an object");
    }
    this.#target = target;
  }

  deref() {
    return this.#target;
  }
}

Object.defineProperty(FinalizationRegistry.prototype, Symbol.toStringTag, {
  value: "FinalizationRegistry",
  enumerable: false,
  writable: false,
  configurable: true,
});
Object.defineProperty(WeakRef.prototype, Symbol.toStringTag, {
  value: "WeakRef",
  enumerable: false,
  writable: false,
  configurable: true,
});

// https://tc39.es/ecma262/multipage/executable-code-and-execution-contexts.html#sec-canbeheldweakly
function CanBeHeldWeakly(v: any) {
  if (v !== null && (typeof v === "object" || typeof v === "function")) {
    return true;
  }
  // Symbols in the global registry are excluded: they have no language identity
  // and are never collected.
  if (typeof v === "symbol" && originalSymbolKeyFor(v) === undefined) {
    return true;
  }
  return false;
}
