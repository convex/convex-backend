import { BaseConvexClient } from "../browser/index.js";
import type {
  OptimisticUpdate,
  PaginatedQueryToken,
  QueryToken,
  PaginationStatus,
} from "../browser/index.js";
import React, { useCallback, useContext, useMemo } from "react";
import { convexToJson, Value } from "../values/index.js";
import { QueryJournal } from "../browser/sync/protocol.js";
import {
  AuthTokenFetcher,
  BaseConvexClientOptions,
  ConnectionState,
  BaseConvexClientInterface,
} from "../browser/sync/client.js";
import type { UserIdentityAttributes } from "../browser/sync/protocol.js";
import { RequestForQueries, useQueries } from "./use_queries.js";
import { useSubscription } from "./use_subscription.js";
import { parseArgs } from "../common/index.js";
import {
  ArgsAndOptions,
  FunctionArgs,
  FunctionReturnType,
  OptionalRestArgs,
  getFunctionName,
  makeFunctionReference,
  FunctionReference,
  FunctionReference_future,
} from "../server/api.js";
import { EmptyObject } from "../server/registration.js";
import {
  instantiateDefaultLogger,
  instantiateNoopLogger,
  Logger,
} from "../browser/logging.js";
import type { QueryOptions } from "../browser/query_options.js";
import { LoadMoreOfPaginatedQuery } from "../browser/sync/pagination.js";
import {
  PaginatedQueryClient,
  ExtendedTransition,
} from "../browser/sync/paginated_query_client.js";

// When no arguments are passed, extend subscriptions (for APIs that do this by default)
// for this amount after the subscription would otherwise be dropped.
const DEFAULT_EXTEND_SUBSCRIPTION_FOR = 5_000;

if (typeof React === "undefined") {
  throw new Error("Required dependency 'react' not found");
}

// TODO Typedoc doesn't generate documentation for the comment below perhaps
// because it's a callable interface.
/**
 * An interface to execute a Convex mutation function on the server.
 *
 * @public
 */
export interface ReactMutation<
  Mutation extends
    | FunctionReference<"mutation">
    | FunctionReference_future<"mutation">,
> {
  /**
   * Execute the mutation on the server, returning a `Promise` of its return value.
   *
   * @param args - Arguments for the mutation to pass up to the server.
   * @returns The return value of the server-side function call.
   */
  (...args: OptionalRestArgs<Mutation>): Promise<FunctionReturnType<Mutation>>;

  /**
   * Define an optimistic update to apply as part of this mutation.
   *
   * This is a temporary update to the local query results to facilitate a
   * fast, interactive UI. It enables query results to update before a mutation
   * executed on the server.
   *
   * When the mutation is invoked, the optimistic update will be applied.
   *
   * Optimistic updates can also be used to temporarily remove queries from the
   * client and create loading experiences until a mutation completes and the
   * new query results are synced.
   *
   * The update will be automatically rolled back when the mutation is fully
   * completed and queries have been updated.
   *
   * @param optimisticUpdate - The optimistic update to apply.
   * @returns A new `ReactMutation` with the update configured.
   *
   * @public
   */
  withOptimisticUpdate<T extends OptimisticUpdate<FunctionArgs<Mutation>>>(
    optimisticUpdate: T &
      (ReturnType<T> extends Promise<any>
        ? "Optimistic update handlers must be synchronous"
        : {}),
  ): ReactMutation<Mutation>;
}

// Exported only for testing.
export function createMutation(
  mutationReference:
    | FunctionReference<"mutation">
    | FunctionReference_future<"mutation">,
  client: ConvexReactClient,
  update?: OptimisticUpdate<any>,
): ReactMutation<any> {
  function mutation(args?: Record<string, Value>): Promise<unknown> {
    assertNotAccidentalArgument(args);

    return client.mutation(mutationReference, args, {
      optimisticUpdate: update,
    });
  }
  mutation.withOptimisticUpdate = function withOptimisticUpdate(
    optimisticUpdate: OptimisticUpdate<any>,
  ): ReactMutation<any> {
    if (update !== undefined) {
      throw new Error(
        `Already specified optimistic update for mutation ${getFunctionName(
          mutationReference,
        )}`,
      );
    }
    return createMutation(mutationReference, client, optimisticUpdate);
  };
  return mutation as ReactMutation<any>;
}

/**
 * An interface to execute a Convex action on the server.
 *
 * @public
 */
export interface ReactAction<
  Action extends
    | FunctionReference<"action">
    | FunctionReference_future<"action">,
> {
  /**
   * Execute the function on the server, returning a `Promise` of its return value.
   *
   * @param args - Arguments for the function to pass up to the server.
   * @returns The return value of the server-side function call.
   * @public
   */
  (...args: OptionalRestArgs<Action>): Promise<FunctionReturnType<Action>>;
}

