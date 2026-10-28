import {
  ConvexError,
  convexToJson,
  GenericValidator,
  jsonToConvex,
  v,
  Validator,
  Value,
} from "../../values/index.js";
import { GenericDataModel } from "../data_model.js";
import {
  ActionBuilder,
  DefaultFunctionArgs,
  FunctionVisibility,
  GenericActionCtx,
  GenericMutationCtx,
  GenericQueryCtx,
  MutationBuilder,
  PublicHttpAction,
  QueryBuilder,
  RegisteredAction,
  RegisteredMutation,
  RegisteredQuery,
} from "../registration.js";
import { setupActionCalls } from "./actions_impl.js";
import { setupActionVectorSearch } from "./vector_search_impl.js";
import { setupAuth, setupMutationAuth } from "./authentication_impl.js";
import { setupReader, setupWriter } from "./database_impl.js";
import { QueryImpl, QueryInitializerImpl } from "./query_impl.js";
import {
  setupActionScheduler,
  setupMutationScheduler,
} from "./scheduler_impl.js";
import {
  setupStorageActionWriter,
  setupStorageReader,
  setupStorageWriter,
} from "./storage_impl.js";
import { parseArgs } from "../../common/index.js";
import { performAsyncSyscall } from "./syscall.js";
import { asObjectValidator } from "../../values/validator.js";
import { getFunctionAddress } from "../components/paths.js";
import {
  setupQueryMeta,
  setupMutationMeta,
  setupActionMeta,
} from "./meta_impl.js";

async function invokeMutation<
  F extends (ctx: GenericMutationCtx<GenericDataModel>, ...args: any) => any,
>(func: F, argsStr: string, visibility: FunctionVisibility) {
  // TODO(presley): Change the function signature and propagate the requestId from Rust.
  // Ok, to mock it out for now, since queries are only running in V8.
  const requestId = "";
  const args = jsonToConvex(JSON.parse(argsStr));
  const mutationCtx = {
    db: setupWriter(),
    auth: setupMutationAuth(requestId),
    storage: setupStorageWriter(requestId),
    scheduler: setupMutationScheduler(),
    meta: setupMutationMeta(visibility),

    runQuery: (reference: any, args?: any, options?: any) =>
      options?.useStaleSnapshot
        ? runUdf("snapshotQuery", reference, args, options?.transactionLimits)
        : runUdf("query", reference, args, options?.transactionLimits),
    runMutation: (reference: any, args?: any, options?: any) =>
      runUdf("mutation", reference, args, options?.transactionLimits),
  };
  const result = await invokeFunction(func, mutationCtx, args as any);
  validateReturnValue(result);
  return JSON.stringify(convexToJson(result === undefined ? null : result));
}

export function validateReturnValue(v: any) {
  if (v instanceof QueryInitializerImpl || v instanceof QueryImpl) {
    throw new Error(
      "Return value is a Query. Results must be retrieved with `.collect()`, `.take(n), `.unique()`, or `.first()`.",
    );
  }
}

export async function invokeFunction<
  Ctx,
  Args extends any[],
  F extends (ctx: Ctx, ...args: Args) => any,
>(func: F, ctx: Ctx, args: Args) {
  let result;
  try {
    result = await Promise.resolve(func(ctx, ...args));
  } catch (thrown: unknown) {
    throw serializeConvexErrorData(thrown);
  }
  return result;
}

function dontCallDirectly(
  funcType: string,
  handler: (ctx: any, args: any) => any,
): unknown {
  return (ctx: any, args: any) => {
    globalThis.console.warn(
      "Convex functions should not directly call other Convex functions. Consider calling a helper function instead. " +
        `e.g. \`export const foo = ${funcType}(...); await foo(ctx);\` is not supported. ` +
        "See https://docs.convex.dev/production/best-practices/#use-helper-functions-to-write-shared-code",
    );
    return handler(ctx, args);
  };
}

