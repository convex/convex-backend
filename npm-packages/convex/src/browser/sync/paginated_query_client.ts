/**
 * PaginatedQueryClient maps subscriptions to paginated queries to the
 * individual page queries and handles page splits.
 *
 * In order to process all modified queries, paginated and normal, in the same
 * synchronous call the PaginatedQueryClient transition should be used exclusively.
 *
 * Like the BaseConvexClient, this client is not Convex Function type-aware: it deals
 * with queries as functions that return Value, not the specific value.
 * Use a higher-level library to get types.
 */

import { Value } from "../../values/index.js";
import {
  PaginatedQueryToken,
  QueryToken,
  serializePaginatedPathAndArgs,
  canonicalizeUdfPath,
} from "./udf_path_utils.js";
import { BaseConvexClientInterface, Transition } from "./client.js";
import {
  PaginatedQueryResult,
  PaginationStatus,
  asPaginationResult,
} from "./pagination.js";
import { TS } from "./protocol.js";
import { Long } from "../../vendor/long.js";

type QueryPageKey = number;

/**
 * Represents a paginated query subscription with multiple pages.
 *
 * To know the order of pages it's necessary to consult `pageKeys`.
 * The pages in this array are active, they constitute a gapless sequence of results.
 * Some pages are not in this array: they may be waiting for data for a page split.
 */
type LocalPaginatedQuery = {
  token: PaginatedQueryToken;
  canonicalizedUdfPath: string;
  args: Record<string, Value>; // WITHOUT paginationOpts
  numSubscribers: number;
  options: { initialNumItems: number };
  nextPageKey: QueryPageKey;
  pageKeys: QueryPageKey[]; // These pages make up the active page queries.
  // Map page keys to their query subscriptions
  pageKeyToQuery: Map<
    QueryPageKey,
    { queryToken: QueryToken; unsubscribe: () => void; cursor: string | null }
  >;
  ongoingSplits: Map<QueryPageKey, [QueryPageKey, QueryPageKey]>;
  skip: boolean;

  // Give separate uses of the query separate identities,
  // which may be removed in the future to improve caching.
  id: number;
};

export interface SubscribeToPaginatedQueryOptions {
  initialNumItems: number;
  id: number;
}

type AnyPaginatedQueryResult = PaginatedQueryResult<Value>;

export type PaginatedQueryModification =
  | { kind: "Updated"; result: AnyPaginatedQueryResult | undefined }
  | { kind: "Removed" };

export type ExtendedTransition = Transition & {
  paginatedQueries: Array<{
    token: PaginatedQueryToken;
    modification: PaginatedQueryModification;
  }>;
};

/** @internal */
export class PaginatedQueryClient {
  private paginatedQuerySet: Map<PaginatedQueryToken, LocalPaginatedQuery> =
    new Map();
  // hold onto a real Transition so we can construct synthetic ones with that timestamp
  private lastTransitionTs: TS;

  constructor(
    private client: BaseConvexClientInterface,
    private onTransition: (transition: ExtendedTransition) => void,
  ) {
    // Nonsense initial value to construct synthetic Transitions
    this.lastTransitionTs = Long.fromNumber(0);
    this.client.addOnTransitionHandler((transition: Transition) =>
      this.onBaseTransition(transition),
    );
  }

  /**
   * Subscribe to a paginated query.
   *
   * @param name - The name of the paginated query function
   * @param args - Arguments for the query (excluding paginationOpts)
   * @param options - Pagination options including initialNumItems
   * @returns Object with paginatedQueryToken and unsubscribe function
   */
  subscribe(
    name: string,
    args: Record<string, Value>,
    options: SubscribeToPaginatedQueryOptions,
  ): {
    paginatedQueryToken: PaginatedQueryToken;
    unsubscribe: () => void;
  } {
    const canonicalizedUdfPath = canonicalizeUdfPath(name);
    // Note that only the expected options are included in the serialization.
    const token = serializePaginatedPathAndArgs(
      canonicalizedUdfPath,
      args,
      options,
    );

    const unsubscribe = () => this.removePaginatedQuerySubscriber(token);

    const existingEntry = this.paginatedQuerySet.get(token);
    if (existingEntry) {
      existingEntry.numSubscribers += 1;
      return {
        paginatedQueryToken: token,
        unsubscribe,
      };
    }

    // Create new paginated query
    this.paginatedQuerySet.set(token, {
      token,
      canonicalizedUdfPath,
      args,
      numSubscribers: 1,
      options: { initialNumItems: options.initialNumItems },
      nextPageKey: 0,
      pageKeys: [],
      pageKeyToQuery: new Map(),
      ongoingSplits: new Map(),
      skip: false,
      id: options.id,
    });

    this.addPageToPaginatedQuery(token, null, options.initialNumItems);

    return {
      paginatedQueryToken: token,
      unsubscribe,
    };
  }