function createAction(
  actionReference:
    | FunctionReference<"action">
    | FunctionReference_future<"action">,
  client: ConvexReactClient,
): ReactAction<any> {
  return function (args?: Record<string, Value>): Promise<unknown> {
    return client.action(actionReference, args);
  } as ReactAction<any>;
}

// Watches should be stateless: in QueriesObserver we create a watch just to get
// the current value.
/**
 * A watch on the output of a Convex query function.
 *
 * @public
 */
export interface Watch<T> {
  /**
   * Initiate a watch on the output of a query.
   *
   * This will subscribe to this query and call
   * the callback whenever the query result changes.
   *
   * **Important: If the client is already subscribed to this query with the
   * same arguments this callback will not be invoked until the query result is
   * updated.** To get the current, local result call
   * {@link react.Watch.localQueryResult}.
   *
   * @param callback - Function that is called whenever the query result changes.
   * @returns - A function that disposes of the subscription.
   */
  onUpdate(callback: () => void): () => void;

  /**
   * Get the current result of a query.
   *
   * This will only return a result if we're already subscribed to the query
   * and have received a result from the server or the query value has been set
   * optimistically.
   *
   * @returns The result of the query or `undefined` if it isn't known.
   * @throws An error if the query encountered an error on the server.
   */
  localQueryResult(): T | undefined;

  /**
   * @internal
   */
  localQueryLogs(): string[] | undefined;

  /**
   * Get the current {@link browser.QueryJournal} for this query.
   *
   * If we have not yet received a result for this query, this will be `undefined`.
   */
  journal(): QueryJournal | undefined;
}

/**
 * A watch on the output of a paginated Convex query function.
 *
 * @public
 */
export interface PaginatedWatch<T> {
  /**
   * Initiate a watch on the output of a paginated query.
   *
   * This will subscribe to this query and call
   * the callback whenever the query result changes.
   *
   * @param callback - Function that is called whenever the query result changes.
   * @returns - A function that disposes of the subscription.
   */
  onUpdate(callback: () => void): () => void;

  /**
   * Get the current result of a paginated query.
   *
   * @returns The current results, status, and loadMore function, or `undefined` if not loaded.
   */
  localQueryResult():
    | {
        results: T[];
        status: PaginationStatus;
        loadMore: LoadMoreOfPaginatedQuery;
      }
    | undefined;
}

/**
 * Options for {@link ConvexReactClient.watchQuery}.
 *
 * @public
 */
export interface WatchQueryOptions {
  /**
   * An (optional) journal produced from a previous execution of this query
   * function.
   *
   * If there is an existing subscription to a query function with the same
   * name and arguments, this journal will have no effect.
   */
  journal?: QueryJournal;

  /**
   * @internal
   */
  componentPath?: string;
}

/**
 * Options for {@link ConvexReactClient.watchPaginatedQuery}.
 *
 * @internal
 */
export interface WatchPaginatedQueryOptions {
  /**
   * The initial number of items to load.
   */
  initialNumItems: number;

  // We may be able to remove this in the future, but to preserve the existing behavior of
  // usePaginatedQuery() it's still here.
  id: number;

  /**
   * @internal
   */
  componentPath?: string;
}

/**
 * Options for {@link ConvexReactClient.mutation}.
 *
 * @public
 */
export interface MutationOptions<Args extends Record<string, Value>> {
  /**
   * An optimistic update to apply along with this mutation.
   *
   * An optimistic update locally updates queries while a mutation is pending.
   * Once the mutation completes, the update will be rolled back.
   */
  optimisticUpdate?: OptimisticUpdate<Args> | undefined;
}

/**
 * Options for {@link ConvexReactClient}.
 *
 * @public
 */
export interface ConvexReactClientOptions extends BaseConvexClientOptions {
  /**
   * An already-constructed {@link BaseConvexClient} to use instead of
   * constructing one internally. When provided, this client (and its
   * derived {@link PaginatedQueryClient}) is used for all operations.
   *
   * @internal
   */
  baseClient?: BaseConvexClientInterface;

  // FIXME: Ideally we define `ConvexReactClientOptions` as
  // `type ConvexReactClientOptions = BaseConvexClientOptions | { baseClient: BaseConvexClient }`
  // so that callers won’t try to set `BaseConvexClientOptions` attributes that are ineffective.
  // But since `baseClient` is internal at the moment, I don’t think we can do that.
  // We can fix this if we ever make `baseClient` public.
}

/**
 * A Convex client for use within React.
 *
 * This loads reactive queries and executes mutations over a WebSocket.
 *
 * @public
 */