// Keep in sync with node executor
function serializeConvexErrorData(thrown: unknown) {
  if (
    typeof thrown === "object" &&
    thrown !== null &&
    Symbol.for("ConvexError") in thrown
  ) {
    const error = thrown as ConvexError<any>;
    error.data = JSON.stringify(
      convexToJson(error.data === undefined ? null : error.data),
    );
    (error as any).ConvexErrorSymbol = Symbol.for("ConvexError");
    return error;
  } else {
    return thrown;
  }
}

/**
 * Guard against Convex functions accidentally getting included in a browser bundle.
 * Convex functions may include secret logic or credentials that should not be
 * send to untrusted clients (browsers).
 */
function assertNotBrowser() {
  if (
    typeof window === "undefined" ||
    (window as any).__convexAllowFunctionsInBrowser
  ) {
    return;
  }
  // JSDom doesn't count, developers are allowed to use JSDom in Convex functions.
  const isRealBrowser =
    Object.getOwnPropertyDescriptor(globalThis, "window")
      ?.get?.toString()
      .includes("[native code]") ?? false;
  if (isRealBrowser) {
    // eslint-disable-next-line no-console
    console.error(
      "Convex functions should not be imported in the browser. This will throw an error in future versions of `convex`. If this is a false negative, please report it to Convex support.",
    );
  }
}

type FunctionDefinition =
  | ((ctx: any, args: DefaultFunctionArgs) => any)
  | {
      args?: GenericValidator | Record<string, GenericValidator>;
      returns?: GenericValidator | Record<string, GenericValidator>;
      handler: (ctx: any, args: DefaultFunctionArgs) => any;
    };

function strictReplacer(key: string, value: any) {
  if (value === undefined) {
    throw new Error(
      `A validator is undefined for field "${key}". ` +
        `This is often caused by circular imports. ` +
        `See https://docs.convex.dev/error#undefined-validator for details.`,
    );
  }
  return value;
}
function exportArgs(functionDefinition: FunctionDefinition) {
  return () => {
    let args: GenericValidator = v.any();
    if (
      typeof functionDefinition === "object" &&
      functionDefinition.args !== undefined
    ) {
      args = asObjectValidator(functionDefinition.args);
    }
    return JSON.stringify(args.json, strictReplacer);
  };
}

function exportReturns(functionDefinition: FunctionDefinition) {
  return () => {
    let returns: Validator<any, any, any> | undefined;
    if (
      typeof functionDefinition === "object" &&
      functionDefinition.returns !== undefined
    ) {
      returns = asObjectValidator(functionDefinition.returns);
    }
    return JSON.stringify(returns ? returns.json : null, strictReplacer);
  };
}

/**
 * Define a mutation in this Convex app's public API.
 *
 * You should generally use the `mutation` function from
 * `"./_generated/server"`.
 *
 * Mutations can read from and write to the database, and are accessible from
 * the client. They run **transactionally**, all database reads and writes
 * within a single mutation are atomic and isolated from other mutations.
 *
 * @example
 * ```typescript
 * import { mutation } from "./_generated/server";
 * import { v } from "convex/values";
 *
 * export const createTask = mutation({
 *   args: { text: v.string() },
 *   returns: v.id("tasks"),
 *   handler: async (ctx, args) => {
 *     const taskId = await ctx.db.insert("tasks", {
 *       text: args.text,
 *       completed: false,
 *     });
 *     return taskId;
 *   },
 * });
 * ```
 *
 * **Best practice:** Always include `args` and `returns` validators on all
 * mutations. If the function doesn't return a value, use `returns: v.null()`.
 * Argument validation is critical for security since public mutations are
 * exposed to the internet.
 *
 * **Common mistake:** Mutations cannot call third-party APIs or use `fetch`.
 * They must be deterministic. Use actions for external API calls.
 *
 * **Common mistake:** Do not use `mutation` for sensitive internal functions
 * that should not be called by clients. Use `internalMutation` instead.
 *
 * @param func - The mutation function. It receives a {@link GenericMutationCtx} as its first argument.
 * @returns The wrapped mutation. Include this as an `export` to name it and make it accessible.
 *
 * @see https://docs.convex.dev/functions/mutation-functions
 * @public
 */
