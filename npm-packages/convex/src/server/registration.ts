import {
  Auth,
  MutationAuth,
  GenericDatabaseReader,
  GenericDatabaseReaderWithTable,
  GenericDatabaseWriter,
  GenericDatabaseWriterWithTable,
  StorageActionWriter,
  StorageReader,
  StorageWriter,
} from "./index.js";
import {
  ActionMeta,
  MutationMeta,
  QueryMeta,
  TransactionLimits,
} from "./meta.js";
import {
  ArgsAndOptions,
  FunctionReturnType,
  OptionalRestArgs,
  ValidatorTypeToReturnType,
  FunctionReference,
  FunctionReference_future,
} from "../server/api.js";
import {
  GenericValidator,
  Infer,
  ObjectType,
  PropertyValidators,
} from "../values/validator.js";
import { Id } from "../values/value.js";
import {
  GenericDataModel,
  NamedTableInfo,
  TableNamesInDataModel,
  VectorIndexNames,
} from "./data_model.js";
import { Scheduler } from "./scheduler.js";
import { VectorSearchQuery } from "./vector_search.js";
import { Expand } from "../type_utils.js";
import { Validator } from "../values/validators.js";

/**
 * A set of services for use within Convex mutation functions.
 *
 * The mutation context is passed as the first argument to any Convex mutation
 * function run on the server. Mutations run **transactionally**, all reads
 * and writes within a single mutation are atomic and isolated.
 *
 * You should generally use the `MutationCtx` type from
 * `"./_generated/server"`.
 *
 * @example
 * ```typescript
 * import { mutation } from "./_generated/server";
 * import { internal } from "./_generated/api";
 * import { v } from "convex/values";
 *
 * export const createTask = mutation({
 *   args: { text: v.string() },
 *   returns: v.id("tasks"),
 *   handler: async (ctx, args) => {
 *     // ctx.db: read and write documents
 *     const taskId = await ctx.db.insert("tasks", { text: args.text, completed: false });
 *
 *     // ctx.auth: check the authenticated user
 *     const identity = await ctx.auth.getUserIdentity();
 *
 *     // ctx.scheduler: schedule functions for later
 *     await ctx.scheduler.runAfter(0, internal.notifications.send, { taskId });
 *
 *     return taskId;
 *   },
 * });
 * ```
 *
 * @public
 */
export interface GenericMutationCtx<DataModel extends GenericDataModel> {
  /**
   * A utility for reading and writing data in the database.
   *
   * Use `ctx.db.insert()`, `ctx.db.patch()`, `ctx.db.replace()`, and
   * `ctx.db.delete()` to write data. Use `ctx.db.get()` and `ctx.db.query()`
   * to read data. All operations within a mutation are atomic.
   */
  db: GenericDatabaseWriter<DataModel>;

  /**
   * Auth functionality for mutations.
   *
   * Call `await ctx.auth.getUserIdentity()` to get the current user's identity,
   * or `null` if the user is not authenticated.
   *
   * `ctx.auth.signJwt` is available for creating deployment issued JWTs.
   */
  auth: MutationAuth;

  /**
   * A utility for reading and writing files in storage.
   *
   * Use `ctx.storage.generateUploadUrl()` to create an upload URL for clients,
   * `ctx.storage.getUrl(storageId)` to get a URL for a stored file,
   * or `ctx.storage.delete(storageId)` to remove one.
   */
  storage: StorageWriter;

  /**
   * A utility for scheduling Convex functions to run in the future.
   *
   * @example
   * ```typescript
   * // Schedule an action to run immediately after this mutation commits:
   * await ctx.scheduler.runAfter(0, internal.emails.sendWelcome, { userId });
   *
   * // Schedule a cleanup to run in 24 hours:
   * await ctx.scheduler.runAfter(24 * 60 * 60 * 1000, internal.tasks.cleanup, {});
   * ```
   */
  scheduler: Scheduler;

