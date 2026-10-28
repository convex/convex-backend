import { convexToJson, Value } from "../values/index.js";
import { PaginatedWatch, Watch } from "./client.js";
import { QueryJournal } from "../browser/sync/protocol.js";
import {
  getFunctionName,
  FunctionReference,
  FunctionReference_future,
} from "../server/api.js";
import { PaginatedQueryResult } from "../browser/sync/pagination.js";
import { SubscribeToPaginatedQueryOptions } from "../browser/sync/paginated_query_client.js";

type Identifier = string;

type RequestForQueriesCompat = Record<
  Identifier,
  {
    query: FunctionReference<"query"> | FunctionReference_future<"query">;
    args: Record<string, Value>;
    paginationOptions?: SubscribeToPaginatedQueryOptions;
  }
>;

type QueryInfo = {
  query: FunctionReference<"query"> | FunctionReference_future<"query">;
  args: Record<string, Value>;
  watch: Watch<Value> | PaginatedWatch<Value>;
  unsubscribe: () => void;
  paginationOptions?: SubscribeToPaginatedQueryOptions;
};

export interface CreateWatch {
  (
    query: FunctionReference<"query"> | FunctionReference_future<"query">,
    args: Record<string, Value>,
    options: {
      journal?: QueryJournal;
      // Just the existence of this option makes this a paginated query
      paginationOptions?: SubscribeToPaginatedQueryOptions;
    },
  ): Watch<Value> | PaginatedWatch<Value>;
}

/**
 * A class for observing the results of multiple queries at the same time.
 *
 * Any time the result of a query changes, the listeners are notified.
 */
export class QueriesObserver {
  public createWatch: CreateWatch;
  private queries: Record<Identifier, QueryInfo>;
  private listeners: Set<() => void>;

  constructor(createWatch: CreateWatch) {
    this.createWatch = createWatch;
    this.queries = {};
    this.listeners = new Set();
  }

  setQueries(newQueries: RequestForQueriesCompat) {
    // Add the new queries before unsubscribing from the old ones so that
    // the deduping in the `ConvexReactClient` can help if there are duplicates.
    for (const identifier of Object.keys(newQueries)) {
      const { query, args, paginationOptions } = newQueries[identifier];
      // Might throw
      getFunctionName(query);

      if (this.queries[identifier] === undefined) {
        // No existing query => add it.
        this.addQuery(
          identifier,
          query,
          args,
          paginationOptions ? { paginationOptions } : {},
        );
      } else {
        const existingInfo = this.queries[identifier];

        if (
          getFunctionName(query) !== getFunctionName(existingInfo.query) ||
          JSON.stringify(convexToJson(args)) !==
            JSON.stringify(convexToJson(existingInfo.args)) ||
          JSON.stringify(paginationOptions) !==
            JSON.stringify(existingInfo.paginationOptions)
        ) {
          // Existing query that doesn't match => remove the old and add the new.
          this.removeQuery(identifier);
          this.addQuery(
            identifier,
            query,
            args,
            paginationOptions ? { paginationOptions } : {},
          );
        }
      }
    }

    // Prune all the existing queries that we no longer need.
    for (const identifier of Object.keys(this.queries)) {
      if (newQueries[identifier] === undefined) {
        this.removeQuery(identifier);
      }
    }
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  getLocalResults(
    queries: RequestForQueriesCompat,
  ): Record<
    Identifier,
    Value | undefined | Error | PaginatedQueryResult<Value>
  > {
    const result: Record<
      Identifier,
      Value | Error | undefined | PaginatedQueryResult<Value>
    > = {};
    for (const identifier of Object.keys(queries)) {
      const { query, args } = queries[identifier];
      const paginationOptions = queries[identifier].paginationOptions;

      // Might throw
      getFunctionName(query);

      // Note: We're not gonna watch, we could save some allocations
      // by getting a reference to the client directly instead.
      const watch = this.createWatch(
        query,
        args,
        paginationOptions ? { paginationOptions } : {},
      );

      let value: Value | undefined | Error | PaginatedQueryResult<Value>;
      try {
        value = watch.localQueryResult();
      } catch (e) {
        // Only collect instances of `Error` because thats how callers
        // will distinguish errors from normal results.
        if (e instanceof Error) {
          value = e;
        } else {
          throw e;
        }
      }
      result[identifier] = value;
    }
    return result;
  }

  setCreateWatch(createWatch: CreateWatch) {
    this.createWatch = createWatch;
    // If we have a new watch, we might be using a new Convex client.
    // Recreate all the watches being careful to preserve the journals.
    for (const identifier of Object.keys(this.queries)) {
      const { query, args, watch, paginationOptions } =
        this.queries[identifier];
      const journal = "journal" in watch ? watch.journal() : undefined;
      this.removeQuery(identifier);
      this.addQuery(identifier, query, args, {
        ...(journal ? { journal } : []),
        ...(paginationOptions ? { paginationOptions } : {}),
      });
    }
  }

  destroy() {
    for (const identifier of Object.keys(this.queries)) {
      this.removeQuery(identifier);
    }
    this.listeners = new Set();
  }

  private addQuery(
    identifier: Identifier,
    query: FunctionReference<"query"> | FunctionReference_future<"query">,
    args: Record<string, Value>,
    {
      paginationOptions,
      journal,
    }: {
      paginationOptions?: SubscribeToPaginatedQueryOptions;
      journal?: QueryJournal;
    },
  ) {
    if (this.queries[identifier] !== undefined) {
      throw new Error(
        `Tried to add a new query with identifier ${identifier} when it already exists.`,
      );
    }
    const watch = this.createWatch(query, args, {
      ...(journal ? { journal } : []),
      ...(paginationOptions ? { paginationOptions } : {}),
    });
    const unsubscribe = watch.onUpdate(() => this.notifyListeners());
    this.queries[identifier] = {
      query,
      args,
      watch,
      unsubscribe,
      ...(paginationOptions ? { paginationOptions } : {}),
    };
  }

  private removeQuery(identifier: Identifier) {
    const info = this.queries[identifier];
    if (info === undefined) {
      throw new Error(`No query found with identifier ${identifier}.`);
    }
    info.unsubscribe();
    delete this.queries[identifier];
  }

  private notifyListeners(): void {
    for (const listener of this.listeners) {
      listener();
    }
  }
}
