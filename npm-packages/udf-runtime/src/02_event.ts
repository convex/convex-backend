// The initial implementation taken from Deno.
// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.
// https://github.com/denoland/deno/blob/main/LICENSE.md

import { requiredArguments } from "./helpers";

const _attributes = Symbol("[[attributes]]");
const _canceledFlag = Symbol("[[canceledFlag]]");
const _stopPropagationFlag = Symbol("[[stopPropagationFlag]]");
const _stopImmediatePropagationFlag = Symbol(
  "[[stopImmediatePropagationFlag]]",
);
const _inPassiveListener = Symbol("[[inPassiveListener]]");
const _dispatched = Symbol("[[dispatched]]");
const _isTrusted = Symbol("[[isTrusted]]");
const _path = Symbol("[[path]]");

const isTrusted = Object.getOwnPropertyDescriptor(
  {
    get isTrusted() {
      return this[_isTrusted];
    },
  },
  "isTrusted",
)?.get;

class Event {
  constructor(type: string, eventInitDict?: EventInit | undefined) {
    this[_canceledFlag] = false;
    this[_stopPropagationFlag] = false;
    this[_stopImmediatePropagationFlag] = false;
    this[_inPassiveListener] = false;
    this[_dispatched] = false;
    this[_isTrusted] = false;
    this[_path] = [];

    const eventInit = {
      bubbles: false,
      cancelable: false,
      composed: false,
      ...eventInitDict,
    };
    this[_attributes] = {
      type: String(type),
      ...eventInit,
      currentTarget: null,
      eventPhase: Event.NONE,
      target: null,
      timeStamp: Date.now(),
    };
    Reflect.defineProperty(this, "isTrusted", {
      enumerable: true,
      get: isTrusted,
    });
  }

  get type() {
    return this[_attributes].type;
  }

  get target() {
    return this[_attributes].target;
  }

  get srcElement() {
    return null;
  }

  set srcElement(_) {
    // This is deprecated
  }

  get currentTarget() {
    return this[_attributes].currentTarget;
  }

  composedPath() {
    const path = this[_path];
    if (path.length === 0) {
      return [];
    }

    if (!this.currentTarget) {
      throw new Error("assertion error");
    }
    const composedPath = [
      {
        item: this.currentTarget,
        itemInShadowTree: false,
        relatedTarget: null,
        rootOfClosedTree: false,
        slotInClosedTree: false,
        target: null,
        touchTargetList: [],
      },
    ];

    let currentTargetIndex = 0;
    let currentTargetHiddenSubtreeLevel = 0;

    for (let index = path.length - 1; index >= 0; index--) {
      const { item, rootOfClosedTree, slotInClosedTree } = path[index];

      if (rootOfClosedTree) {
        currentTargetHiddenSubtreeLevel++;
      }

      if (item === this.currentTarget) {
        currentTargetIndex = index;
        break;
      }

      if (slotInClosedTree) {
        currentTargetHiddenSubtreeLevel--;
      }
    }

    let currentHiddenLevel = currentTargetHiddenSubtreeLevel;
    let maxHiddenLevel = currentTargetHiddenSubtreeLevel;

    for (let i = currentTargetIndex - 1; i >= 0; i--) {
      const { item, rootOfClosedTree, slotInClosedTree } = path[i];

      if (rootOfClosedTree) {
        currentHiddenLevel++;
      }

      if (currentHiddenLevel <= maxHiddenLevel) {
        composedPath.unshift({
          item,
          itemInShadowTree: false,
          relatedTarget: null,
          rootOfClosedTree: false,
          slotInClosedTree: false,
          target: null,
          touchTargetList: [],
        });
      }

      if (slotInClosedTree) {
        currentHiddenLevel--;

        if (currentHiddenLevel < maxHiddenLevel) {
          maxHiddenLevel = currentHiddenLevel;
        }
      }
    }

    currentHiddenLevel = currentTargetHiddenSubtreeLevel;
    maxHiddenLevel = currentTargetHiddenSubtreeLevel;

    for (let index = currentTargetIndex + 1; index < path.length; index++) {
      const { item, rootOfClosedTree, slotInClosedTree } = path[index];

      if (slotInClosedTree) {
        currentHiddenLevel++;
      }

      if (currentHiddenLevel <= maxHiddenLevel) {
        composedPath.push({
          item,
          itemInShadowTree: false,
          relatedTarget: null,
          rootOfClosedTree: false,
          slotInClosedTree: false,
          target: null,
          touchTargetList: [],
        });
      }

      if (rootOfClosedTree) {
        currentHiddenLevel--;

        if (currentHiddenLevel < maxHiddenLevel) {
          maxHiddenLevel = currentHiddenLevel;
        }
      }
    }
    return composedPath.map((p) => p.item);
  }