  /**
   * Get current results for a paginated query based on local state.
   *
   * Throws an error when one of the pages has errored.
   */
  localQueryResult(
    name: string,
    args: Record<string, Value>,
    options: { initialNumItems: number; id: number },
  ): AnyPaginatedQueryResult | undefined {
    const canonicalizedUdfPath = canonicalizeUdfPath(name);
    const token = serializePaginatedPathAndArgs(
      canonicalizedUdfPath,
      args,
      options,
    );
    return this.localQueryResultByToken(token);
  }

  /**
   * @internal
   */
  localQueryResultByToken(
    token: PaginatedQueryToken,
  ): AnyPaginatedQueryResult | undefined {
    // undefined is probably the wrong value! Should be a real paginated query result for loading!
    // Butit's confusing why we'd ever get this, I guess some flows call localQueryResult before
    // subscribing? That's proabbly fair but is it consistent with the normal client?
    // What is the invariant here, will a token always exist? Or can a lookup occur at any time?
    const paginatedQuery = this.paginatedQuerySet.get(token);
    if (!paginatedQuery) {
      return undefined;
    }

    const activePages = this.activePageQueryTokens(paginatedQuery);
    if (activePages.length === 0) {
      return {
        results: [],
        status: "LoadingFirstPage",
        loadMore: (numItems: number) => {
          return this.loadMoreOfPaginatedQuery(token, numItems);
        },
      };
    }

    let allResults: Value[] = [];

    // Some page is loading (this isn't supposed to happen to any page but the last)
    let hasUndefined = false;
    let isDone = false;

    for (const pageToken of activePages) {
      // This throws, don't catch it, it should bubble up.
      // It might be a InvalidCursor Error. If it is, this query
      // should be reset (for now, use a new ID to ensure new state).
      //
      // In the future this might be caught and dealt with here but
      // an ID-based solution won't work here, ID is an intrinsic property
      // of this paginated query.
      const result = this.client.localQueryResultByToken(pageToken);

      if (result === undefined) {
        hasUndefined = true;
        isDone = false;
        continue;
      }

      const paginationResult = asPaginationResult(result);
      allResults = allResults.concat(paginationResult.page);
      // logic only relevant to the last page, we just happen to run it each time
      isDone = !!paginationResult.isDone;
    }

    let status: PaginationStatus;
    if (hasUndefined) {
      status = allResults.length === 0 ? "LoadingFirstPage" : "LoadingMore";
    } else if (isDone) {
      status = "Exhausted";
    } else {
      status = "CanLoadMore";
    }

    return {
      results: allResults,
      status,
      loadMore: (numItems: number) => {
        return this.loadMoreOfPaginatedQuery(token, numItems);
      },
    };
  }

  private onBaseTransition(transition: Transition) {
    const changedBaseTokens = transition.queries.map((q) => q.token);
    const changed = this.queriesContainingTokens(changedBaseTokens);

    let paginatedQueries: Array<{
      token: PaginatedQueryToken;
      modification: PaginatedQueryModification;
    }> = [];

    if (changed.length > 0) {
      this.processPaginatedQuerySplits(changed, (token) =>
        this.client.localQueryResultByToken(token),
      );

      paginatedQueries = changed.map((token) => ({
        token,
        modification: {
          kind: "Updated" as const,
          result: this.localQueryResultByToken(token),
        },
      }));
    }

    const extendedTransition: ExtendedTransition = {
      ...transition,
      paginatedQueries,
    };

    this.onTransition(extendedTransition);
  }