  /**
   * Call a query function within the same transaction.
   *
   * The query runs within the same transaction as the calling mutation,
   * seeing a consistent snapshot of the database. Requires a
   * {@link FunctionReference} (e.g., `api.myModule.myQuery` or
   * `internal.myModule.myQuery`).
   *
   * NOTE: Often you can extract shared logic into a helper function instead.
   * `runQuery` incurs overhead of running argument and return value validation,
   * and creating a new isolated JS context.
   *
   * @example
   * ```typescript
   * const user = await ctx.runQuery(internal.users.getUser, { userId });
   * ```
   */
  runQuery: <
    Query extends
      | FunctionReference<"query", "public" | "internal">
      | FunctionReference_future<"query", "public" | "internal">,
  >(
    query: Query,
    ...args: ArgsAndOptions<Query, AdvancedRunQueryOptions>
  ) => Promise<FunctionReturnType<Query>>;

  /**
   * Call a mutation function within the same transaction.
   *
   * The mutation runs in a sub-transaction, so if it throws an error, all of
   * its writes will be rolled back. Requires a {@link FunctionReference}.
   *
   * NOTE: Often you can extract shared logic into a helper function instead.
   * `runMutation` incurs overhead of running argument and return value
   * validation, and creating a new isolated JS context.
   */
  runMutation: <
    Mutation extends
      | FunctionReference<"mutation", "public" | "internal">
      | FunctionReference_future<"mutation", "public" | "internal">,
  >(
    mutation: Mutation,
    ...args: ArgsAndOptions<Mutation, { transactionLimits?: TransactionLimits }>
  ) => Promise<FunctionReturnType<Mutation>>;

  meta: MutationMeta;
}

/**
 * A set of services for use within Convex mutation functions.
 *
 * The mutation context is passed as the first argument to any Convex mutation
 * function run on the server.
 *
 * You should generally use the `MutationCtx` type from
 * `"./_generated/server"`.
 *
 * @public
 */
export type GenericMutationCtxWithTable<DataModel extends GenericDataModel> =
  Omit<GenericMutationCtx<DataModel>, "db"> & {
    db: GenericDatabaseWriterWithTable<DataModel>;
  };

/**
 * A set of services for use within Convex query functions.
 *
 * The query context is passed as the first argument to any Convex query
 * function run on the server. Queries are **read-only**, they can read from
 * the database but cannot write. They are also **reactive**, when used with
 * `useQuery` on the client, the result automatically updates when data changes.
 *
 * You should generally use the `QueryCtx` type from
 * `"./_generated/server"`.
 *
 * @example
 * ```typescript
 * import { query } from "./_generated/server";
 * import { v } from "convex/values";
 *
 * export const listTasks = query({
 *   args: {},
 *   returns: v.array(v.object({
 *     _id: v.id("tasks"),
 *     _creationTime: v.number(),
 *     text: v.string(),
 *     completed: v.boolean(),
 *   })),
 *   handler: async (ctx, args) => {
 *     // ctx.db: read-only database access
 *     return await ctx.db.query("tasks").order("desc").take(100);
 *   },
 * });
 * ```
 *
 * @public
 */
export interface GenericQueryCtx<DataModel extends GenericDataModel> {
  /**
   * A utility for reading data in the database.
   *
   * Use `ctx.db.get(table, id)` to fetch a single document by ID, or
   * `ctx.db.query("tableName")` to query multiple documents with filtering
   * and ordering. Queries are read-only, no write methods are available.
   */
  db: GenericDatabaseReader<DataModel>;

  /**
   * Information about the currently authenticated user.
   *
   * Call `await ctx.auth.getUserIdentity()` to get the current user's identity,
   * or `null` if the user is not authenticated.
   */
  auth: Auth;

  /**
   * A utility for reading files in storage.
   *
   * Use `ctx.storage.getUrl(storageId)` to get a URL for a stored file.
   */
  storage: StorageReader;

  /**
   * Call a query function within the same transaction.
   *
   * The query runs within the same read snapshot. Requires a
   * {@link FunctionReference} (e.g., `api.myModule.myQuery` or
   * `internal.myModule.myQuery`).
   *
   * NOTE: Often you can extract shared logic into a helper function instead.
   * `runQuery` incurs overhead of running argument and return value validation,
   * and creating a new isolated JS context.
   */
  runQuery: <
    Query extends
      | FunctionReference<"query", "public" | "internal">
      | FunctionReference_future<"query", "public" | "internal">,
  >(
    query: Query,
    ...args: ArgsAndOptions<Query, { transactionLimits?: TransactionLimits }>
  ) => Promise<FunctionReturnType<Query>>;