export const mutationGeneric: MutationBuilder<any, "public"> = ((
  functionDefinition: FunctionDefinition,
) => {
  const handler = (
    typeof functionDefinition === "function"
      ? functionDefinition
      : functionDefinition.handler
  ) as (ctx: GenericMutationCtx<any>, args: any) => any;
  const func = dontCallDirectly("mutation", handler) as RegisteredMutation<
    "public",
    any,
    any
  >;

  assertNotBrowser();
  func.isMutation = true;
  func.isPublic = true;
  func.invokeMutation = (argsStr) => invokeMutation(handler, argsStr, "public");
  func.exportArgs = exportArgs(functionDefinition);
  func.exportReturns = exportReturns(functionDefinition);
  func._handler = handler;
  return func;
}) as MutationBuilder<any, "public">;

/**
 * Define a mutation that is only accessible from other Convex functions (but not from the client).
 *
 * You should generally use the `internalMutation` function from
 * `"./_generated/server"`.
 *
 * Internal mutations can read from and write to the database but are **not**
 * exposed as part of your app's public API. They can only be called by other
 * Convex functions using `ctx.runMutation` or by the scheduler. Like public
 * mutations, they run transactionally.
 *
 * @example
 * ```typescript
 * import { internalMutation } from "./_generated/server";
 * import { v } from "convex/values";
 *
 * // This mutation can only be called from other Convex functions:
 * export const markTaskCompleted = internalMutation({
 *   args: { taskId: v.id("tasks") },
 *   returns: v.null(),
 *   handler: async (ctx, args) => {
 *     await ctx.db.patch("tasks", args.taskId, { completed: true });
 *     return null;
 *   },
 * });
 * ```
 *
 * **Best practice:** Use `internalMutation` for any mutation that should not
 * be directly callable by clients, such as write-back functions from actions
 * or scheduled background work. Reference it via the `internal` object:
 * `await ctx.runMutation(internal.myModule.markTaskCompleted, { taskId })`.
 *
 * @param func - The mutation function. It receives a {@link GenericMutationCtx} as its first argument.
 * @returns The wrapped mutation. Include this as an `export` to name it and make it accessible.
 *
 * @see https://docs.convex.dev/functions/internal-functions
 * @public
 */
export const internalMutationGeneric: MutationBuilder<any, "internal"> = ((
  functionDefinition: FunctionDefinition,
) => {
  const handler = (
    typeof functionDefinition === "function"
      ? functionDefinition
      : functionDefinition.handler
  ) as (ctx: GenericMutationCtx<any>, args: any) => any;
  const func = dontCallDirectly(
    "internalMutation",
    handler,
  ) as RegisteredMutation<"internal", any, any>;

  assertNotBrowser();
  func.isMutation = true;
  func.isInternal = true;
  func.invokeMutation = (argsStr) =>
    invokeMutation(handler, argsStr, "internal");
  func.exportArgs = exportArgs(functionDefinition);
  func.exportReturns = exportReturns(functionDefinition);
  func._handler = handler;
  return func;
}) as MutationBuilder<any, "internal">;

async function invokeQuery<
  F extends (ctx: GenericQueryCtx<GenericDataModel>, ...args: any) => any,