export class ConvexReactClient {
  private address: string;
  private cachedSync?: BaseConvexClientInterface | undefined;
  private cachedPaginatedQueryClient?: PaginatedQueryClient | undefined;
  private listeners: Map<QueryToken | PaginatedQueryToken, Set<() => void>>;
  private options: ConvexReactClientOptions;
  // "closed" means this client is done, not just that the underlying WS connection is closed.
  private closed = false;
  private _logger: Logger;

  private adminAuth?: string;
  private fakeUserIdentity?: UserIdentityAttributes | undefined;

  /**
   * @param address - The url of your Convex deployment, often provided
   * by an environment variable. E.g. `https://small-mouse-123.convex.cloud`.
   * @param options - See {@link ConvexReactClientOptions} for a full description.
   */
  constructor(address: string, options?: ConvexReactClientOptions) {
    // Validate address immediately since validation by the lazily-instantiated
    // internal client does not occur synchronously.
    if (address === undefined) {
      throw new Error(
        "No address provided to ConvexReactClient.\n" +
          "If trying to deploy to production, make sure to follow all the instructions found at https://docs.convex.dev/production/hosting/\n" +
          "If running locally, make sure to run `convex dev` and ensure the .env.local file is populated.",
      );
    }
    if (typeof address !== "string") {
      throw new Error(
        `ConvexReactClient requires a URL like 'https://happy-otter-123.convex.cloud', received something of type ${typeof address} instead.`,
      );
    }
    if (!address.includes("://")) {
      throw new Error("Provided address was not an absolute URL.");
    }
    this.address = address;
    this.listeners = new Map();
    this._logger =
      options?.logger === false
        ? instantiateNoopLogger({ verbose: options?.verbose ?? false })
        : options?.logger !== true && options?.logger
          ? options.logger
          : instantiateDefaultLogger({ verbose: options?.verbose ?? false });
    this.options = { ...options, logger: this._logger };
  }

  /**
   * Return the address for this client, useful for creating a new client.
   *
   * Not guaranteed to match the address with which this client was constructed:
   * it may be canonicalized.
   */
  get url() {
    return this.address;
  }

  /**
   * Lazily instantiate the `BaseConvexClient` so we don't create the WebSocket
   * when server-side rendering.
   *
   * @internal
   */
  get sync() {
    if (this.closed) {
      throw new Error("ConvexReactClient has already been closed.");
    }
    if (this.cachedSync) {
      return this.cachedSync;
    }
    // BaseConvexClient and paginated query client are always created together.
    this.cachedSync =
      this.options.baseClient ??
      new BaseConvexClient(
        this.address,
        () => {}, // Use the PaginatedQueryClient's transition instead.
        this.options,
      );
    if (this.adminAuth) {
      this.cachedSync.setAdminAuth(this.adminAuth, this.fakeUserIdentity);
    }
    this.cachedPaginatedQueryClient = new PaginatedQueryClient(
      this.cachedSync,
      (transition) => this.handleTransition(transition),
    );
    return this.cachedSync;
  }

  /**
   * Lazily instantiate the `PaginatedQueryClient` so we don't create it
   * when server-side rendering.
   *
   * @internal
   */
  get paginatedQueryClient() {
    // access sync to instantiate the clients
    this.sync;
    if (this.cachedPaginatedQueryClient) {
      return this.cachedPaginatedQueryClient;
    }
    throw new Error("Should already be instantiated");
  }

  /**
   * Set the authentication token to be used for subsequent queries and mutations.
   * `fetchToken` will be called automatically again if a token expires.
   * `fetchToken` should return `null` if the token cannot be retrieved, for example
   * when the user's rights were permanently revoked.
   * @param fetchToken - an async function returning the JWT-encoded OpenID Connect Identity Token
   * @param onChange - a callback that will be called when the authentication status changes
   * @param onRefreshChange - a callback called with `true` when the socket is paused to fetch a replacement token after a server rejection, and `false` when refresh completes
   */
  setAuth(
    fetchToken: AuthTokenFetcher,
    onChange?: (isAuthenticated: boolean) => void,
    onRefreshChange?: (isRefreshing: boolean) => void,
  ) {
    if (typeof fetchToken === "string") {
      throw new Error(
        "Passing a string to ConvexReactClient.setAuth is no longer supported, " +
          "please upgrade to passing in an async function to handle reauthentication.",
      );
    }
    this.sync.setAuth(
      fetchToken,
      onChange ??
        (() => {
          // Do nothing
        }),
      onRefreshChange,
    );
  }

  /**
   * Clear the current authentication token if set.
   */
  clearAuth() {
    this.sync.clearAuth();
  }

