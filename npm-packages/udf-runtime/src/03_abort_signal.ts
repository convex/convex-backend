import {
  AbortController,
  AbortSignal,
} from "abortcontroller-polyfill/dist/abortcontroller";

Object.defineProperty(AbortController.prototype, Symbol.toStringTag, {
  value: "AbortController",
  enumerable: false,
  writable: false,
  configurable: true,
});
Object.defineProperty(AbortSignal.prototype, Symbol.toStringTag, {
  value: "AbortSignal",
  enumerable: false,
  writable: false,
  configurable: true,
});

export const setupAbortSignal = (global) => {
  global.AbortController = AbortController;
  global.AbortSignal = AbortSignal;
};
