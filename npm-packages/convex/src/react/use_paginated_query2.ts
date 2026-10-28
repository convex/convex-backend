import { useState } from "react";

import {
  FunctionReference,
  FunctionReturnType,
  getFunctionName,
} from "../server/api.js";
import {
  PaginatedQueryReference,
  PaginatedQueryArgs,
  PaginatedQueryItem,
  UsePaginatedQueryReturnType,
} from "./use_paginated_query.js";
import { convexToJson, Value } from "../values/value.js";
import { useQueries } from "./use_queries.js";
import { PaginatedQueryResult } from "../browser/sync/pagination.js";
import { SubscribeToPaginatedQueryOptions } from "../browser/sync/paginated_query_client.js";
import { ConvexError } from "../values/errors.js";
import { useConvex } from "./client.js";

/**
 * Options for object-form {@link usePaginatedQuery_experimental}.
 *
 * @public
 */
export type UsePaginatedQueryOptions<
  Query extends PaginatedQueryReference,
  ThrowOnError extends boolean = false,
> = {
  query: Query;
  args: PaginatedQueryArgs<Query> | "skip";
  initialNumItems: number;
  /**
   * When `true` (default for positional form), errors are thrown and caught
   * by an error boundary. When `false` (default for object form), errors are
   * returned as `{ status: "Error", error: Error }` instead of being thrown.
   */
  throwOnError?: ThrowOnError;
};

/**
 * Return type of the object-form {@link usePaginatedQuery_experimental} overload.
 *
 * Uses lowercase query status (`"pending" | "success" | "error"`) and a
 * `canLoadMore` boolean instead of the TitleCase pagination status strings
 * used by the positional form.
 *
 * @public
 */
export type UsePaginatedQueryObjectReturnType<
  Query extends PaginatedQueryReference,
  ThrowOnError extends boolean = false,
> =
  | {
      data: PaginatedQueryItem<Query>[] | undefined;
      status: "pending";
      canLoadMore: false;
      isLoading: true;
      error: undefined;
      loadMore: (numItems: number) => void;
    }
  | {
      data: PaginatedQueryItem<Query>[];
      status: "success";
      canLoadMore: boolean;
      isLoading: false;
      error: undefined;
      loadMore: (numItems: number) => void;
    }
  | (ThrowOnError extends true
      ? never
      : {
          data: PaginatedQueryItem<Query>[];
          status: "error";
          canLoadMore: false;
          isLoading: false;
          error: Error;
          loadMore: (numItems: number) => void;
        });

type UsePaginatedQueryState = {
  query: FunctionReference<"query">;
  args: Record<string, Value>;
  id: number;
  queries: {
    paginatedQuery?: {
      query: FunctionReference<"query">;
      args: Record<string, Value>;
      paginationOptions: SubscribeToPaginatedQueryOptions;
    };
  };
  skip: boolean;
};

/**
 * Experimental new usePaginatedQuery implementation that will replace the current one
 * in the future.
 *
 * Load data reactively from a paginated query to a create a growing list.
 *
 * This is an alternate implementation that relies on new client pagination logic.
 *
 * This can be used to power "infinite scroll" UIs.
 *
 * This hook must be used with public query references that match
 * {@link PaginatedQueryReference}.
 *
 * `usePaginatedQuery` concatenates all the pages of results into a single list
 * and manages the continuation cursors when requesting more items.
 *
 * Example usage:
 * ```typescript
 * const { results, status, isLoading, loadMore } = usePaginatedQuery(
 *   api.messages.list,
 *   { channel: "#general" },
 *   { initialNumItems: 5 }
 * );
 * ```
 *
 * If the query reference or arguments change, the pagination state will be reset
 * to the first page. Similarly, if any of the pages result in an InvalidCursor
 * error or an error associated with too much data, the pagination state will also
 * reset to the first page.
 *
 * To learn more about pagination, see [Paginated Queries](https://docs.convex.dev/database/pagination).
 *
 * @param query - A FunctionReference to the public query function to run.
 * @param args - The arguments object for the query function, excluding
 * the `paginationOpts` property. That property is injected by this hook.
 * @param options - An object specifying the `initialNumItems` to be loaded in
 * the first page.
 * @returns A {@link UsePaginatedQueryResult} that includes the currently loaded
 * items, the status of the pagination, and a `loadMore` function.
 *
 * @public
 */
export function usePaginatedQuery_experimental<
  Query extends PaginatedQueryReference,