  /**
   * @internal
   */
  setAdminAuth(token: string, identity?: UserIdentityAttributes) {
    this.adminAuth = token;
    this.fakeUserIdentity = identity;
    if (this.closed) {
      throw new Error("ConvexReactClient has already been closed.");
    }
    if (this.cachedSync) {
      this.sync.setAdminAuth(token, identity);
    }
  }

  /**
   * Construct a new {@link Watch} on a Convex query function.
   *
   * **Most application code should not call this method directly. Instead use
   * the {@link useQuery} hook.**
   *
   * The act of creating a watch does nothing, a Watch is stateless.
   *
   * @param query - A {@link server.FunctionReference} for the public query to run.
   * @param args - An arguments object for the query. If this is omitted,
   * the arguments will be `{}`.
   * @param options - A {@link WatchQueryOptions} options object for this query.
   *
   * @returns The {@link Watch} object.
   */
  watchQuery<
    Query extends
      | FunctionReference<"query">
      | FunctionReference_future<"query">,
  >(
    query: Query,
    ...argsAndOptions: ArgsAndOptions<Query, WatchQueryOptions>
  ): Watch<FunctionReturnType<Query>> {
    const [args, options] = argsAndOptions;
    const name = getFunctionName(query);

    return {
      onUpdate: (callback) => {
        const { queryToken, unsubscribe } = this.sync.subscribe(
          name as string,
          args,
          options,
        );

        const currentListeners = this.listeners.get(queryToken);
        if (currentListeners !== undefined) {
          currentListeners.add(callback);
        } else {
          this.listeners.set(queryToken, new Set([callback]));
        }

        return () => {
          if (this.closed) {
            return;
          }

          const currentListeners = this.listeners.get(queryToken)!;
          currentListeners.delete(callback);
          if (currentListeners.size === 0) {
            this.listeners.delete(queryToken);
          }
          unsubscribe();
        };
      },

      localQueryResult: () => {
        // Use the cached client because we can't have a query result if we don't
        // even have a client yet!
        if (this.cachedSync) {
          return this.cachedSync.localQueryResult(name, args);
        }
        return undefined;
      },

      localQueryLogs: () => {
        if (this.cachedSync) {
          return this.cachedSync.localQueryLogs(name, args);
        }
        return undefined;
      },

      journal: () => {
        if (this.cachedSync) {
          return this.cachedSync.queryJournal(name, args);
        }
        return undefined;
      },
    };
  }

  // Let's try out a queryOptions-style API.
  // This method is similar to the React Query API `queryClient.prefetchQuery()`.
  // In the future an ensureQueryData(): Promise<Data> method could exist.
  /**
   * Indicates likely future interest in a query subscription.
   *
   * The implementation currently immediately subscribes to a query. In the future this method
   * may prioritize some queries over others, fetch the query result without subscribing, or
   * do nothing in slow network connections or high load scenarios.
   *
   * To use this in a React component, call useQuery() and ignore the return value.
   *
   * @param queryOptions - A query (function reference from an api object) and its args, plus
   * an optional extendSubscriptionFor for how long to subscribe to the query.
   */
  prewarmQuery<
    Query extends
      | FunctionReference<"query">
      | FunctionReference_future<"query">,
  >(queryOptions: QueryOptions<Query> & { extendSubscriptionFor?: number }) {
    const extendSubscriptionFor =
      queryOptions.extendSubscriptionFor ?? DEFAULT_EXTEND_SUBSCRIPTION_FOR;
    const watch = this.watchQuery(queryOptions.query, queryOptions.args || {});
    const unsubscribe = watch.onUpdate(() => {});
    setTimeout(unsubscribe, extendSubscriptionFor);
  }

  /**
   * Construct a new {@link PaginatedWatch} on a Convex paginated query function.
   *
   * **Most application code should not call this method directly. Instead use
   * the {@link usePaginatedQuery} hook.**
   *
   * The act of creating a watch does nothing, a Watch is stateless.
   *
   * @param query - A {@link server.FunctionReference} for the public query to run.
   * @param args - An arguments object for the query. If this is omitted,
   * the arguments will be `{}`.
   * @param options - A {@link WatchPaginatedQueryOptions} options object for this query.
   *
   * @returns The {@link PaginatedWatch} object.
   *
   * @internal
   */
  watchPaginatedQuery<
    Query extends
      | FunctionReference<"query">
      | FunctionReference_future<"query">,
  >(
    query: Query,
    args: FunctionArgs<Query>,
    options: WatchPaginatedQueryOptions,
  ): PaginatedWatch<FunctionReturnType<Query>> {
    const name = getFunctionName(query);

    return {
      onUpdate: (callback) => {
        const { paginatedQueryToken, unsubscribe } =
          this.paginatedQueryClient.subscribe(name, args || {}, options);

        const currentListeners = this.listeners.get(paginatedQueryToken);
        if (currentListeners !== undefined) {
          currentListeners.add(callback);
        } else {
          this.listeners.set(paginatedQueryToken, new Set([callback]));
        }

        return () => {
          if (this.closed) {
            return;
          }

          const currentListeners = this.listeners.get(paginatedQueryToken)!;
          currentListeners.delete(callback);
          if (currentListeners.size === 0) {
            this.listeners.delete(paginatedQueryToken);
          }
          unsubscribe();
        };
      },

      localQueryResult: () => {
        // Use our new paginated query client
        return this.paginatedQueryClient.localQueryResult(name, args, options);
      },
    };
  }