  meta: QueryMeta;
}

/**
 * A set of services for use within Convex query functions.
 *
 * The query context is passed as the first argument to any Convex query
 * function run on the server.
 *
 * This differs from the {@link MutationCtx} because all of the services are
 * read-only.
 *
 *
 * @public
 */
export type GenericQueryCtxWithTable<DataModel extends GenericDataModel> = Omit<
  GenericQueryCtx<DataModel>,
  "db"
> & {
  db: GenericDatabaseReaderWithTable<DataModel>;
};

/**
 * A set of services for use within Convex action functions.
 *
 * The action context is passed as the first argument to any Convex action
 * run on the server. Actions can call external APIs and use Node.js libraries,
 * but do **not** have direct database access (`ctx.db` is not available).
 * Use `ctx.runQuery` and `ctx.runMutation` to interact with the database.
 *
 * You should generally use the `ActionCtx` type from
 * `"./_generated/server"`.
 *
 * @example
 * ```typescript
 * import { action } from "./_generated/server";
 * import { internal } from "./_generated/api";
 * import { v } from "convex/values";
 *
 * export const processPayment = action({
 *   args: { orderId: v.id("orders"), amount: v.number() },
 *   returns: v.null(),
 *   handler: async (ctx, args) => {
 *     // Read data via ctx.runQuery:
 *     const order = await ctx.runQuery(internal.orders.get, { id: args.orderId });
 *
 *     // Call external API:
 *     const result = await fetch("https://api.stripe.com/v1/charges", { ... });
 *
 *     // Write results back via ctx.runMutation:
 *     await ctx.runMutation(internal.orders.markPaid, { id: args.orderId });
 *
 *     return null;
 *   },
 * });
 * ```
 *
 * **Common mistake:** `ctx.db` is not available in actions. Do not try to
 * access it, use `ctx.runQuery` and `ctx.runMutation` instead.
 *
 * @public
 */
export interface GenericActionCtx<DataModel extends GenericDataModel> {
  /**
   * Run the Convex query with the given name and arguments.
   *
   * Each `runQuery` call is a separate read transaction. Consider using an
   * {@link internalQuery} to prevent users from calling the query directly.
   *
   * @example
   * ```typescript
   * const user = await ctx.runQuery(internal.users.get, { userId });
   * ```
   *
   * @param query - A {@link FunctionReference} for the query to run.
   * @param args - The arguments to the query function.
   * @returns A promise of the query's result.
   */
  runQuery<
    Query extends
      | FunctionReference<"query", "public" | "internal">
      | FunctionReference_future<"query", "public" | "internal">,
  >(
    query: Query,
    ...args: OptionalRestArgs<Query>
  ): Promise<FunctionReturnType<Query>>;

  /**
   * Run the Convex mutation with the given name and arguments.
   *
   * Each `runMutation` call is a separate write transaction. Consider using
   * an {@link internalMutation} to prevent users from calling it directly.
   *
   * @example
   * ```typescript
   * await ctx.runMutation(internal.orders.markPaid, { id: orderId });
   * ```
   *
   * @param mutation - A {@link FunctionReference} for the mutation to run.
   * @param args - The arguments to the mutation function.
   * @returns A promise of the mutation's result.
   */
  runMutation<
    Mutation extends
      | FunctionReference<"mutation", "public" | "internal">
      | FunctionReference_future<"mutation", "public" | "internal">,
  >(
    mutation: Mutation,
    ...args: OptionalRestArgs<Mutation>
  ): Promise<FunctionReturnType<Mutation>>;

