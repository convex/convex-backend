/* eslint-disable */
/**
 * Generated `api` utility.
 *
 * THIS CODE IS AUTOMATICALLY GENERATED.
 *
 * To regenerate, run `npx convex dev`.
 * @module
 */

import type * as components_ from "../components.js";
import type * as crons from "../crons.js";
import type * as http from "../http.js";
import type * as messages from "../messages.js";
import type * as notes from "../notes.js";
import type * as triggers from "../triggers.js";
import type * as types from "../types.js";
import type * as withnode from "../withnode.js";

import type {
  ApiFromModules,
  FilterApi,
  FunctionReference,
} from "convex/server";

declare const fullApi: ApiFromModules<{
  components: typeof components_;
  crons: typeof crons;
  http: typeof http;
  messages: typeof messages;
  notes: typeof notes;
  triggers: typeof triggers;
  types: typeof types;
  withnode: typeof withnode;
}>;

/**
 * A utility for referencing Convex functions in your app's public API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = api.myModule.myFunction;
 * ```
 */
export declare const api: FilterApi<
  typeof fullApi,
  FunctionReference<any, "public">
>;

/**
 * A utility for referencing Convex functions in your app's internal API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = internal.myModule.myFunction;
 * ```
 */
export declare const internal: FilterApi<
  typeof fullApi,
  FunctionReference<any, "internal">
>;

export declare const components: {
  triggers: import("@convex-dev/triggers/_generated/component.js").ComponentApi<"triggers">;
  waitlist: import("../../examples/waitlist@name-with-dashes/_generated/component.js").ComponentApi<"waitlist">;
  waitlist2: import("../../examples/waitlist@name-with-dashes/_generated/component.js").ComponentApi<"waitlist2">;
  ratelimiter: import("@convex-dev/ratelimiter/_generated/component.js").ComponentApi<"ratelimiter">;
};
