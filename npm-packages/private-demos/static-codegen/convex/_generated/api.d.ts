/* eslint-disable */
/**
 * Generated `api` utility.
 *
 * THIS CODE IS AUTOMATICALLY GENERATED.
 *
 * To regenerate, run `npx convex dev`.
 * @module
 */

import type { FunctionReference } from "convex/server";
import type { GenericId as Id } from "convex/values";

/**
 * A utility for referencing Convex functions in your app's public API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = api.myModule.myFunction;
 * ```
 */
export declare const api: {
  messages: {
    componentTest: FunctionReference<"action", "public", {}, any>;
    list: FunctionReference<"query", "public", {}, any>;
    save: FunctionReference<"action", "public", { message: string }, string>;
    scheduleSendWaitlistMessage: FunctionReference<
      "mutation",
      "public",
      {},
      any
    >;
    send: FunctionReference<
      "mutation",
      "public",
      { author: string; body: string },
      any
    >;
    testPartialRollback: FunctionReference<"mutation", "public", {}, any>;
  };
  staticFunctions: {
    a: FunctionReference<
      "action",
      "public",
      { branded: string; id: Id<"empty"> },
      string
    >;
    m: FunctionReference<
      "mutation",
      "public",
      { branded: string },
      { _creationTime: number; _id: Id<"empty"> } | null
    >;
    q: FunctionReference<"query", "public", { branded: string }, string>;
  };
};

/**
 * A utility for referencing Convex functions in your app's internal API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = internal.myModule.myFunction;
 * ```
 */
export declare const internal: {
  staticFunctions: {
    ia: FunctionReference<"action", "internal", {}, any>;
    im: FunctionReference<"mutation", "internal", { branded: string }, null>;
    iq: FunctionReference<
      "query",
      "internal",
      {
        arr: Array<number>;
        bool: boolean;
        data: ArrayBuffer;
        id: Id<"empty">;
        literal: "literal";
        null: null;
        num: number;
        str: string;
      },
      | string
      | "literal"
      | number
      | boolean
      | ArrayBuffer
      | Array<number>
      | null
      | Id<"empty">
    >;
  };
};

export declare const components: {
  waitlist: import("../../examples/waitlist@name-with-dashes/_generated/component.js").ComponentApi<"waitlist">;
  waitlist2: import("../../examples/waitlist@name-with-dashes/_generated/component.js").ComponentApi<"waitlist2">;
  nestedComponent: import("../nested-component/_generated/component.js").ComponentApi<"nestedComponent">;
};
