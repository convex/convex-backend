import {
  DocumentByInfo,
  GenericTableInfo,
  IndexNames,
  NamedIndex,
  NamedSearchIndex,
  SearchIndexNames,
} from "./data_model.js";
import { ExpressionOrValue, FilterBuilder } from "./filter_builder.js";
import { IndexRange, IndexRangeBuilder } from "./index_range_builder.js";
import { PaginationResult, PaginationOptions } from "./pagination.js";
import { SearchFilter, SearchFilterBuilder } from "./search_filter_builder.js";

/**
 * The {@link QueryInitializer} interface is the entry point for building a {@link Query}
 * over a Convex database table.
 *
 * There are two types of queries:
 * 1. Full table scans: Queries created with {@link QueryInitializer.fullTableScan} which
 * iterate over all of the documents in the table in insertion order.
 * 2. Indexed Queries: Queries created with {@link QueryInitializer.withIndex} which iterate
 * over an index range in index order.
 *
 * For convenience, {@link QueryInitializer} extends the {@link Query} interface, implicitly
 * starting a full table scan.
 *
 * @public
 */
export interface QueryInitializer<
  TableInfo extends GenericTableInfo,
> extends Query<TableInfo> {
  /**
   * Query by reading all of the values out of this table.
   *
   * This query's cost is relative to the size of the entire table, so this
   * should only be used on tables that will stay very small (say between a few
   * hundred and a few thousand documents) and are updated infrequently.
   *
   * @returns - The {@link Query} that iterates over every document of the table.
   */
  fullTableScan(): Query<TableInfo>;

  /**
   * Query by reading documents from an index on this table.
   *
   * This query's cost is relative to the number of documents that match the
   * index range expression.
   *
   * Results will be returned in index order.
   *
   * To learn about indexes, see [Indexes](https://docs.convex.dev/using/indexes).
   *
   * @param indexName - The name of the index to query.
   * @param indexRange - An optional index range constructed with the supplied
   *  {@link IndexRangeBuilder}. An index range is a description of which
   * documents Convex should consider when running the query. If no index
   * range is present, the query will consider all documents in the index.
   * @returns - The query that yields documents in the index.
   */
  withIndex<IndexName extends IndexNames<TableInfo>>(
    indexName: IndexName,
    indexRange?: (
      q: IndexRangeBuilder<
        DocumentByInfo<TableInfo>,
        NamedIndex<TableInfo, IndexName>
      >,
    ) => IndexRange,
  ): Query<TableInfo>;

  /**
   * Query by running a full text search against a search index.
   *
   * Search queries must always search for some text within the index's
   * `searchField`. This query can optionally add equality filters for any
   * `filterFields` specified in the index.
   *
   * Documents will be returned in relevance order based on how well they
   * match the search text.
   *
   * To learn about full text search, see [Indexes](https://docs.convex.dev/text-search).
   *
   * @param indexName - The name of the search index to query.
   * @param searchFilter - A search filter expression constructed with the
   * supplied {@link SearchFilterBuilder}. This defines the full text search to run
   * along with equality filtering to run within the search index.
   * @returns - A query that searches for matching documents, returning them
   * in relevancy order.
   */
  withSearchIndex<IndexName extends SearchIndexNames<TableInfo>>(
    indexName: IndexName,
    searchFilter: (
      q: SearchFilterBuilder<
        DocumentByInfo<TableInfo>,
        NamedSearchIndex<TableInfo, IndexName>
      >,
    ) => SearchFilter,
  ): OrderedQuery<TableInfo>;

  /**
   * The number of documents in the table.
   *
   * @internal
   */
  count(): Promise<number>;
}

/**
 * The {@link Query} interface allows functions to read values out of the database.
 *
 * **If you only need to load an object by ID, use `db.get(tableName, id)` instead.**
 *
 * Executing a query consists of calling
 * 1. (Optional) {@link Query.order} to define the order
 * 2. (Optional) {@link Query.filter} to refine the results
 * 3. A *consumer* method to obtain the results
 *
 * Queries are lazily evaluated. No work is done until iteration begins, so constructing and
 * extending a query is free. The query is executed incrementally as the results are iterated over,
 * so early terminating also reduces the cost of the query.
 *
 * @example
 * ```typescript
 * // Use .withIndex() for efficient queries (preferred over .filter()):
 * const messages = await ctx.db
 *   .query("messages")
 *   .withIndex("by_channel", (q) => q.eq("channelId", channelId))
 *   .order("desc")
 *   .take(10);
 *
 * // Async iteration for processing large result sets:
 * for await (const task of ctx.db.query("tasks")) {
 *   // Process each task without loading all into memory
 * }
 *
 * // Get a single unique result (throws if multiple match):
 * const user = await ctx.db
 *   .query("users")
 *   .withIndex("by_email", (q) => q.eq("email", email))
 *   .unique();
 * ```
 *
 * **Common mistake:** `.collect()` loads **all** matching documents into memory.
 * If the result set can grow unbounded as your database grows, this will
 * eventually cause problems. Prefer `.first()`, `.unique()`, `.take(n)`, or
 * pagination instead. Only use `.collect()` on queries with a tightly bounded
 * result set (e.g., items belonging to a single user with a known small limit).
 *
 * |                                              | |
 * |----------------------------------------------|-|
 * | **Ordering**                                 | |
 * | [`order("asc")`](#order)                     | Define the order of query results. |
 * |                                              | |
 * | **Filtering**                                | |
 * | [`filter(...)`](#filter)                     | Filter the query results to only the values that match some condition. |
 * |                                              | |
 * | **Consuming**                                | Execute a query and return results in different ways. |
 * | [`[Symbol.asyncIterator]()`](#asynciterator) | The query's results can be iterated over using a `for await..of` loop. |
 * | [`collect()`](#collect)                      | Return all of the results as an array. |
 * | [`take(n: number)`](#take)                   | Return the first `n` results as an array. |
 * | [`first()`](#first)                          | Return the first result. |
 * | [`unique()`](#unique)                        | Return the only result, and throw if there is more than one result. |
 *
 * To learn more about how to write queries, see [Querying the Database](https://docs.convex.dev/database/reading-data).
 *
 * @public
 */