  /**
   * Execute a mutation function.
   *
   * @param mutation - A {@link server.FunctionReference} for the public mutation
   * to run.
   * @param args - An arguments object for the mutation. If this is omitted,
   * the arguments will be `{}`.
   * @param options - A {@link MutationOptions} options object for the mutation.
   * @returns A promise of the mutation's result.
   */
  mutation<
    Mutation extends
      | FunctionReference<"mutation">
      | FunctionReference_future<"mutation">,
  >(
    mutation: Mutation,
    ...argsAndOptions: ArgsAndOptions<
      Mutation,
      MutationOptions<FunctionArgs<Mutation>>
    >
  ): Promise<FunctionReturnType<Mutation>> {
    const [args, options] = argsAndOptions;
    const name = getFunctionName(mutation);
    return this.sync.mutation(name, args, options);
  }

  /**
   * Execute an action function.
   *
   * @param action - A {@link server.FunctionReference} for the public action
   * to run.
   * @param args - An arguments object for the action. If this is omitted,
   * the arguments will be `{}`.
   * @returns A promise of the action's result.
   */
  action<
    Action extends
      | FunctionReference<"action">
      | FunctionReference_future<"action">,
  >(
    action: Action,
    ...args: OptionalRestArgs<Action>
  ): Promise<FunctionReturnType<Action>> {
    const name = getFunctionName(action);
    return this.sync.action(name, ...args);
  }

  /**
   * Fetch a query result once.
   *
   * **Most application code should subscribe to queries instead, using
   * the {@link useQuery} hook.**
   *
   * @param query - A {@link server.FunctionReference} for the public query
   * to run.
   * @param args - An arguments object for the query. If this is omitted,
   * the arguments will be `{}`.
   * @returns A promise of the query's result.
   */
  query<
    Query extends
      | FunctionReference<"query">
      | FunctionReference_future<"query">,
  >(
    query: Query,
    ...args: OptionalRestArgs<Query>
  ): Promise<FunctionReturnType<Query>> {
    const watch = this.watchQuery(query, ...args);
    const existingResult = watch.localQueryResult();
    if (existingResult !== undefined) {
      return Promise.resolve(existingResult);
    }
    return new Promise((resolve, reject) => {
      const unsubscribe = watch.onUpdate(() => {
        unsubscribe();
        try {
          resolve(watch.localQueryResult());
        } catch (e) {
          reject(e);
        }
      });
    });
  }

  /**
   * Get the current {@link ConnectionState} between the client and the Convex
   * backend.
   *
   * @returns The {@link ConnectionState} with the Convex backend.
   */
  connectionState(): ConnectionState {
    return this.sync.connectionState();
  }

  /**
   * Subscribe to the {@link ConnectionState} between the client and the Convex
   * backend, calling a callback each time it changes.
   *
   * Subscribed callbacks will be called when any part of ConnectionState changes.
   * ConnectionState may grow in future versions (e.g. to provide a array of
   * inflight requests) in which case callbacks would be called more frequently.
   * ConnectionState may also *lose* properties in future versions as we figure
   * out what information is most useful. As such this API is considered unstable.
   *
   * @returns An unsubscribe function to stop listening.
   */
  subscribeToConnectionState(
    cb: (connectionState: ConnectionState) => void,
  ): () => void {
    return this.sync.subscribeToConnectionState(cb);
  }

  /**
   * Get the logger for this client.
   *
   * @returns The {@link Logger} for this client.
   */
  get logger(): Logger {
    return this._logger;
  }

  /**
   * Close any network handles associated with this client and stop all subscriptions.
   *
   * Call this method when you're done with a {@link ConvexReactClient} to
   * dispose of its sockets and resources.
   *
   * @returns A `Promise` fulfilled when the connection has been completely closed.
   */
  async close(): Promise<void> {
    this.closed = true;
    // Prevent outstanding React batched updates from invoking listeners.
    this.listeners = new Map();
    if (this.cachedPaginatedQueryClient) {
      this.cachedPaginatedQueryClient = undefined;
    }
    if (this.cachedSync) {
      const sync = this.cachedSync;
      this.cachedSync = undefined;
      await sync.close();
    }
  }