>(func: F, argsStr: string, visibility: FunctionVisibility) {
  // TODO(presley): Change the function signature and propagate the requestId from Rust.
  // Ok, to mock it out for now, since queries are only running in V8.
  const requestId = "";
  const args = jsonToConvex(JSON.parse(argsStr));
  const queryCtx = {
    db: setupReader(),
    auth: setupAuth(requestId),
    storage: setupStorageReader(requestId),
    meta: setupQueryMeta(visibility),
    runQuery: (reference: any, args?: any, options?: any) => {
      if (options?.useStaleSnapshot) {
        throw new Error(
          "`useStaleSnapshot` is only supported in mutations, not queries.",
        );
      }
      return runUdf("query", reference, args, options?.transactionLimits);
    },
  };
  const result = await invokeFunction(func, queryCtx, args as any);
  validateReturnValue(result);
  return JSON.stringify(convexToJson(result === undefined ? null : result));
}

/**
 * Define a query in this Convex app's public API.
 *
 * You should generally use the `query` function from
 * `"./_generated/server"`.
 *
 * Queries can read from the database and are accessible from the client. They
 * are **reactive**, when used with `useQuery` in React, the component
 * automatically re-renders whenever the underlying data changes. Queries
 * cannot modify the database.
 * Query results are automatically cached by the Convex client and kept
 * consistent via WebSocket subscriptions.
 *
 *
 * @example
 * ```typescript
 * import { query } from "./_generated/server";
 * import { v } from "convex/values";
 *
 * export const listTasks = query({
 *   args: { completed: v.optional(v.boolean()) },
 *   returns: v.array(v.object({
 *     _id: v.id("tasks"),
 *     _creationTime: v.number(),
 *     text: v.string(),
 *     completed: v.boolean(),
 *   })),
 *   handler: async (ctx, args) => {
 *     if (args.completed !== undefined) {
 *       return await ctx.db
 *         .query("tasks")
 *         .withIndex("by_completed", (q) => q.eq("completed", args.completed))
 *         .collect();
 *     }
 *     return await ctx.db.query("tasks").collect();
 *   },
 * });
 * ```
 *
 * **Best practice:** Always include `args` and `returns` validators. Use
 * `.withIndex()` instead of `.filter()` for efficient database queries.
 * Queries should be fast since they run on every relevant data change.
 *
 * **Common mistake:** Queries are pure reads, they cannot write to the
 * database, call external APIs, or schedule functions. Use actions for HTTP
 * calls and mutations for database writes and scheduling.
 *
 * @param func - The query function. It receives a {@link GenericQueryCtx} as its first argument.
 * @returns The wrapped query. Include this as an `export` to name it and make it accessible.
 *
 * @see https://docs.convex.dev/functions/query-functions
 * @public
 */
export const queryGeneric: QueryBuilder<any, "public"> = ((
  functionDefinition: FunctionDefinition,
) => {
  const handler = (
    typeof functionDefinition === "function"
      ? functionDefinition
      : functionDefinition.handler
  ) as (ctx: GenericQueryCtx<any>, args: any) => any;
  const func = dontCallDirectly("query", handler) as RegisteredQuery<
    "public",
    any,
    any
  >;

  assertNotBrowser();
  func.isQuery = true;
  func.isPublic = true;
  func.invokeQuery = (argsStr) => invokeQuery(handler, argsStr, "public");
  func.exportArgs = exportArgs(functionDefinition);
  func.exportReturns = exportReturns(functionDefinition);
  func._handler = handler;
  return func;
}) as QueryBuilder<any, "public">;