  /**
   * Run the Convex action with the given name and arguments.
   *
   * **Important:** Only use `runAction` when you need to cross runtimes
   * (e.g., calling a `"use node"` action from the default Convex runtime).
   * For code in the same runtime, extract shared logic into a plain
   * TypeScript helper function instead, `runAction` has significant
   * overhead (separate function call, separate resource allocation).
   *
   * Consider using an {@link internalAction} to prevent users from calling the
   * action directly.
   *
   * @param action - A {@link FunctionReference} for the action to run.
   * @param args - The arguments to the action function.
   * @returns A promise of the action's result.
   */
  runAction<
    Action extends
      | FunctionReference<"action", "public" | "internal">
      | FunctionReference_future<"action", "public" | "internal">,
  >(
    action: Action,
    ...args: OptionalRestArgs<Action>
  ): Promise<FunctionReturnType<Action>>;

  /**
   * A utility for scheduling Convex functions to run in the future.
   */
  scheduler: Scheduler;

  /**
   * Information about the currently authenticated user.
   */
  auth: Auth;

  /**
   * A utility for reading and writing files in storage.
   */
  storage: StorageActionWriter;

  /**
   * Run a vector search on the given table and index.
   *
   * @param tableName - The name of the table to query.
   * @param indexName - The name of the vector index on the table to query.
   * @param query - A {@link VectorSearchQuery} containing the vector to query,
   * the number of results to return, and any filters.
   * @returns A promise of IDs and scores for the documents with the nearest
   * vectors
   */
  vectorSearch<
    TableName extends TableNamesInDataModel<DataModel>,
    IndexName extends VectorIndexNames<NamedTableInfo<DataModel, TableName>>,
  >(
    tableName: TableName,
    indexName: IndexName,
    query: Expand<
      VectorSearchQuery<NamedTableInfo<DataModel, TableName>, IndexName>
    >,
  ): Promise<Array<{ _id: Id<TableName>; _score: number }>>;

  meta: ActionMeta;
}

/**
 * The default arguments type for a Convex query, mutation, or action function.
 *
 * Convex functions always take an arguments object that maps the argument
 * names to their values.
 *
 * @public
 */
export type DefaultFunctionArgs = Record<string, unknown>;

/**
 * The arguments array for a function that takes arguments.
 *
 * This is an array of a single {@link DefaultFunctionArgs} element.
 */
type OneArgArray<ArgsObject extends DefaultFunctionArgs = DefaultFunctionArgs> =
  [ArgsObject];

/**
 * The arguments to a function that takes no arguments (just an empty array).
 */
type NoArgsArray = [];

/**
 * An array of arguments to a Convex function.
 *
 * Convex functions can take either a single {@link DefaultFunctionArgs} object or no
 * args at all.
 *
 * @public
 */
export type ArgsArray = OneArgArray | NoArgsArray;

/**
 * A type for the empty object `{}`.
 *
 * Note that we don't use `type EmptyObject = {}` because that matches every object.
 */
export type EmptyObject = Record<string, never>;

/**
 * Convert an {@link ArgsArray} into a single object type.
 *
 * Empty arguments arrays are converted to {@link EmptyObject}.
 * @public
 */
export type ArgsArrayToObject<Args extends ArgsArray> =
  Args extends OneArgArray<infer ArgsObject> ? ArgsObject : EmptyObject;

/**
 * A type representing the visibility of a Convex function.
 *
 * @public
 */
export type FunctionVisibility = "public" | "internal";

/**
 * Given a {@link FunctionVisibility}, should this function have `isPublic: true`
 * or `isInternal: true`?
 */
type VisibilityProperties<Visiblity extends FunctionVisibility> = {
  /** Phantom type marker; not present at runtime. */
  _visibility: Visiblity;
} & (Visiblity extends "public"
  ? {
      isPublic: true;
    }
  : {
      isInternal: true;
    });

/**
 * A mutation function that is part of this app.
 *
 * You can create a mutation by wrapping your function in
 * {@link mutationGeneric} or {@link internalMutationGeneric} and exporting it.
 *
 * @public
 */
export type RegisteredMutation<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = {
  isConvexFunction: true;
  isMutation: true;

  /** @internal */
  invokeMutation(argsStr: string): Promise<string>;

  /** @internal */
  exportArgs(): string;

  /** @internal */
  exportReturns(): string;

  /** @internal */
  _handler: (ctx: GenericMutationCtx<any>, args: Args) => Returns;
} & VisibilityProperties<Visibility>;

