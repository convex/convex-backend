/* eslint-disable */
/**
 * Generated `ComponentApi` utility.
 *
 * THIS CODE IS AUTOMATICALLY GENERATED.
 *
 * To regenerate, run `npx convex dev`.
 * @module
 */

import type { FunctionReference } from "convex/server";

/**
 * A utility for referencing a Convex component's exposed API.
 *
 * Useful when expecting a parameter like `components.myComponent`.
 * Usage:
 * ```ts
 * async function myFunction(ctx: QueryCtx, component: ComponentApi) {
 *   return ctx.runQuery(component.someFile.someQuery, { ...args });
 * }
 * ```
 */
export type ComponentApi<Name extends string | undefined = string | undefined> =
  {
    lib: {
      addHook: FunctionReference<
        "mutation",
        "internal",
        {
          handle: import("convex/server").FunctionHandle_future<any, any, any>;
        },
        any,
        Name
      >;
      enqueue: FunctionReference<
        "mutation",
        "internal",
        {
          input: string;
          onComplete: import("convex/server").FunctionHandle_future<
            "mutation",
            { jobId: string; result: string },
            null
          >;
        },
        any,
        Name
      >;
      listHooks: FunctionReference<
        "query",
        "internal",
        {},
        Array<import("convex/server").FunctionHandle_future<any, any, any>>,
        Name
      >;
      runHooks: FunctionReference<
        "mutation",
        "internal",
        { event: string },
        any,
        Name
      >;
    };
  };