/**
 * Define a query that is only accessible from other Convex functions (but not from the client).
 *
 * You should generally use the `internalQuery` function from
 * `"./_generated/server"`.
 *
 * Internal queries can read from the database but are **not** exposed as part
 * of your app's public API. They can only be called by other Convex functions
 * using `ctx.runQuery`. This is useful for loading data in actions or for
 * helper queries that shouldn't be client-facing.
 *
 * @example
 * ```typescript
 * import { internalQuery } from "./_generated/server";
 * import { v } from "convex/values";
 *
 * // Only callable from other Convex functions:
 * export const getUser = internalQuery({
 *   args: { userId: v.id("users") },
 *   returns: v.union(
 *     v.object({
 *       _id: v.id("users"),
 *       _creationTime: v.number(),
 *       name: v.string(),
 *       email: v.string(),
 *     }),
 *     v.null(),
 *   ),
 *   handler: async (ctx, args) => {
 *     return await ctx.db.get("users", args.userId);
 *   },
 * });
 * ```
 *
 * **Best practice:** Use `internalQuery` for data-loading in actions via
 * `ctx.runQuery(internal.myModule.getUser, { userId })`.
 *
 * @param func - The query function. It receives a {@link GenericQueryCtx} as its first argument.
 * @returns The wrapped query. Include this as an `export` to name it and make it accessible.
 *
 * @see https://docs.convex.dev/functions/internal-functions
 * @public
 */
export const internalQueryGeneric: QueryBuilder<any, "internal"> = ((
  functionDefinition: FunctionDefinition,
) => {
  const handler = (
    typeof functionDefinition === "function"
      ? functionDefinition
      : functionDefinition.handler
  ) as (ctx: GenericQueryCtx<any>, args: any) => any;
  const func = dontCallDirectly("internalQuery", handler) as RegisteredQuery<
    "internal",
    any,
    any
  >;

  assertNotBrowser();
  func.isQuery = true;
  func.isInternal = true;
  func.invokeQuery = (argsStr) =>
    invokeQuery(handler as any, argsStr, "internal");
  func.exportArgs = exportArgs(functionDefinition);
  func.exportReturns = exportReturns(functionDefinition);
  func._handler = handler;
  return func;
}) as QueryBuilder<any, "internal">;

async function invokeAction<
  F extends (ctx: GenericActionCtx<GenericDataModel>, ...args: any) => any,
>(func: F, requestId: string, argsStr: string, visibility: FunctionVisibility) {
  const args = jsonToConvex(JSON.parse(argsStr));
  const calls = setupActionCalls(requestId);
  const ctx = {
    ...calls,
    auth: setupAuth(requestId),
    scheduler: setupActionScheduler(requestId),
    storage: setupStorageActionWriter(requestId),
    vectorSearch: setupActionVectorSearch(requestId) as any,
    meta: setupActionMeta(visibility),
  };
  const result = await invokeFunction(func, ctx, args as any);
  return JSON.stringify(convexToJson(result === undefined ? null : result));
}

/**
 * Define an action in this Convex app's public API.
 *
 * Actions can call third-party APIs, use Node.js libraries, and perform other
 * side effects. Unlike queries and mutations, actions do **not** have direct
 * database access (`ctx.db` is not available). Instead, use `ctx.runQuery`
 * and `ctx.runMutation` to read and write data.
 *
 * You should generally use the `action` function from
 * `"./_generated/server"`.
 *
 * Actions are accessible from the client and run outside of the database
 * transaction, so they are not atomic. They are best for integrating with
 * external services.
 *
 * @example
 * ```typescript
 * // Add "use node"; at the top of the file if using Node.js built-in modules.
 * import { action } from "./_generated/server";
 * import { v } from "convex/values";
 * import { internal } from "./_generated/api";
 *
 * export const generateSummary = action({
 *   args: { text: v.string() },
 *   returns: v.string(),
 *   handler: async (ctx, args) => {
 *     // Call an external API:
 *     const response = await fetch("https://api.example.com/summarize", {
 *       method: "POST",
 *       body: JSON.stringify({ text: args.text }),
 *     });
 *     const { summary } = await response.json();
 *
 *     // Write results back via a mutation:
 *     await ctx.runMutation(internal.myModule.saveSummary, {
 *       text: args.text,
 *       summary,
 *     });
 *
 *     return summary;
 *   },
 * });
 * ```
 *
 * **Best practice:** Minimize the number of `ctx.runQuery` and
 * `ctx.runMutation` calls from actions. Each call is a separate transaction,
 * so splitting logic across multiple calls introduces the risk of race
 * conditions. Try to batch reads/writes into single query/mutation calls.
 *
 * **`"use node"` runtime:** Actions run in Convex's default JavaScript
 * runtime, which supports `fetch` and most NPM packages. Only add
 * `"use node";` at the top of the file if a third-party library specifically
 * requires Node.js built-in APIs, it is a last resort, not the default.
 * Node.js actions have slower cold starts, and **only actions can be defined
 * in `"use node"` files** (no queries or mutations), so prefer the default
 * runtime whenever possible.
 *
 * **Common mistake:** Do not try to access `ctx.db` in an action, it is
 * not available. Use `ctx.runQuery` and `ctx.runMutation` instead.
 *
 * @param func - The function. It receives a {@link GenericActionCtx} as its first argument.
 * @returns The wrapped function. Include this as an `export` to name it and make it accessible.
 *
 * @see https://docs.convex.dev/functions/actions
 * @public
 */