  static get NONE() {
    return 0;
  }

  static get CAPTURING_PHASE() {
    return 1;
  }

  static get AT_TARGET() {
    return 2;
  }

  static get BUBBLING_PHASE() {
    return 3;
  }

  get eventPhase() {
    return this[_attributes].eventPhase;
  }

  stopPropagation() {
    this[_stopPropagationFlag] = true;
  }

  get cancelBubble() {
    return this[_stopPropagationFlag];
  }

  set cancelBubble(value: boolean) {
    this[_stopPropagationFlag] = value;
  }

  stopImmediatePropagation() {
    this[_stopPropagationFlag] = true;
    this[_stopImmediatePropagationFlag] = true;
  }

  get bubbles() {
    return this[_attributes].bubbles;
  }

  get cancelable() {
    return this[_attributes].cancelable;
  }

  get returnValue() {
    return !this[_canceledFlag];
  }

  set returnValue(value) {
    if (!value) {
      this[_canceledFlag] = true;
    }
  }

  preventDefault() {
    if (this[_attributes].cancelable && !this[_inPassiveListener]) {
      this[_canceledFlag] = true;
    }
  }

  get defaultPrevented() {
    return this[_canceledFlag];
  }

  get composed() {
    return this[_attributes].composed;
  }

  get initialized() {
    return true;
  }

  get timeStamp() {
    return this[_attributes].timeStamp;
  }
}

Object.defineProperty(Event.prototype, Symbol.toStringTag, {
  value: "Event",
  enumerable: false,
  writable: false,
  configurable: true,
});

const eventTargetData = Symbol();

export function setEventTargetData(target) {
  target[eventTargetData] = getDefaultTargetData();
}

function getDefaultTargetData() {
  return {
    assignedSlot: false,
    hasActivationBehavior: false,
    host: null,
    listeners: Object.create(null),
    mode: "",
  };
}

function getStopImmediatePropagation(event) {
  return Boolean(event[_stopImmediatePropagationFlag]);
}

function setCurrentTarget(event, value) {
  event[_attributes].currentTarget = value;
}

function setDispatched(event, value) {
  event[_dispatched] = value;
}

function setEventPhase(event, value) {
  event[_attributes].eventPhase = value;
}

function setInPassiveListener(event, value) {
  event[_inPassiveListener] = value;
}

function setPath(event, value) {
  event[_path] = value;
}

function setRelatedTarget(event, value) {
  event[_attributes].relatedTarget = value;
}

function setTarget(event, value) {
  event[_attributes].target = value;
}

function setStopImmediatePropagation(event, value) {
  event[_stopImmediatePropagationFlag] = value;
}

function getAssignedSlot(target) {
  return Boolean(target?.[eventTargetData]?.assignedSlot);
}

function getHasActivationBehavior(target) {
  return Boolean(target?.[eventTargetData]?.hasActivationBehavior);
}

function getHost(target) {
  return target?.[eventTargetData]?.host ?? null;
}

function getListeners(target) {
  return target?.[eventTargetData]?.listeners ?? {};
}

function getMode(target) {
  return target?.[eventTargetData]?.mode ?? null;
}

function getPath(event) {
  return event[_path] ?? [];
}

// DOM Logic Helper functions and type guards

/** Get the parent node, for event targets that have a parent.
 *
 * Ref: https://dom.spec.whatwg.org/#get-the-parent */
function getParent(eventTarget) {
  return isNode(eventTarget) ? eventTarget.parentNode : null;
}

function getRoot(eventTarget) {
  return isNode(eventTarget)
    ? eventTarget.getRootNode({ composed: true })
    : null;
}

function isNode(eventTarget) {
  return eventTarget?.nodeType !== undefined;
}

// https://dom.spec.whatwg.org/#concept-shadow-including-inclusive-ancestor
function isShadowInclusiveAncestor(ancestor, node) {
  while (isNode(node)) {
    if (node === ancestor) {
      return true;
    }

    if (isShadowRoot(node)) {
      node = node && getHost(node);
    } else {
      node = getParent(node);
    }
  }

  return false;
}