  /**
   * Load more items for a paginated query.
   *
   * This *always* causes a transition, the status of the query
   * has probably changed from "CanLoadMore" to "LoadingMore".
   * Data might have changed too: maybe a subscription to this page
   * query already exists (unlikely but possible) or this page query
   * has an optimistic update providing some initial data.
   *
   * @internal
   */
  private loadMoreOfPaginatedQuery(
    token: PaginatedQueryToken,
    numItems: number,
  ): boolean {
    this.mustGetPaginatedQuery(token);

    const lastPageToken = this.queryTokenForLastPageOfPaginatedQuery(token);
    const lastPageResult = this.client.localQueryResultByToken(lastPageToken);

    if (!lastPageResult) {
      // Still loading a page and concurrent loads are not allowed
      return false;
    }

    const paginationResult = asPaginationResult(lastPageResult);
    if (paginationResult.isDone) {
      // No more pages available
      return false;
    }

    this.addPageToPaginatedQuery(
      token,
      paginationResult.continueCursor,
      numItems,
    );

    const loadMoreTransition: ExtendedTransition = {
      timestamp: this.lastTransitionTs,
      reflectedMutations: [],
      queries: [],
      paginatedQueries: [
        {
          token,
          modification: {
            kind: "Updated" as const,
            result: this.localQueryResultByToken(token),
          },
        },
      ],
    };
    this.onTransition(loadMoreTransition);

    return true;
  }

  /**
   * @internal
   */
  private queriesContainingTokens(
    queryTokens: QueryToken[],
  ): PaginatedQueryToken[] {
    if (queryTokens.length === 0) {
      return [];
    }

    const changed: PaginatedQueryToken[] = [];
    const queryTokenSet = new Set(queryTokens);

    for (const [paginatedToken, paginatedQuery] of this.paginatedQuerySet) {
      for (const pageToken of this.allQueryTokens(paginatedQuery)) {
        if (queryTokenSet.has(pageToken)) {
          changed.push(paginatedToken);
          break;
        }
      }
    }

    return changed;
  }

  /**
   * @internal
   */
  private processPaginatedQuerySplits(
    changed: PaginatedQueryToken[],
    getResult: (token: QueryToken) => Value | undefined,
  ): void {
    for (const paginatedQueryToken of changed) {
      const paginatedQuery = this.mustGetPaginatedQuery(paginatedQueryToken);

      // These properties are all mutable, the destructure here is optional.
      const { ongoingSplits, pageKeyToQuery, pageKeys } = paginatedQuery;

      // Check for any completed splits
      for (const [pageKey, [splitKey1, splitKey2]] of ongoingSplits) {
        const bothNewPagesLoaded =
          getResult(pageKeyToQuery.get(splitKey1)!.queryToken) !== undefined &&
          getResult(pageKeyToQuery.get(splitKey2)!.queryToken) !== undefined;

        if (bothNewPagesLoaded) {
          this.completePaginatedQuerySplit(
            paginatedQuery,
            pageKey,
            splitKey1,
            splitKey2,
          );
        }
      }

      // Check each active page for splits needed
      for (const pageKey of pageKeys) {
        if (ongoingSplits.has(pageKey)) {
          continue; // Already splitting
        }

        const pageEntry = pageKeyToQuery.get(pageKey);
        if (!pageEntry) {
          throw new Error(`No page query for active pageKey ${pageKey}`);
        }
        const pageResult = getResult(pageEntry.queryToken);
        if (!pageResult) {
          continue;
        }
        const result = asPaginationResult(pageResult);

        // Check if this page needs splitting
        const shouldSplit =
          result.splitCursor &&
          (result.pageStatus === "SplitRecommended" ||
            result.pageStatus === "SplitRequired" ||
            // This client-driven page splitting condition will change in the future.
            result.page.length > paginatedQuery.options.initialNumItems * 2);

        if (shouldSplit) {
          this.splitPaginatedQueryPage(
            paginatedQuery,
            pageKey,
            pageEntry.cursor,
            result.splitCursor!, // we just checked
            result.continueCursor,
          );
        }
      }
    }
  }