/**
 * A query function that is part of this app.
 *
 * You can create a query by wrapping your function in
 * {@link queryGeneric} or {@link internalQueryGeneric} and exporting it.
 *
 * @public
 */
export type RegisteredQuery<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = {
  isConvexFunction: true;
  isQuery: true;

  /** @internal */
  invokeQuery(argsStr: string): Promise<string>;

  /** @internal */
  exportArgs(): string;

  /** @internal */
  exportReturns(): string;

  /** @internal */
  _handler: (ctx: GenericQueryCtx<any>, args: Args) => Returns;
} & VisibilityProperties<Visibility>;

/**
 * An action that is part of this app.
 *
 * You can create an action by wrapping your function in
 * {@link actionGeneric} or {@link internalActionGeneric} and exporting it.
 *
 * @public
 */
export type RegisteredAction<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = {
  isConvexFunction: true;
  isAction: true;

  /** @internal */
  invokeAction(requestId: string, argsStr: string): Promise<string>;

  /** @internal */
  exportArgs(): string;

  /** @internal */
  exportReturns(): string;

  /** @internal */
  _handler: (ctx: GenericActionCtx<any>, args: Args) => Returns;
} & VisibilityProperties<Visibility>;

/**
 * An HTTP action that is part of this app's public API.
 *
 * You can create public HTTP actions by wrapping your function in
 * {@link httpActionGeneric} and exporting it.
 *
 * @public
 */
export type PublicHttpAction = {
  isHttp: true;

  /** @internal */
  invokeHttpAction(request: Request): Promise<Response>;
  /** @internal */
  _handler: (ctx: GenericActionCtx<any>, request: Request) => Promise<Response>;
};

/**
 * @deprecated -- See the type definition for `MutationBuilder` or similar for
 * the types used for defining Convex functions.
 *
 * The definition of a Convex query, mutation, or action function without
 * argument validation.
 *
 * Convex functions always take a context object as their first argument
 * and an (optional) args object as their second argument.
 *
 * This can be written as a function like:
 * ```js
 * import { query } from "./_generated/server";
 *
 * export const func = query(({ db }, { arg }) => {...});
 * ```
 * or as an object like:
 *
 * ```js
 * import { query } from "./_generated/server";
 *
 * export const func = query({
 *   handler: ({ db }, { arg }) => {...},
 * });
 * ```
 * See {@link ValidatedFunction} to add argument validation.
 *
 * @public
 */
export type UnvalidatedFunction<Ctx, Args extends ArgsArray, Returns> =
  | ((ctx: Ctx, ...args: Args) => Returns)
  | {
      handler: (ctx: Ctx, ...args: Args) => Returns;
    };

/**
 * @deprecated -- See the type definition for `MutationBuilder` or similar for
 * the types used for defining Convex functions.
 *
 * The definition of a Convex query, mutation, or action function with argument
 * validation.
 *
 * Argument validation allows you to assert that the arguments to this function
 * are the expected type.
 *
 * Example:
 *
 * ```js
 * import { query } from "./_generated/server";
 * import { v } from "convex/values";
 *
 * export const func = query({
 *   args: {
 *     arg: v.string()
 *   },
 *   handler: ({ db }, { arg }) => {...},
 * });
 * ```
 *
 * **For security, argument validation should be added to all public functions in
 * production apps.**
 *
 * See {@link UnvalidatedFunction} for functions without argument validation.
 * @public
 */
export interface ValidatedFunction<
  Ctx,
  ArgsValidator extends PropertyValidators,
  Returns,
> {
  /**
   * A validator for the arguments of this function.
   *
   * This is an object mapping argument names to validators constructed with
   * {@link values.v}.
   *
   * ```js
   * import { v } from "convex/values";
   *
   * const args = {
   *   stringArg: v.string(),
   *   optionalNumberArg: v.optional(v.number()),
   * }
   * ```
   */
  args: ArgsValidator;

  /**
   * The implementation of this function.
   *
   * This is a function that takes in the appropriate context and arguments
   * and produces some result.
   *
   * @param ctx - The context object. This is one of {@link QueryCtx},
   * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
   * @param args - The arguments object for this function. This will match
   * the type defined by the argument validator.
   * @returns
   */
  handler: (ctx: Ctx, args: ObjectType<ArgsValidator>) => Returns;
}