>(
  query: Query,
  args: PaginatedQueryArgs<Query> | "skip",
  // Future options this hook might accept:
  // - maximumRowsRead
  // - maximumBytesRead
  // - a cursor for where to start? although probably no endCursor
  options: { initialNumItems: number },
): UsePaginatedQueryReturnType<Query>;

/**
 * Experimental new usePaginatedQuery implementation that accepts an options object
 * rather than positional arguments.
 *
 * @param options - A {@link UsePaginatedQueryOptions} object including `query` and `args`.
 * @returns A {@link UsePaginatedQueryObjectReturnType} object with `data`, `status`,
 * `canLoadMore`, `isLoading`, `error`, and `loadMore`. `status` is `"pending"` while
 * loading, `"success"` when data is available, or `"error"` if the query threw.
 * When `throwOnError` is `true`, the `"error"` status is excluded from the return
 * type since errors will be thrown instead.
 * `canLoadMore` is `true` only when idle and more pages exist.
 *
 * @public
 */
export function usePaginatedQuery_experimental<
  Query extends PaginatedQueryReference,
  ThrowOnError extends boolean = false,
>(
  options: UsePaginatedQueryOptions<Query, ThrowOnError>,
): UsePaginatedQueryObjectReturnType<Query, ThrowOnError>;

export function usePaginatedQuery_experimental<
  Query extends PaginatedQueryReference,
>(
  queryOrOptions: Query | UsePaginatedQueryOptions<Query>,
  args?: PaginatedQueryArgs<Query> | "skip",
  // Future options this hook might accept:
  // - maximumRowsRead
  // - maximumBytesRead
  // - a cursor for where to start? although probably no endCursor
  options?: { initialNumItems: number },
):
  | UsePaginatedQueryReturnType<Query>
  | UsePaginatedQueryObjectReturnType<Query> {
  const isObjectOptions =
    typeof queryOrOptions === "object" &&
    queryOrOptions !== null &&
    "query" in queryOrOptions;

  const query = isObjectOptions ? queryOrOptions.query : queryOrOptions;
  const queryArgs = isObjectOptions ? queryOrOptions.args : args;
  const throwOnError = isObjectOptions
    ? (queryOrOptions.throwOnError ?? false)
    : true;
  const initialOptions = isObjectOptions
    ? { initialNumItems: queryOrOptions.initialNumItems }
    : options;

  if (
    typeof initialOptions?.initialNumItems !== "number" ||
    initialOptions.initialNumItems < 0
  ) {
    throw new Error(
      `\`options.initialNumItems\` must be a positive number. Received \`${initialOptions?.initialNumItems}\`.`,
    );
  }
  const skip = queryArgs === "skip";
  const argsObject = skip ? {} : queryArgs;

  const convexClient = useConvex();
  const logger = convexClient.logger;

  // The identity of createInitialState changes each time!
  const createInitialState: () => UsePaginatedQueryState = () => {
    const id = nextPaginationId();
    return {
      query,
      args: argsObject as Record<string, Value>,
      id,
      // Queries will contain zero or one queries forever.
      queries: skip
        ? ({} as UsePaginatedQueryState["queries"])
        : {
            paginatedQuery: {
              query,
              args: {
                ...argsObject,
              },
              paginationOptions: {
                initialNumItems: initialOptions.initialNumItems,
                id,
              },
            },
          },
      skip,
    };
  };

  const [state, setState] =
    useState<UsePaginatedQueryState>(createInitialState);

  // `currState` is the state that we'll render based on.
  let currState = state;
  // New function, args, or skip? New paginated query!
  if (
    getFunctionName(query) !== getFunctionName(state.query) ||
    JSON.stringify(convexToJson(argsObject as Value)) !==
      JSON.stringify(convexToJson(state.args)) ||
    skip !== state.skip
  ) {
    currState = createInitialState();
    setState(currState);
  }
  // currState.queries is just a single query; we use useQueries
  // because it's the lower-level ook sthat supports pagination options.
  const resultsObject = useQueries(currState.queries);

  // skip
  if (!("paginatedQuery" in resultsObject)) {
    if (!skip) {
      throw new Error("Why is it missing?");
    }
    const internalResult = {
      results: [] as FunctionReturnType<Query>["page"],
      status: "LoadingFirstPage" as const,
      isLoading: true as const,
      loadMore: function skipNOP(_numItems: number) {
        return false;
      },
    };
    if (isObjectOptions) {
      return reshapeToObjectForm2(
        internalResult,
      ) as unknown as UsePaginatedQueryObjectReturnType<Query>;
    }
    return internalResult as unknown as UsePaginatedQueryReturnType<Query>;
  }
  const result = resultsObject.paginatedQuery as
    | PaginatedQueryResult<FunctionReturnType<Query>["page"][number]>
    | Error;

  // TODO this is a weird mix of responsibilities:
  // - is it the hook's job to render the initial loading state?
  // - or is it the paginated query's job to render the approproate loading state?
  // It comes back to why we'd ever get undefined when asking about a query; have we not yet called subscribe for it?
  if (result === undefined) {
    const internalResult = {
      results: [] as FunctionReturnType<Query>["page"],
      loadMore: () => false,
      isLoading: true as const,
      status: "LoadingFirstPage" as const,
    };
    if (isObjectOptions) {
      return reshapeToObjectForm2(
        internalResult,
      ) as unknown as UsePaginatedQueryObjectReturnType<Query>;
    }
    return internalResult as unknown as UsePaginatedQueryReturnType<Query>;
  }

  if (result instanceof Error) {
    if (
      result.message.includes("InvalidCursor") ||
      (result instanceof ConvexError &&
        typeof result.data === "object" &&
        result.data?.isConvexSystemError === true &&
        result.data?.paginationError === "InvalidCursor")
    ) {
      // - InvalidCursor: If the cursor is invalid, probably the paginated
      // database query was data-dependent and changed underneath us. The
      // cursor in the params or journal no longer matches the current
      // database query.

      // In all cases, we want to restart pagination to throw away all our
      // existing cursors.
      logger.warn(
        "usePaginatedQuery hit error, resetting pagination state: " +
          result.message,
      );
      setState(createInitialState);
      const internalResult = {
        results: [] as FunctionReturnType<Query>["page"],
        loadMore: () => false,
        isLoading: true as const,
        status: "LoadingFirstPage" as const,
      };
      if (isObjectOptions) {
        return reshapeToObjectForm2(
          internalResult,
        ) as unknown as UsePaginatedQueryObjectReturnType<Query>;
      }
      return internalResult as unknown as UsePaginatedQueryReturnType<Query>;
    } else {
      if (throwOnError) {
        throw result;
      }
      const internalResult = {
        results: [] as FunctionReturnType<Query>["page"],
        loadMore: () => false,
        isLoading: false as const,
        status: "Error" as const,
        error: result,
      };
      if (isObjectOptions) {
        return reshapeToObjectForm2(
          internalResult,
        ) as unknown as UsePaginatedQueryObjectReturnType<Query>;
      }
      return internalResult as unknown as UsePaginatedQueryReturnType<Query>;
    }
  }

  const internalResult = {
    ...result,
    loadMore: (num: number) => {
      return result.loadMore(num);
    },
    isLoading:
      result.status === "LoadingFirstPage"
        ? (true as const)
        : result.status === "LoadingMore"
          ? (true as const)
          : (false as const),
  };
  if (isObjectOptions) {
    return reshapeToObjectForm2(
      internalResult,
    ) as unknown as UsePaginatedQueryObjectReturnType<Query>;
  }
  return internalResult as unknown as UsePaginatedQueryReturnType<Query>;
}