export const actionGeneric: ActionBuilder<any, "public"> = ((
  functionDefinition: FunctionDefinition,
) => {
  const handler = (
    typeof functionDefinition === "function"
      ? functionDefinition
      : functionDefinition.handler
  ) as (ctx: GenericActionCtx<any>, args: any) => any;
  const func = dontCallDirectly("action", handler) as RegisteredAction<
    "public",
    any,
    any
  >;

  assertNotBrowser();
  func.isAction = true;
  func.isPublic = true;
  func.invokeAction = (requestId, argsStr) =>
    invokeAction(handler, requestId, argsStr, "public");
  func.exportArgs = exportArgs(functionDefinition);
  func.exportReturns = exportReturns(functionDefinition);
  func._handler = handler;
  return func;
}) as ActionBuilder<any, "public">;

/**
 * Define an action that is only accessible from other Convex functions (but not from the client).
 *
 * You should generally use the `internalAction` function from
 * `"./_generated/server"`.
 *
 * Internal actions behave like public actions (they can call external APIs and
 * use Node.js libraries) but are **not** exposed in your app's public API. They
 * can only be called by other Convex functions using `ctx.runAction` or via the
 * scheduler.
 *
 * @example
 * ```typescript
 * import { internalAction } from "./_generated/server";
 * import { v } from "convex/values";
 *
 * export const sendEmail = internalAction({
 *   args: { to: v.string(), subject: v.string(), body: v.string() },
 *   returns: v.null(),
 *   handler: async (ctx, args) => {
 *     // Call an external email service (fetch works in the default runtime):
 *     await fetch("https://api.email-service.com/send", {
 *       method: "POST",
 *       headers: { "Content-Type": "application/json" },
 *       body: JSON.stringify(args),
 *     });
 *     return null;
 *   },
 * });
 * ```
 *
 * **Best practice:** Use `internalAction` for background work scheduled from
 * mutations: `await ctx.scheduler.runAfter(0, internal.myModule.sendEmail, { ... })`.
 * Only use `ctx.runAction` from another action if you need to cross runtimes
 * (e.g., default Convex runtime to Node.js). Otherwise, extract shared code
 * into a helper function.
 *
 * **`"use node"` runtime:** Only add `"use node";` at the top of the file
 * as a last resort when a third-party library requires Node.js APIs. Node.js
 * actions have slower cold starts, and **only actions can be defined in
 * `"use node"` files** (no queries or mutations).
 *
 * @param func - The function. It receives a {@link GenericActionCtx} as its first argument.
 * @returns The wrapped function. Include this as an `export` to name it and make it accessible.
 *
 * @see https://docs.convex.dev/functions/internal-functions
 * @public
 */