export interface Query<
  TableInfo extends GenericTableInfo,
> extends OrderedQuery<TableInfo> {
  /**
   * Define the order of the query output.
   *
   * Use `"asc"` for an ascending order and `"desc"` for a descending order. If not specified, the order defaults to ascending.
   * @param order - The order to return results in.
   */
  order(order: "asc" | "desc"): OrderedQuery<TableInfo>;
}

/**
 * A {@link Query} with an order that has already been defined.
 *
 * @public
 */
export interface OrderedQuery<
  TableInfo extends GenericTableInfo,
> extends AsyncIterable<DocumentByInfo<TableInfo>> {
  /**
   * Filter the query output, returning only the values for which `predicate` evaluates to true.
   *
   * **Important:** Prefer using `.withIndex()` over `.filter()` whenever
   * possible. Filters scan all documents matched so far and discard non-matches,
   * while indexes efficiently skip non-matching documents. Define an index in
   * your schema for fields you filter on frequently.
   *
   * @param predicate - An {@link Expression} constructed with the supplied {@link FilterBuilder} that specifies which documents to keep.
   * @returns - A new {@link OrderedQuery} with the given filter predicate applied.
   */
  filter(
    predicate: (q: FilterBuilder<TableInfo>) => ExpressionOrValue<boolean>,
  ): this;

  /**
   * Take only the first `n` results from the pipeline so far.
   *
   * @param n - Limit for the number of results at this stage of the query pipeline.
   * @returns - A new {@link OrderedQuery} with the specified limit applied.
   *
   * @internal
   */
  limit(n: number): this;

  /**
   * Load a page of `n` results and obtain a {@link Cursor} for loading more.
   *
   * Note: If this is called from a reactive query function the number of
   * results may not match `paginationOpts.numItems`!
   *
   * `paginationOpts.numItems` is only an initial value. After the first invocation,
   * `paginate` will return all items in the original query range. This ensures
   * that all pages will remain adjacent and non-overlapping.
   *
   * @param paginationOpts - A {@link PaginationOptions} object containing the number
   * of items to load and the cursor to start at.
   * @returns A {@link PaginationResult} containing the page of results and a
   * cursor to continue paginating.
   */
  paginate(
    paginationOpts: PaginationOptions,
  ): Promise<PaginationResult<DocumentByInfo<TableInfo>>>;

  /**
   * Execute the query and return all of the results as an array.
   *
   * **Warning:** This loads every matching document into memory. If the result
   * set can grow unbounded as your database grows, `.collect()` will eventually
   * cause performance problems or hit limits. Only use `.collect()` when the
   * result set is tightly bounded (e.g., a known small number of items).
   *
   * Prefer `.first()`, `.unique()`, `.take(n)`, or `.paginate()` when the
   * result set may be large or unbounded. For processing many results without
   * loading all into memory, use the `Query` as an `AsyncIterable` with
   * `for await...of`.
   *
   * @returns - An array of all of the query's results.
   */
  collect(): Promise<Array<DocumentByInfo<TableInfo>>>;

  /**
   * Execute the query and return the first `n` results.
   *
   * @param n - The number of items to take.
   * @returns - An array of the first `n` results of the query (or less if the
   * query doesn't have `n` results).
   */
  take(n: number): Promise<Array<DocumentByInfo<TableInfo>>>;

  /**
   * Execute the query and return the first result if there is one.
   *
   * @returns - The first value of the query or `null` if the query returned no results.
   * */
  first(): Promise<DocumentByInfo<TableInfo> | null>;

  /**
   * Execute the query and return the singular result if there is one.
   *
   * Use this when you expect exactly zero or one result, for example when
   * querying by a unique field. If the query matches more than one document,
   * this will throw an error.
   *
   * @example
   * ```typescript
   * const user = await ctx.db
   *   .query("users")
   *   .withIndex("by_email", (q) => q.eq("email", "alice@example.com"))
   *   .unique();
   * ```
   *
   * @returns - The single result returned from the query or null if none exists.
   * @throws Will throw an error if the query returns more than one result.
   */
  unique(): Promise<DocumentByInfo<TableInfo> | null>;
}