  /**
   * Handle transitions from both base client and paginated client.
   * This ensures all transitions are processed synchronously and in order.
   */
  private handleTransition(transition: ExtendedTransition) {
    const simple = transition.queries.map((q) => q.token);
    const paginated = transition.paginatedQueries.map((q) => q.token);
    this.transition([...simple, ...paginated]);
  }

  private transition(updatedQueries: (QueryToken | PaginatedQueryToken)[]) {
    for (const queryToken of updatedQueries) {
      const callbacks = this.listeners.get(queryToken);
      if (callbacks) {
        for (const callback of callbacks) {
          callback();
        }
      }
    }
  }
}

const ConvexContext = React.createContext<ConvexReactClient>(
  undefined as unknown as ConvexReactClient, // in the future this will be a mocked client for testing
);

/**
 * Get the {@link ConvexReactClient} within a React component.
 *
 * This relies on the {@link ConvexProvider} being above in the React component tree.
 *
 * @returns The active {@link ConvexReactClient} object, or `undefined`.
 *
 * @public
 */
export function useConvex(): ConvexReactClient {
  return useContext(ConvexContext);
}

/**
 * Provides an active Convex {@link ConvexReactClient} to descendants of this component.
 *
 * Wrap your app in this component to use Convex hooks `useQuery`,
 * `useMutation`, and `useConvex`.
 *
 * @param props - an object with a `client` property that refers to a {@link ConvexReactClient}.
 *
 * @public
 */
export const ConvexProvider: React.FC<{
  client: ConvexReactClient;
  children?: React.ReactNode;
}> = ({ client, children }) => {
  return React.createElement(
    ConvexContext.Provider,
    { value: client },
    children,
  );
};

export type OptionalRestArgsOrSkip<
  FuncRef extends FunctionReference<any> | FunctionReference_future<any>,
> =
  FunctionArgs<FuncRef> extends EmptyObject
    ? [args?: EmptyObject | "skip"]
    : [args: FunctionArgs<FuncRef> | "skip"];

/**
 * Result returned by object-form {@link useQuery_experimental}.
 *
 * @public
 */
export type UseQueryResult<QueryResult, ThrowOnError extends boolean = false> =
  | { status: "pending" }
  | { status: "success"; data: QueryResult }
  | (ThrowOnError extends true ? never : { status: "error"; error: Error });

type UseQueryOptions<
  Query extends FunctionReference<"query"> | FunctionReference_future<"query">,
  ThrowOnError extends boolean,
> = {
  query: Query;
  args: FunctionArgs<Query> | "skip";
  throwOnError?: ThrowOnError;
};

/**
 * Load a reactive query within a React component.
 *
 * This React hook subscribes to a Convex query and causes a rerender whenever
 * the query result changes. The subscription is managed automatically --
 * it starts when the component mounts and stops when it unmounts.
 *
 * Throws an error if not used under {@link ConvexProvider}.
 *
 * @example
 * ```tsx
 * import { useQuery } from "convex/react";
 * import { api } from "../convex/_generated/api";
 *
 * function TaskList() {
 *   // Reactively loads tasks, re-renders when data changes:
 *   const tasks = useQuery(api.tasks.list, { completed: false });
 *
 *   // Returns `undefined` while loading:
 *   if (tasks === undefined) return <div>Loading...</div>;
 *
 *   return tasks.map((task) => <div key={task._id}>{task.text}</div>);
 * }
 *
 * // Pass "skip" to conditionally disable the query:
 * function MaybeProfile({ userId }: { userId?: Id<"users"> }) {
 *   const profile = useQuery(
 *     api.users.get,
 *     userId ? { userId } : "skip",
 *   );
 *   // ...
 * }
 * ```
 *
 * @param query - a {@link server.FunctionReference} for the public query to run
 * like `api.dir1.dir2.filename.func`.
 * @param args - The arguments to the query function or the string `"skip"` if the
 * query should not be loaded.
 * @returns the result of the query. Returns `undefined` while loading.
 *
 * @see https://docs.convex.dev/client/react#fetching-data
 * @public
 */
export function useQuery<
  Query extends FunctionReference<"query"> | FunctionReference_future<"query">,