// Convex doesn't have a DOM that can have shadow roots.
function isShadowRoot(_nodeImpl) {
  return false;
}

function isSlottable(nodeImpl) {
  return Boolean(isNode(nodeImpl) && Reflect.has(nodeImpl, "assignedSlot"));
}

/** Retarget the target following the spec logic.
 *
 * Ref: https://dom.spec.whatwg.org/#retarget */
function retarget(a, _b) {
  while (true) {
    if (!isNode(a)) {
      return a;
    }

    const aRoot = a.getRootNode();

    // there are no ShadowRoots.
    if (aRoot) {
      return a;
    }
  }
}

// DOM Logic functions

/** Append a path item to an event's path.
 *
 * Ref: https://dom.spec.whatwg.org/#concept-event-path-append
 */
function appendToEventPath(
  eventImpl,
  target,
  targetOverride,
  relatedTarget,
  touchTargets,
  slotInClosedTree,
) {
  const itemInShadowTree = isNode(target) && isShadowRoot(getRoot(target));
  const rootOfClosedTree = isShadowRoot(target) && getMode(target) === "closed";

  getPath(eventImpl).push({
    item: target,
    itemInShadowTree,
    target: targetOverride,
    relatedTarget,
    touchTargetList: touchTargets,
    rootOfClosedTree,
    slotInClosedTree,
  });
}

function dispatch(targetImpl: any, eventImpl: any, targetOverride?: any) {
  let clearTargets = false;
  let activationTarget = null;

  eventImpl[_dispatched] = true;

  targetOverride = targetOverride ?? targetImpl;
  const eventRelatedTarget = Reflect.has(eventImpl, "relatedTarget")
    ? eventImpl.relatedTarget
    : null;
  let relatedTarget = retarget(eventRelatedTarget, targetImpl);

  if (targetImpl !== relatedTarget || targetImpl === eventRelatedTarget) {
    const touchTargets = [];

    appendToEventPath(
      eventImpl,
      targetImpl,
      targetOverride,
      relatedTarget,
      touchTargets,
      false,
    );

    const isActivationEvent = eventImpl.type === "click";

    if (isActivationEvent && getHasActivationBehavior(targetImpl)) {
      activationTarget = targetImpl;
    }

    let slotInClosedTree = false;
    let slottable =
      isSlottable(targetImpl) && getAssignedSlot(targetImpl)
        ? targetImpl
        : null;
    let parent = getParent(targetImpl);

    // Populate event path
    // https://dom.spec.whatwg.org/#event-path
    while (parent !== null) {
      if (slottable !== null) {
        slottable = null;

        const parentRoot = getRoot(parent);
        if (
          isShadowRoot(parentRoot) &&
          parentRoot &&
          getMode(parentRoot) === "closed"
        ) {
          slotInClosedTree = true;
        }
      }

      relatedTarget = retarget(eventRelatedTarget, parent);

      if (
        isNode(parent) &&
        isShadowInclusiveAncestor(getRoot(targetImpl), parent)
      ) {
        appendToEventPath(
          eventImpl,
          parent,
          null,
          relatedTarget,
          touchTargets,
          slotInClosedTree,
        );
      } else if (parent === relatedTarget) {
        parent = null;
      } else {
        targetImpl = parent;

        if (
          isActivationEvent &&
          activationTarget === null &&
          getHasActivationBehavior(targetImpl)
        ) {
          activationTarget = targetImpl;
        }

        appendToEventPath(
          eventImpl,
          parent,
          targetImpl,
          relatedTarget,
          touchTargets,
          slotInClosedTree,
        );
      }

      if (parent !== null) {
        parent = getParent(parent);
      }

      slotInClosedTree = false;
    }

    let clearTargetsTupleIndex = -1;
    const path = getPath(eventImpl);
    for (
      let i = path.length - 1;
      i >= 0 && clearTargetsTupleIndex === -1;
      i--
    ) {
      if (path[i].target !== null) {
        clearTargetsTupleIndex = i;
      }
    }
    const clearTargetsTuple = path[clearTargetsTupleIndex];

    clearTargets =
      (isNode(clearTargetsTuple.target) &&
        isShadowRoot(getRoot(clearTargetsTuple.target))) ||
      (isNode(clearTargetsTuple.relatedTarget) &&
        isShadowRoot(getRoot(clearTargetsTuple.relatedTarget)));

    setEventPhase(eventImpl, Event.CAPTURING_PHASE);

    for (let i = path.length - 1; i >= 0; --i) {
      const tuple = path[i];

      if (tuple.target === null) {
        invokeEventListeners(tuple, eventImpl);
      }
    }

    for (let i = 0; i < path.length; i++) {
      const tuple = path[i];

      if (tuple.target !== null) {
        setEventPhase(eventImpl, Event.AT_TARGET);
      } else {
        setEventPhase(eventImpl, Event.BUBBLING_PHASE);
      }

      if (
        (eventImpl.eventPhase === Event.BUBBLING_PHASE && eventImpl.bubbles) ||
        eventImpl.eventPhase === Event.AT_TARGET
      ) {
        invokeEventListeners(tuple, eventImpl);
      }
    }
  }

  setEventPhase(eventImpl, Event.NONE);
  setCurrentTarget(eventImpl, null);
  setPath(eventImpl, []);
  setDispatched(eventImpl, false);
  eventImpl.cancelBubble = false;
  setStopImmediatePropagation(eventImpl, false);

  if (clearTargets) {
    setTarget(eventImpl, null);
    setRelatedTarget(eventImpl, null);
  }

  // TODO(bartlomieju): invoke activation targets if HTML nodes will be implemented
  // if (activationTarget !== null) {
  //   if (!eventImpl.defaultPrevented) {
  //     activationTarget._activationBehavior();
  //   }
  // }

  return !eventImpl.defaultPrevented;
}