  private splitPaginatedQueryPage(
    paginatedQuery: LocalPaginatedQuery,
    pageKey: QueryPageKey,
    // Where the page being split started.
    startCursor: string | null,
    // The split position - will be end of the first split page and the start
    // of the second.
    splitCursor: string,
    // Where the page being split ended.
    continueCursor: string | null,
  ): void {
    const splitKey1 = paginatedQuery.nextPageKey++;
    const splitKey2 = paginatedQuery.nextPageKey++;

    const paginationOpts: Value = {
      numItems: paginatedQuery.options.initialNumItems,
      id: paginatedQuery.id,
    };

    // First split page: same cursor as the original page, ending at splitCursor
    const firstSubscription = this.client.subscribe(
      paginatedQuery.canonicalizedUdfPath,
      {
        ...paginatedQuery.args,
        paginationOpts: {
          ...paginationOpts,
          cursor: startCursor,
          endCursor: splitCursor,
        },
      },
    );
    paginatedQuery.pageKeyToQuery.set(splitKey1, {
      ...firstSubscription,
      cursor: startCursor,
    });

    // Second split page: cursor starts at splitCursor, endCursor is the original continueCursor
    const secondSubscription = this.client.subscribe(
      paginatedQuery.canonicalizedUdfPath,
      {
        ...paginatedQuery.args,
        paginationOpts: {
          ...paginationOpts,
          cursor: splitCursor,
          endCursor: continueCursor,
        },
      },
    );
    paginatedQuery.pageKeyToQuery.set(splitKey2, {
      ...secondSubscription,
      cursor: splitCursor,
    });

    paginatedQuery.ongoingSplits.set(pageKey, [splitKey1, splitKey2]);
  }

  /**
   * @internal
   */
  private addPageToPaginatedQuery(
    token: PaginatedQueryToken,
    continueCursor: string | null,
    numItems: number,
  ): { queryToken: QueryToken; unsubscribe: () => void } {
    const paginatedQuery = this.mustGetPaginatedQuery(token);
    const pageKey = paginatedQuery.nextPageKey++;

    const paginationOpts: Value = {
      cursor: continueCursor,
      numItems,
      id: paginatedQuery.id,
    };

    const pageArgs = {
      ...paginatedQuery.args,
      paginationOpts,
    };

    const subscription = this.client.subscribe(
      paginatedQuery.canonicalizedUdfPath,
      pageArgs,
    );

    paginatedQuery.pageKeys.push(pageKey);
    paginatedQuery.pageKeyToQuery.set(pageKey, {
      ...subscription,
      cursor: continueCursor,
    });
    return subscription;
  }

  private removePaginatedQuerySubscriber(token: PaginatedQueryToken): void {
    const paginatedQuery = this.paginatedQuerySet.get(token);
    if (!paginatedQuery) {
      return;
    }

    paginatedQuery.numSubscribers -= 1;
    if (paginatedQuery.numSubscribers > 0) {
      return;
    }

    // Remove all page subscriptions by calling their unsubscribe callbacks
    for (const subscription of paginatedQuery.pageKeyToQuery.values()) {
      subscription.unsubscribe();
    }

    this.paginatedQuerySet.delete(token);
  }

  private completePaginatedQuerySplit(
    paginatedQuery: LocalPaginatedQuery,
    pageKey: QueryPageKey,
    splitKey1: QueryPageKey,
    splitKey2: QueryPageKey,
  ): void {
    const originalQuery = paginatedQuery.pageKeyToQuery.get(pageKey)!;
    paginatedQuery.pageKeyToQuery.delete(pageKey);
    const pageIndex = paginatedQuery.pageKeys.indexOf(pageKey);
    paginatedQuery.pageKeys.splice(pageIndex, 1, splitKey1, splitKey2);
    paginatedQuery.ongoingSplits.delete(pageKey);
    originalQuery.unsubscribe();
  }

  /** The query tokens for all active pages, in result order */
  private activePageQueryTokens(
    paginatedQuery: LocalPaginatedQuery,
  ): QueryToken[] {
    return paginatedQuery.pageKeys.map(
      (pageKey) => paginatedQuery.pageKeyToQuery.get(pageKey)!.queryToken,
    );
  }

  private allQueryTokens(paginatedQuery: LocalPaginatedQuery): QueryToken[] {
    return Array.from(paginatedQuery.pageKeyToQuery.values()).map(
      (sub) => sub.queryToken,
    );
  }

  private queryTokenForLastPageOfPaginatedQuery(
    token: PaginatedQueryToken,
  ): QueryToken {
    const paginatedQuery = this.mustGetPaginatedQuery(token);
    const lastPageKey =
      paginatedQuery.pageKeys[paginatedQuery.pageKeys.length - 1];
    if (lastPageKey === undefined) {
      throw new Error(`No pages for paginated query ${token}`);
    }
    return paginatedQuery.pageKeyToQuery.get(lastPageKey)!.queryToken;
  }

  private mustGetPaginatedQuery(
    token: PaginatedQueryToken,
  ): LocalPaginatedQuery {
    const paginatedQuery = this.paginatedQuerySet.get(token);
    if (!paginatedQuery) {
      throw new Error("paginated query no longer exists for token " + token);
    }
    return paginatedQuery;
  }
}