>(
  query: Query,
  ...args: OptionalRestArgsOrSkip<Query>
): FunctionReturnType<Query> | undefined {
  const skip = args[0] === "skip";
  const argsObject = args[0] === "skip" ? {} : parseArgs(args[0]);
  const queryReference =
    typeof query === "string"
      ? makeFunctionReference<"query", any, any>(query)
      : query;

  const queryName = getFunctionName(queryReference);

  const queries = useMemo(
    () =>
      skip
        ? ({} as RequestForQueries)
        : { query: { query: queryReference, args: argsObject } },
    // Stringify args so args that are semantically the same don't trigger a
    // rerender. Saves developers from adding `useMemo` on every args usage.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [JSON.stringify(convexToJson(argsObject)), queryName, skip],
  );

  const results = useQueries(queries);
  const result = results["query"];

  if (result instanceof Error) {
    throw result;
  }
  return result;
}

/**
 * Load a reactive query within a React component using an options object.
 *
 * This is an experimental form of {@link useQuery} that accepts a single
 * {@link UseQueryOptions} object instead of positional arguments.
 *
 * Consumers are expected to check the returned object `status` field to
 * make proper use of the result. If an error occurs, it will be present
 * in the result object unless `throwOnError` is `true`, in which case
 * the error will be thrown instead.
 *
 * @example
 * ```tsx
 * import { useQuery_experimental as useQuery } from "convex/react";
 * import { api } from "../convex/_generated/api";
 *
 * function TaskList() {
 *   const state = useQuery({ query: api.tasks.list, args: { completed: false } });
 *
 *   if (state.status === "pending") return <div>Loading...</div>;
 *   if (state.status === "error") return <div>Error: {state.error.message}</div>;
 *   return state.data.map((task) => <div key={task._id}>{task.text}</div>);
 * }
 * ```
 *
 * @param options - Query options. Pass `args: "skip"` to disable the query.
 * @returns the current query state as a {@link UseQueryResult} object.
 *
 * @see https://docs.convex.dev/client/react#fetching-data
 * @public
 */
export function useQuery_experimental<
  Query extends FunctionReference<"query"> | FunctionReference_future<"query">,
  ThrowOnError extends boolean = false,
>(
  options: UseQueryOptions<Query, ThrowOnError>,
): UseQueryResult<FunctionReturnType<Query>, ThrowOnError>;

export function useQuery_experimental<
  Query extends FunctionReference<"query"> | FunctionReference_future<"query">,
  ThrowOnError extends boolean = false,
>(
  options: UseQueryOptions<Query, ThrowOnError>,
): UseQueryResult<FunctionReturnType<Query>, false> {
  const throwOnError = options.throwOnError ?? false;
  const queryReference =
    typeof options.query === "string"
      ? (makeFunctionReference<"query", any, any>(options.query) as Query)
      : options.query;
  const skip = options.args === "skip";
  const argsObject = !skip
    ? parseArgs(options.args as Record<string, Value>)
    : {};

  const queryName = getFunctionName(queryReference);
  const queries = useMemo(
    () =>
      skip
        ? ({} as RequestForQueries)
        : { query: { query: queryReference, args: argsObject } },
    // Stringify args so args that are semantically the same don't trigger a
    // rerender. Saves developers from adding `useMemo` on every args usage.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [JSON.stringify(convexToJson(argsObject)), queryName, skip],
  );

  const results = useQueries(queries);
  const result = results["query"];

  if (result instanceof Error) {
    if (throwOnError) {
      throw result;
    }
    return {
      error: result,
      status: "error",
    };
  }

  if (result === undefined) {
    return {
      status: "pending",
    };
  }

  return {
    data: result,
    status: "success",
  };
}

/**
 * Construct a new {@link ReactMutation}.
 *
 * Returns a function that you can call to execute a Convex mutation. The
 * returned function is stable across renders (same reference identity), so
 * it can be safely used in dependency arrays and memoization.
 *
 * Mutations can optionally be configured with
 * [optimistic updates](https://docs.convex.dev/client/react/optimistic-updates)
 * for instant UI feedback.
 *
 * Throws an error if not used under {@link ConvexProvider}.
 *
 * @example
 * ```tsx
 * import { useMutation } from "convex/react";
 * import { api } from "../convex/_generated/api";
 *
 * function CreateTask() {
 *   const createTask = useMutation(api.tasks.create);
 *
 *   const handleClick = async () => {
 *     await createTask({ text: "New task" });
 *   };
 *
 *   return <button onClick={handleClick}>Add Task</button>;
 * }
 * ```
 *
 * @param mutation - A {@link server.FunctionReference} for the public mutation
 * to run like `api.dir1.dir2.filename.func`.
 * @returns The {@link ReactMutation} object with that name.
 *
 * @see https://docs.convex.dev/client/react#editing-data
 * @public
 */
export function useMutation<
  Mutation extends
    | FunctionReference<"mutation">
    | FunctionReference_future<"mutation">,
