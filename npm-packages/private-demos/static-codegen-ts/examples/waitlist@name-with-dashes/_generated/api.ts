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
import { anyApi, componentsGeneric } from "convex/server";

/**
 * A utility for referencing Convex functions in your app's public API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = api.myModule.myFunction;
 * ```
 */
export const api: {
  actionDemo: {
    demo: FunctionReference<"action", "public", any, any>;
  };
  index: {
    fileDownloadUrl: FunctionReference<
      "query",
      "public",
      { id: Id<"_storage"> },
      string
    >;
    fileUploadUrl: FunctionReference<"mutation", "public", {}, string>;
    getMessageCount: FunctionReference<"query", "public", {}, number>;
    latestWrite: FunctionReference<"query", "public", {}, string>;
    listFiles: FunctionReference<"query", "public", any, any>;
    readFromFile: FunctionReference<
      "action",
      "public",
      { id: Id<"_storage"> },
      string
    >;
    repeatMessage: FunctionReference<
      "action",
      "public",
      { message: string; n: number },
      string
    >;
    sayGoodbyeFromQuery: FunctionReference<"query", "public", {}, string>;
    sayHelloFromMutation: FunctionReference<"mutation", "public", {}, string>;
    scheduleMessage: FunctionReference<"mutation", "public", {}, any>;
    scheduleSend: FunctionReference<"mutation", "public", {}, any>;
    sendMessage: FunctionReference<"mutation", "public", {}, any>;
    storeInFile: FunctionReference<
      "action",
      "public",
      { message: string },
      Id<"_storage">
    >;
    writeSuccessfully: FunctionReference<
      "mutation",
      "public",
      { text: string },
      any
    >;
    writeThenFail: FunctionReference<
      "mutation",
      "public",
      { text: string },
      any
    >;
  };
} = anyApi as any;

/**
 * A utility for referencing Convex functions in your app's internal API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = internal.myModule.myFunction;
 * ```
 */
export const internal: {} = anyApi as any;

export const components = componentsGeneric() as unknown as {
  ratelimiter: import("@convex-dev/ratelimiter/_generated/component.js").ComponentApi<"ratelimiter">;
};
