import { cursorForSyncObject } from "../../server/resolvers";

import {
  Key,
  IndexKey,
  ConvexSubscriptionId,
  IndexRangeBounds,
  IndexPrefix,
  IndexRangeRequest,
} from "../../shared/types";
import { compareKeys, maximalKey, minimalKey } from "../../shared/compare";
import { GenericDocument, SchemaDefinition } from "convex/server";
import { CopyOnWriteLocalStore } from "./localStore";
import { compareValues } from "convex/values";

export type SerializedRangeExpression = {
  type: "Eq" | "Gt" | "Gte" | "Lt" | "Lte";
  fieldPath: string;
  value: any;
};

// maximal key
// [conversationId, max number]
// [next value after conversationId]
// { kind: "successor", key: [conversationId] }

export const indexRangeUnbounded: IndexRangeBounds = {
  lowerBound: [] as unknown as IndexPrefix,
  lowerBoundInclusive: true,
  upperBound: [] as unknown as IndexPrefix,
  upperBoundInclusive: true,
};

export type SubscriptionState =
  | {
      kind: "loading";
    }
  | {
      kind: "loaded";
      results: GenericDocument[];
    };

export const LOG2_PAGE_SIZE = 4;

export class SingleIndexRangeExecutor {
  constructor(
    private rangeRequest: IndexRangeRequest,
    private schema: SchemaDefinition<any, any>,
    private localStore: CopyOnWriteLocalStore,
  ) {}

  tryFulfill():
    | {
        state: "fulfilled";
        pageSubscriptionIds: ConvexSubscriptionId[];
        results: GenericDocument[];
      }
    | {
        state: "waitingOnLoadingPage";
        loadingPageSubscriptionIds: ConvexSubscriptionId[];
      }
    | {
        state: "needsMorePages";
        existingPageSubscriptionIds: ConvexSubscriptionId[];
        targetKey: Key;
      } {
    const loadingPageSubscriptionIds = this.loadingPagesWithOverlap(
      this.rangeRequest.indexRangeBounds,
    );
    if (loadingPageSubscriptionIds.length > 0) {
      // Something is loading, so wait for that to finish before adding more pages.
      return {
        state: "waitingOnLoadingPage",
        loadingPageSubscriptionIds,
      };
    }

    const anchorKey = this.getAnchorKey();
    const firstPageSubscriptionId = this.getLoadedPageContaining(anchorKey);
    if (firstPageSubscriptionId === null) {
      return {
        state: "needsMorePages",
        existingPageSubscriptionIds: [],
        targetKey: anchorKey,
      };
    }

    const pageSubscriptionIds = [
      firstPageSubscriptionId,
      ...this.getConsecutiveLoadedPagesInDirection(
        firstPageSubscriptionId,
        this.rangeRequest.order,
      ),
    ];
    const subscribedPageSubscriptionIds = [];
    const documents: GenericDocument[] = [];
    for (const pageSubscriptionId of pageSubscriptionIds) {
      const page = this.localStore.getPage(
        this.rangeRequest.tableName,
        this.rangeRequest.indexName,
        pageSubscriptionId,
      );
      if (page === null) {
        throw new Error(`page not found: ${pageSubscriptionId}`);
      }
      if (page.state.kind === "loading") {
        throw new Error(`page is loading: ${pageSubscriptionId}`);
      }
      const pageResult = page.state.value;
      const pageInRange = pageResult.results.filter((d: GenericDocument) => {
        const cursor = this.keyForSyncObject(d);
        return (
          compareKeys(cursor, minimalKey(this.rangeRequest.indexRangeBounds)) >=
            0 &&
          compareKeys(cursor, maximalKey(this.rangeRequest.indexRangeBounds)) <=
            0
        );
      });
      if (this.rangeRequest.order === "desc") {
        pageInRange.reverse();
      }
      documents.push(...pageInRange);
      subscribedPageSubscriptionIds.push(pageSubscriptionId);
      const isEndOfQueriedRange =
        this.rangeRequest.order === "asc"
          ? compareKeys(
              pageResult.upperBound,
              maximalKey(this.rangeRequest.indexRangeBounds),
            ) >= 0
          : compareKeys(
              pageResult.lowerBound,
              minimalKey(this.rangeRequest.indexRangeBounds),
            ) <= 0;
      if (isEndOfQueriedRange || documents.length >= this.rangeRequest.count) {
        return {
          state: "fulfilled",
          pageSubscriptionIds: subscribedPageSubscriptionIds,
          results: documents.slice(0, this.rangeRequest.count),
        };
      }
    }

    const lastPage = this.localStore.getPage(
      this.rangeRequest.tableName,
      this.rangeRequest.indexName,
      pageSubscriptionIds[pageSubscriptionIds.length - 1],
    );

    if (lastPage === null || lastPage.state.kind === "loading") {
      throw new Error(`lastPage not found or still loading`);
    }
    const lastPageResult = lastPage.state.value;

    let nextTargetKey: Key;
    if (this.rangeRequest.order === "asc") {
      nextTargetKey = {
        kind: "successor",
        value: lastPageResult.upperBound.value as IndexPrefix,
      };
    } else {
      nextTargetKey = {
        kind: "exact",
        // TODO -- why will this always be an IndexKey?
        value: lastPageResult.lowerBound.value as unknown as IndexKey,
      };
    }
    return {
      state: "needsMorePages",
      existingPageSubscriptionIds: subscribedPageSubscriptionIds,
      targetKey: nextTargetKey,
    };
  }