/** Inner invoking of the event listeners where the resolved listeners are
 * called.
 *
 * Ref: https://dom.spec.whatwg.org/#concept-event-listener-inner-invoke */
function innerInvokeEventListeners(eventImpl, targetListeners) {
  let found = false;

  const { type } = eventImpl;

  if (!targetListeners || !targetListeners[type]) {
    return found;
  }

  let handlers = targetListeners[type];
  const handlersLength = handlers.length;

  // Copy event listeners before iterating since the list can be modified during the iteration.
  if (handlersLength > 1) {
    handlers = targetListeners[type].slice();
  }

  for (let i = 0; i < handlersLength; i++) {
    const listener = handlers[i];

    let capture, once, passive;
    if (typeof listener.options === "boolean") {
      capture = listener.options;
      once = false;
      passive = false;
    } else {
      capture = listener.options.capture;
      once = listener.options.once;
      passive = listener.options.passive;
    }

    // Check if the event listener has been removed since the listeners has been cloned.
    if (!targetListeners[type].includes(listener)) {
      continue;
    }

    found = true;

    if (
      (eventImpl.eventPhase === Event.CAPTURING_PHASE && !capture) ||
      (eventImpl.eventPhase === Event.BUBBLING_PHASE && capture)
    ) {
      continue;
    }

    if (once) {
      targetListeners[type].splice(targetListeners[type].indexOf(listener), 1);
    }

    if (passive) {
      setInPassiveListener(eventImpl, true);
    }

    if (typeof listener.callback === "object") {
      if (typeof listener.callback.handleEvent === "function") {
        listener.callback.handleEvent(eventImpl);
      }
    } else {
      listener.callback.call(eventImpl.currentTarget, eventImpl);
    }

    setInPassiveListener(eventImpl, false);

    if (getStopImmediatePropagation(eventImpl)) {
      return found;
    }
  }

  return found;
}

/** Invokes the listeners on a given event path with the supplied event.
 *
 * Ref: https://dom.spec.whatwg.org/#concept-event-listener-invoke */
function invokeEventListeners(tuple, eventImpl) {
  const path = getPath(eventImpl);
  if (path.length === 1) {
    const t = path[0];
    if (t.target) {
      setTarget(eventImpl, t.target);
    }
  } else {
    const tupleIndex = path.indexOf(tuple);
    for (let i = tupleIndex; i >= 0; i--) {
      const t = path[i];
      if (t.target) {
        setTarget(eventImpl, t.target);
        break;
      }
    }
  }

  setRelatedTarget(eventImpl, tuple.relatedTarget);

  if (eventImpl.cancelBubble) {
    return;
  }

  setCurrentTarget(eventImpl, tuple.item);

  innerInvokeEventListeners(eventImpl, getListeners(tuple.item));
}

export class EventTarget {
  constructor() {
    this[eventTargetData] = getDefaultTargetData();
  }