>(mutation: Mutation): ReactMutation<Mutation> {
  const mutationReference =
    typeof mutation === "string"
      ? makeFunctionReference<"mutation", any, any>(mutation)
      : mutation;

  const convex = useContext(ConvexContext);
  if (convex === undefined) {
    throw new Error(
      "Could not find Convex client! `useMutation` must be used in the React component " +
        "tree under `ConvexProvider`. Did you forget it? " +
        "See https://docs.convex.dev/quick-start#set-up-convex-in-your-react-app",
    );
  }
  return useMemo(
    () => createMutation(mutationReference, convex),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [convex, getFunctionName(mutationReference)],
  );
}

/**
 * Construct a new {@link ReactAction}.
 *
 * Returns a function that you can call to execute a Convex action. Actions
 * can call third-party APIs and perform side effects. The returned function
 * is stable across renders (same reference identity).
 *
 * **Error handling:** Actions can fail (e.g., if an external API is down).
 * Always wrap action calls in try/catch or handle the rejected promise.
 *
 * **Note:** In most cases, calling an action directly from a client is an
 * anti-pattern. Prefer having the client call a mutation that captures the
 * user's intent (by writing to the database) and then schedules the action
 * via `ctx.scheduler.runAfter`. This ensures the intent is durably recorded
 * even if the client disconnects.
 *
 * Throws an error if not used under {@link ConvexProvider}.
 *
 * @example
 * ```tsx
 * import { useAction } from "convex/react";
 * import { api } from "../convex/_generated/api";
 *
 * function GenerateSummary() {
 *   const generate = useAction(api.ai.generateSummary);
 *
 *   const handleClick = async () => {
 *     try {
 *       const summary = await generate({ text: "Some long text..." });
 *       console.log(summary);
 *     } catch (error) {
 *       console.error("Action failed:", error);
 *     }
 *   };
 *
 *   return <button onClick={handleClick}>Generate</button>;
 * }
 * ```
 *
 * @param action - A {@link server.FunctionReference} for the public action
 * to run like `api.dir1.dir2.filename.func`.
 * @returns The {@link ReactAction} object with that name.
 *
 * @see https://docs.convex.dev/functions/actions#calling-actions-from-clients
 * @public
 */
export function useAction<
  Action extends
    | FunctionReference<"action">
    | FunctionReference_future<"action">,
>(action: Action): ReactAction<Action> {
  const convex = useContext(ConvexContext);
  const actionReference =
    typeof action === "string"
      ? makeFunctionReference<"action", any, any>(action)
      : action;

  if (convex === undefined) {
    throw new Error(
      "Could not find Convex client! `useAction` must be used in the React component " +
        "tree under `ConvexProvider`. Did you forget it? " +
        "See https://docs.convex.dev/quick-start#set-up-convex-in-your-react-app",
    );
  }
  return useMemo(
    () => createAction(actionReference, convex),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [convex, getFunctionName(actionReference)],
  );
}

/**
 * React hook to get the current {@link ConnectionState} and subscribe to changes.
 *
 * This hook returns the current connection state and automatically rerenders
 * when any part of the connection state changes (e.g., when going online/offline,
 * when requests start/complete, etc.).
 *
 * The shape of ConnectionState may change in the future which may cause this
 * hook to rerender more frequently.
 *
 * Throws an error if not used under {@link ConvexProvider}.
 *
 * @returns The current {@link ConnectionState} with the Convex backend.
 *
 * @public
 */
export function useConvexConnectionState(): ConnectionState {
  const convex = useContext(ConvexContext);
  if (convex === undefined) {
    throw new Error(
      "Could not find Convex client! `useConvexConnectionState` must be used in the React component " +
        "tree under `ConvexProvider`. Did you forget it? " +
        "See https://docs.convex.dev/quick-start#set-up-convex-in-your-react-app",
    );
  }

  const getCurrentValue = useCallback(() => {
    return convex.connectionState();
  }, [convex]);

  const subscribe = useCallback(
    (callback: () => void) => {
      return convex.subscribeToConnectionState(() => {
        callback();
      });
    },
    [convex],
  );

  return useSubscription({ getCurrentValue, subscribe });
}

// When a function is called with a single argument that looks like a
// React SyntheticEvent it was likely called as an event handler.
function assertNotAccidentalArgument(value: any) {
  // these are properties of a React.SyntheticEvent
  // https://reactjs.org/docs/events.html
  if (
    typeof value === "object" &&
    value !== null &&
    "bubbles" in value &&
    "persist" in value &&
    "isDefaultPrevented" in value
  ) {
    throw new Error(
      `Convex function called with SyntheticEvent object. Did you use a Convex function as an event handler directly? Event handlers like onClick receive an event object as their first argument. These SyntheticEvent objects are not valid Convex values. Try wrapping the function like \`const handler = () => myMutation();\` and using \`handler\` in the event handler.`,
    );
  }
}