/**
 * There are multiple syntaxes for defining a Convex function:
 * ```
 *  - query(async (ctx, args) => {...})
 *  - query({ handler: async (ctx, args) => {...} })
 *  - query({ args: { a: v.string }, handler: async (ctx, args) => {...} } })
 *  - query({ args: { a: v.string }, returns: v.string(), handler: async (ctx, args) => {...} } })
 *```
 *
 * In each of these, we want to correctly infer the type for the arguments and
 * return value, preferring the type derived from a validator if it's provided.
 *
 * To avoid having a separate overload for each, which would show up in error messages,
 * we use the type params -- ArgsValidator, ReturnsValidator, ReturnValue, OneOrZeroArgs.
 *
 * The type for ReturnValue and OneOrZeroArgs are constrained by the type or ArgsValidator and
 * ReturnsValidator if they're present, and inferred from any explicit type annotations to the
 * arguments or return value of the function.
 *
 * Below are a few utility types to get the appropriate type constraints based on
 * an optional validator.
 *
 * Additional tricks:
 * - We use Validator | void instead of Validator | undefined because the latter does
 * not work with `strictNullChecks` since it's equivalent to just `Validator`.
 * - We use a tuple type of length 1 to avoid distribution over the union
 *  https://github.com/microsoft/TypeScript/issues/29368#issuecomment-453529532
 */

export type ReturnValueForOptionalValidator<
  ReturnsValidator extends Validator<any, any, any> | PropertyValidators | void,
> = [ReturnsValidator] extends [Validator<any, any, any>]
  ? ValidatorTypeToReturnType<Infer<ReturnsValidator>>
  : [ReturnsValidator] extends [PropertyValidators]
    ? ValidatorTypeToReturnType<ObjectType<ReturnsValidator>>
    : any;

export type ArgsArrayForOptionalValidator<
  ArgsValidator extends GenericValidator | PropertyValidators | void,
> = [ArgsValidator] extends [Validator<any, any, any>]
  ? OneArgArray<Infer<ArgsValidator>>
  : [ArgsValidator] extends [PropertyValidators]
    ? OneArgArray<ObjectType<ArgsValidator>>
    : ArgsArray;

export type DefaultArgsForOptionalValidator<
  ArgsValidator extends GenericValidator | PropertyValidators | void,
> = [ArgsValidator] extends [Validator<any, any, any>]
  ? [Infer<ArgsValidator>]
  : [ArgsValidator] extends [PropertyValidators]
    ? [ObjectType<ArgsValidator>]
    : OneArgArray;

/**
 * Internal type helper used by Convex code generation.
 *
 * Used to give {@link mutationGeneric} a type specific to your data model.
 * @public
 */
export type MutationBuilder<
  DataModel extends GenericDataModel,
  Visibility extends FunctionVisibility,
> = {
  <
    ArgsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnValue extends ReturnValueForOptionalValidator<ReturnsValidator> = any,
    OneOrZeroArgs extends ArgsArrayForOptionalValidator<ArgsValidator> =
      DefaultArgsForOptionalValidator<ArgsValidator>,
  >(
    mutation:
      | {
          /**
           * Argument validation.
           *
           * Examples:
           *
           * ```
           * args: {}
           * args: { input: v.optional(v.number()) }
           * args: { message: v.string(), author: v.id("authors") }
           * args: { messages: v.array(v.string()) }
           * ```
           */
          args?: ArgsValidator;
          /**
           * The return value validator.
           *
           * Examples:
           *
           * ```
           * returns: v.null()
           * returns: v.string()
           * returns: { message: v.string(), author: v.id("authors") }
           * returns: v.array(v.string())
           * ```
           */
          returns?: ReturnsValidator;
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          handler: (
            ctx: GenericMutationCtx<DataModel>,
            ...args: OneOrZeroArgs
          ) => ReturnValue;
        }
      | {
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          (
            ctx: GenericMutationCtx<DataModel>,
            ...args: OneOrZeroArgs
          ): ReturnValue;
        },
  ): RegisteredMutation<
    Visibility,
    ArgsArrayToObject<OneOrZeroArgs>,
    ReturnValue
  >;
};