export const internalActionGeneric: ActionBuilder<any, "internal"> = ((
  functionDefinition: FunctionDefinition,
) => {
  const handler = (
    typeof functionDefinition === "function"
      ? functionDefinition
      : functionDefinition.handler
  ) as (ctx: GenericActionCtx<any>, args: any) => any;
  const func = dontCallDirectly("internalAction", handler) as RegisteredAction<
    "internal",
    any,
    any
  >;

  assertNotBrowser();
  func.isAction = true;
  func.isInternal = true;
  func.invokeAction = (requestId, argsStr) =>
    invokeAction(handler, requestId, argsStr, "internal");
  func.exportArgs = exportArgs(functionDefinition);
  func.exportReturns = exportReturns(functionDefinition);
  func._handler = handler;
  return func;
}) as ActionBuilder<any, "internal">;

async function invokeHttpAction<
  F extends (ctx: GenericActionCtx<GenericDataModel>, request: Request) => any,
>(func: F, request: Request) {
  // TODO(presley): Change the function signature and propagate the requestId from Rust.
  // Ok, to mock it out for now, since http endpoints are only running in V8.
  const requestId = "";
  const calls = setupActionCalls(requestId);
  const ctx = {
    ...calls,
    auth: setupAuth(requestId),
    storage: setupStorageActionWriter(requestId),
    scheduler: setupActionScheduler(requestId),
    vectorSearch: setupActionVectorSearch(requestId) as any,
    meta: setupActionMeta("public"),
  };
  return await invokeFunction(func, ctx, [request]);
}

/**
 * Define a Convex HTTP action.
 *
 * HTTP actions handle raw HTTP requests and return HTTP responses. They are
 * registered by routing URL paths to them in `convex/http.ts` using
 * {@link HttpRouter}. Like regular actions, they can call external APIs and
 * use `ctx.runQuery` / `ctx.runMutation` but do not have direct `ctx.db` access.
 *
 * @example
 * ```typescript
 * // convex/http.ts
 * import { httpRouter } from "convex/server";
 * import { httpAction } from "./_generated/server";
 *
 * const http = httpRouter();
 *
 * http.route({
 *   path: "/api/webhook",
 *   method: "POST",
 *   handler: httpAction(async (ctx, request) => {
 *     const body = await request.json();
 *     // Process the webhook payload...
 *     return new Response(JSON.stringify({ ok: true }), {
 *       status: 200,
 *       headers: { "Content-Type": "application/json" },
 *     });
 *   }),
 * });
 *
 * export default http;
 * ```
 *
 * **Best practice:** HTTP actions are registered at the exact path specified.
 * For example, `path: "/api/webhook"` registers at `/api/webhook`.
 *
 * @param func - The function. It receives a {@link GenericActionCtx} as its first argument, and a `Request` object
 * as its second.
 * @returns The wrapped function. Route a URL path to this function in `convex/http.ts`.
 *
 * @see https://docs.convex.dev/functions/http-actions
 * @public
 */
export const httpActionGeneric = (
  func: (
    ctx: GenericActionCtx<GenericDataModel>,
    request: Request,
  ) => Promise<Response>,
): PublicHttpAction => {
  const q = dontCallDirectly("httpAction", func) as PublicHttpAction;
  assertNotBrowser();
  q.isHttp = true;
  q.invokeHttpAction = (request) => invokeHttpAction(func as any, request);
  q._handler = func;
  return q;
};

async function runUdf(
  udfType: "query" | "mutation" | "snapshotQuery",
  f: any,
  args?: Record<string, Value>,
  transactionLimits?: Record<string, number>,
): Promise<any> {
  const queryArgs = parseArgs(args);
  const syscallArgs: Record<string, any> = {
    udfType,
    args: convexToJson(queryArgs),
    ...getFunctionAddress(f),
  };
  if (transactionLimits) {
    syscallArgs.transactionLimits = transactionLimits;
  }
  const result = await performAsyncSyscall("1.0/runUdf", syscallArgs);
  return jsonToConvex(result);
}