/**
 * Reshape the internal TitleCase pagination result into the object-form
 * return type with lowercase `status`, `canLoadMore`, and `data`.
 */
function reshapeToObjectForm2<Item>(internal: {
  results: Item[];
  status: string;
  isLoading: boolean;
  loadMore: (...args: any[]) => any;
  error?: Error;
}) {
  const { results, loadMore } = internal;
  if (internal.status === "Error" && "error" in internal) {
    return {
      data: results,
      status: "error" as const,
      canLoadMore: false as const,
      isLoading: false as const,
      error: internal.error,
      loadMore,
    };
  }
  if (
    internal.status === "LoadingFirstPage" ||
    internal.status === "LoadingMore"
  ) {
    return {
      data: internal.status === "LoadingFirstPage" ? undefined : results,
      status: "pending" as const,
      canLoadMore: false as const,
      isLoading: true as const,
      error: undefined,
      loadMore,
    };
  }
  // CanLoadMore or Exhausted
  return {
    data: results,
    status: "success" as const,
    canLoadMore: internal.status === "CanLoadMore",
    isLoading: false as const,
    error: undefined,
    loadMore,
  };
}

let paginationId = 0;
/**
 * See ./use_paginated_query for the purpose, but we may be able to get rid of this soon.
 *
 * @returns The pagination ID.
 */
function nextPaginationId(): number {
  paginationId++;
  return paginationId;
}

/**
 * Reset pagination id for tests only, so tests know what it is.
 */
export function resetPaginationId() {
  paginationId = 0;
}