/**
 * Internal type helper used by Convex code generation.
 *
 * Used to give {@link mutationGeneric} a type specific to your data model.
 * @public
 */
export type MutationBuilderWithTable<
  DataModel extends GenericDataModel,
  Visibility extends FunctionVisibility,
> = {
  <
    ArgsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnValue extends ReturnValueForOptionalValidator<ReturnsValidator> = any,
    OneOrZeroArgs extends ArgsArrayForOptionalValidator<ArgsValidator> =
      DefaultArgsForOptionalValidator<ArgsValidator>,
  >(
    mutation:
      | {
          /**
           * Argument validation.
           *
           * Examples:
           *
           * ```
           * args: {}
           * args: { input: v.optional(v.number()) }
           * args: { message: v.string(), author: v.id("authors") }
           * args: { messages: v.array(v.string()) }
           * ```
           */
          args?: ArgsValidator;
          /**
           * The return value validator.
           *
           * Examples:
           *
           * ```
           * returns: v.null()
           * returns: v.string()
           * returns: { message: v.string(), author: v.id("authors") }
           * returns: v.array(v.string())
           * ```
           */
          returns?: ReturnsValidator;
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          handler: (
            ctx: GenericMutationCtxWithTable<DataModel>,
            ...args: OneOrZeroArgs
          ) => ReturnValue;
        }
      | {
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          (
            ctx: GenericMutationCtxWithTable<DataModel>,
            ...args: OneOrZeroArgs
          ): ReturnValue;
        },
  ): RegisteredMutation<
    Visibility,
    ArgsArrayToObject<OneOrZeroArgs>,
    ReturnValue
  >;
};

/**
 * Internal type helper used by Convex code generation.
 *
 * Used to give {@link queryGeneric} a type specific to your data model.
 * @public
 */
export type QueryBuilder<
  DataModel extends GenericDataModel,
  Visibility extends FunctionVisibility,
> = {
  <
    ArgsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnValue extends ReturnValueForOptionalValidator<ReturnsValidator> = any,
    OneOrZeroArgs extends ArgsArrayForOptionalValidator<ArgsValidator> =
      DefaultArgsForOptionalValidator<ArgsValidator>,
  >(
    query:
      | {
          /**
           * Argument validation.
           *
           * Examples:
           *
           * ```
           * args: {}
           * args: { input: v.optional(v.number()) }
           * args: { message: v.string(), author: v.id("authors") }
           * args: { messages: v.array(v.string()) }
           * ```
           */
          args?: ArgsValidator;
          /**
           * The return value validator.
           *
           * Examples:
           *
           * ```
           * returns: v.null()
           * returns: v.string()
           * returns: { message: v.string(), author: v.id("authors") }
           * returns: v.array(v.string())
           * ```
           */
          returns?: ReturnsValidator;
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          handler: (
            ctx: GenericQueryCtx<DataModel>,
            ...args: OneOrZeroArgs
          ) => ReturnValue;
        }
      | {
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          (
            ctx: GenericQueryCtx<DataModel>,
            ...args: OneOrZeroArgs
          ): ReturnValue;
        },
  ): RegisteredQuery<Visibility, ArgsArrayToObject<OneOrZeroArgs>, ReturnValue>;
};

/**
 * Internal type helper used by Convex code generation.
 *
 * Used to give {@link queryGeneric} a type specific to your data model.
 * @public
 */
export type QueryBuilderWithTable<
  DataModel extends GenericDataModel,
  Visibility extends FunctionVisibility,