  loadingPagesWithOverlap(
    _indexRangeBounds: IndexRangeBounds,
  ): ConvexSubscriptionId[] {
    // TODO
    return this.getOrderedPages()
      .filter((p) => p.state.kind === "loading")
      .map((p) => p.convexSubscriptionId);
  }

  getOrderedPages() {
    return this.localStore.getOrderedPages(
      this.rangeRequest.tableName,
      this.rangeRequest.indexName,
    );
  }

  getAnchorKey(): Key {
    if (this.rangeRequest.order === "asc") {
      return minimalKey(this.rangeRequest.indexRangeBounds);
    } else {
      return maximalKey(this.rangeRequest.indexRangeBounds);
    }
  }

  getLoadedPageContaining(key: Key): ConvexSubscriptionId | null {
    const page = this.getOrderedPages().find((p) => {
      if (p.state.kind === "loading") {
        return false;
      }
      const pageResult = p.state.value;
      return (
        compareKeys(key, pageResult.lowerBound) >= 0 &&
        compareKeys(key, pageResult.upperBound) <= 0
      );
    });
    return page ? page.convexSubscriptionId : null;
  }

  getConsecutiveLoadedPagesInDirection(
    initialPageSubscriptionId: ConvexSubscriptionId,
    direction: "asc" | "desc",
  ): ConvexSubscriptionId[] {
    const orderedPages = this.getOrderedPages();
    const initialPageIndex = orderedPages.findIndex(
      (p) => p.convexSubscriptionId === initialPageSubscriptionId,
    );
    if (initialPageIndex === -1) {
      throw new Error(`initialPage not found: ${initialPageSubscriptionId}`);
    }
    const initialPage = orderedPages[initialPageIndex];
    if (initialPage.state.kind === "loading") {
      throw new Error("initialPage is loading");
    }
    // console.log(
    //   "getConsecutiveLoadedPagesInDirection",
    //   initialPageSubscriptionId,
    //   initialPage,
    //   direction,
    // );
    const result: ConvexSubscriptionId[] = [];
    const initialPageResult = initialPage.state.value;
    let pageBreak =
      direction === "asc"
        ? initialPageResult.upperBound
        : initialPageResult.lowerBound;
    const pageIncrement = direction === "asc" ? 1 : -1;

    let currentPageIndex = initialPageIndex + pageIncrement;
    while (0 <= currentPageIndex && currentPageIndex < orderedPages.length) {
      const page = orderedPages[currentPageIndex];
      if (page.state.kind === "loading") {
        break;
      }
      const pageResult = page.state.value;
      const nextPageBreak =
        direction === "asc" ? pageResult.lowerBound : pageResult.upperBound;
      // Whichever bound is the lower bound will be exclusive (kind: "successor") and whichever is the upper bound will be inclusive
      // (kind: "exact") so compare their values instead of comparing the keys directly
      // console.log(
      //   "#### compareValues",
      //   pageBreak,
      //   nextPageBreak,
      //   compareValues(pageBreak.value as any, nextPageBreak.value as any),
      // );
      if (
        page.state.kind === "loaded" &&
        compareValues(pageBreak.value as any, nextPageBreak.value as any) !== 0
      ) {
        break;
      }
      result.push(page.convexSubscriptionId);
      pageBreak =
        direction === "asc" ? pageResult.upperBound : pageResult.lowerBound;
      currentPageIndex += pageIncrement;
    }
    return result;
  }

  keyForSyncObject(doc: GenericDocument): { kind: "exact"; value: IndexKey } {
    const cursor = cursorForSyncObject(
      this.schema,
      this.rangeRequest.tableName,
      this.rangeRequest.indexName,
      doc,
    );
    return {
      kind: "exact",
      value: cursor.value,
    };
  }
}