  addEventListener(
    type: string,
    callback: EventListenerOrEventListenerObject | null,
    options?: boolean | AddEventListenerOptions | undefined,
  ): void {
    const self = this ?? globalThis;
    const prefix = "Failed to execute 'addEventListener' on 'EventTarget'";

    requiredArguments(arguments.length, 2, prefix);

    let normalizedOptions;
    if (
      typeof options !== "object" ||
      options === null ||
      options === undefined
    ) {
      normalizedOptions = { capture: !!options };
    } else {
      normalizedOptions = {
        capture: false,
        passive: false,
        once: false,
        ...options,
      };
    }

    if (callback === null) {
      return;
    }

    const { listeners } = self[eventTargetData];

    if (!Reflect.has(listeners, type)) {
      listeners[type] = [];
    }

    const listenerList = listeners[type];
    for (let i = 0; i < listenerList.length; ++i) {
      const listener = listenerList[i];
      if (
        ((typeof listener.options === "boolean" &&
          listener.options === normalizedOptions.capture) ||
          (typeof listener.options === "object" &&
            listener.options.capture === normalizedOptions.capture)) &&
        listener.callback === callback
      ) {
        return;
      }
    }
    if (normalizedOptions.signal) {
      const signal = normalizedOptions.signal;
      if (signal.aborted) {
        // If signal is not null and its aborted flag is set, then return.
        return;
      } else {
        // If listener's signal is not null, then add the following abort
        // abort steps to it: Remove an event listener.
        signal.addEventListener("abort", () => {
          self.removeEventListener(type, callback, normalizedOptions);
        });
      }
    }

    listeners[type].push({ callback, options: normalizedOptions });
  }

  removeEventListener(
    type: string,
    callback: EventListenerOrEventListenerObject | null,
    options?: boolean | EventListenerOptions | undefined,
  ): void {
    const self = this ?? globalThis;
    requiredArguments(
      arguments.length,
      2,
      "Failed to execute 'removeEventListener' on 'EventTarget'",
    );

    const { listeners } = self[eventTargetData];
    if (callback !== null && Reflect.has(listeners, type)) {
      listeners[type] = listeners[type].filter(
        (listener) => listener.callback !== callback,
      );
    } else if (callback === null || !listeners[type]) {
      return;
    }

    let normalizedOptions;
    if (typeof options !== "object" || options === null) {
      normalizedOptions = { capture: !!options };
    } else {
      normalizedOptions = {
        capture: false,
        passive: false,
        once: false,
        ...options,
      };
    }

    for (let i = 0; i < listeners[type].length; ++i) {
      const listener = listeners[type][i];
      if (
        ((typeof listener.options === "boolean" &&
          listener.options === normalizedOptions.capture) ||
          (typeof listener.options === "object" &&
            listener.options.capture === normalizedOptions.capture)) &&
        listener.callback === callback
      ) {
        listeners[type].splice(i, 1);
        break;
      }
    }
  }

  dispatchEvent(event) {
    const self = this ?? globalThis;
    requiredArguments(
      arguments.length,
      1,
      "Failed to execute 'dispatchEvent' on 'EventTarget'",
    );

    // This is an optimization to avoid creating an event listener
    // on each startup.
    // Stores the flag for checking whether unload is dispatched or not.
    // This prevents the recursive dispatches of unload events.
    // See https://github.com/denoland/deno/issues/9201.
    // if (event.type === "unload" && self === globalThis_) {
    //   globalThis_[SymbolFor("Deno.isUnloadDispatched")] = true;
    // }

    const { listeners } = self[eventTargetData];
    if (!Reflect.has(listeners, event.type)) {
      event[_attributes].target = this;
      return true;
    }

    if (event[_dispatched]) {
      throw new DOMException("Invalid event state.", "InvalidStateError");
    }

    if (event.eventPhase !== Event.NONE) {
      throw new DOMException("Invalid event state.", "InvalidStateError");
    }

    return dispatch(self, event);
  }

  getParent(_event) {
    return null;
  }
}

Object.defineProperty(EventTarget.prototype, Symbol.toStringTag, {
  value: "EventTarget",
  enumerable: false,
  writable: false,
  configurable: true,
});

class Window extends EventTarget {}

Object.defineProperty(Window.prototype, Symbol.toStringTag, {
  value: "Window",
  enumerable: false,
  writable: false,
  configurable: true,
});

export function setupEvent(global) {
  setEventTargetData(global);
  Object.setPrototypeOf(global, Window.prototype);
  global.Event = Event;
  global.EventTarget = EventTarget;
}