> = {
  <
    ArgsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnValue extends ReturnValueForOptionalValidator<ReturnsValidator> = any,
    OneOrZeroArgs extends ArgsArrayForOptionalValidator<ArgsValidator> =
      DefaultArgsForOptionalValidator<ArgsValidator>,
  >(
    query:
      | {
          /**
           * Argument validation.
           *
           * Examples:
           *
           * ```
           * args: {}
           * args: { input: v.optional(v.number()) }
           * args: { message: v.string(), author: v.id("authors") }
           * args: { messages: v.array(v.string()) }
           * ```
           */
          args?: ArgsValidator;
          /**
           * The return value validator.
           *
           * Examples:
           *
           * ```
           * returns: v.null()
           * returns: v.string()
           * returns: { message: v.string(), author: v.id("authors") }
           * returns: v.array(v.string())
           * ```
           */
          returns?: ReturnsValidator;
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          handler: (
            ctx: GenericQueryCtxWithTable<DataModel>,
            ...args: OneOrZeroArgs
          ) => ReturnValue;
        }
      | {
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          (
            ctx: GenericQueryCtxWithTable<DataModel>,
            ...args: OneOrZeroArgs
          ): ReturnValue;
        },
  ): RegisteredQuery<Visibility, ArgsArrayToObject<OneOrZeroArgs>, ReturnValue>;
};

/**
 * Internal type helper used by Convex code generation.
 *
 * Used to give {@link actionGeneric} a type specific to your data model.
 * @public
 */
export type ActionBuilder<
  DataModel extends GenericDataModel,
  Visibility extends FunctionVisibility,
> = {
  <
    ArgsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnsValidator extends
      | PropertyValidators
      | Validator<any, "required", any>
      | void,
    ReturnValue extends ReturnValueForOptionalValidator<ReturnsValidator> = any,
    OneOrZeroArgs extends ArgsArrayForOptionalValidator<ArgsValidator> =
      DefaultArgsForOptionalValidator<ArgsValidator>,
  >(
    func:
      | {
          /**
           * Argument validation.
           *
           * Examples:
           *
           * ```
           * args: {}
           * args: { input: v.optional(v.number()) }
           * args: { message: v.string(), author: v.id("authors") }
           * args: { messages: v.array(v.string()) }
           * ```
           *
           */
          args?: ArgsValidator;
          /**
           * The return value validator.
           *
           * Examples:
           *
           * ```
           * returns: v.null()
           * returns: v.string()
           * returns: { message: v.string(), author: v.id("authors") }
           * returns: v.array(v.string())
           * ```
           */
          returns?: ReturnsValidator;
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          handler: (
            ctx: GenericActionCtx<DataModel>,
            ...args: OneOrZeroArgs
          ) => ReturnValue;
        }
      | {
          /**
           * The implementation of this function.
           *
           * This is a function that takes in the appropriate context and arguments
           * and produces some result.
           *
           * @param ctx - The context object. This is one of {@link QueryCtx},
           * {@link MutationCtx}, or {@link ActionCtx} depending on the function type.
           * @param args - The arguments object for this function. This will match
           * the type defined by the argument validator if provided.
           * @returns
           */
          (
            ctx: GenericActionCtx<DataModel>,
            ...args: OneOrZeroArgs
          ): ReturnValue;
        },
  ): RegisteredAction<
    Visibility,
    ArgsArrayToObject<OneOrZeroArgs>,
    ReturnValue
  >;
};

/**
 * Internal type helper used by Convex code generation.
 *
 * Used to give {@link httpActionGeneric} a type specific to your data model
 * and functions.
 * @public
 */
export type HttpActionBuilder = (
  func: (ctx: GenericActionCtx<any>, request: Request) => Promise<Response>,
) => PublicHttpAction;

export interface AdvancedRunQueryOptions {
  /**
   * Custom limits for a nested transaction. Each field specifies the absolute
   * maximum allowed for the nested function call. Values are capped at the
   * global transaction limits, so they can only lower limits, never raise them.
   */
  transactionLimits?: TransactionLimits;
  /**
   * Run a query on a recent snapshot of the database that is not guaranteed
   * to be up-to-date when this transaction commits.
   *
   * This is an advanced feature which can introduce subtle race conditions,
   * so its use is generally discouraged except for specific use-cases where
   * database read conflicts are expected, e.g. reading from an append-only
   * table with immutable records where the only read conflicts are from
   * concurrent appends.
   *
   * To determine the snapshot of the nested query, you can call
   * `ctx.meta.getSnapshotTs()` from within the stale snapshot query. To
   * compare to the parent mutation's snapshot timestamp, you can return that
   * value from the query and call `ctx.meta.getSnapshotTs()` from the parent.
   */
  useStaleSnapshot?: boolean;
}
