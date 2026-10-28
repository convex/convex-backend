import {
  FilterBuilder,
  GenericDocument,
  GenericTableInfo,
  Index,
  OrderedQuery,
  PaginationResult,
  paginationOptsValidator,
} from "convex/server";
import { decode } from "js-base64";
import {
  FilterByBuiltin,
  FilterByOr,
  FilterExpression,
  FilterExpressionSchema,
  FilterValidationError,
  ValidFilterByBuiltin,
  ValidFilterByOr,
  applyIndexFilters,
  applySearchIndexFilters,
  applyTypeFilters,
  findErrorsInFilters,
  findIndexByName,
  getAvailableIndexes,
  isValidFilter,
  partitionFiltersByOperator,
  validateIndexFilter,
  validateSearchIndexFilter,
} from "./lib/filters";
import { queryGeneric } from "../secretSystemTables";
import { getSchemaByState } from "./getSchemas";
import { maximumBytesRead, maximumRowsRead } from "../paginationLimits";
import { jsonToConvex, v } from "convex/values";
import { Expression } from "convex/server";
import { ExpressionOrValue } from "convex/server";
import { Value } from "convex/values";
import { UNDEFINED_PLACEHOLDER } from "./lib/values";
import { SearchIndex } from "../../../../convex/dist/internal-cjs-types/server";

export default queryGeneric("ViewData")({
  args: {
    paginationOpts: paginationOptsValidator,
    table: v.string(),
    filters: v.union(v.string(), v.null()),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  /**
   * @param paginationOpts - Pagination options
   * @param table - The name of the table to query
   * @param filters - The expected value of filters is a b64-encoded string {@link FilterExpression}.
   *  {@link FilterExpression} contains {@link JSONValue}s so that invalid IDs being filtered are still able to
   *  be passed into the UDF over the wire. Since {@link JSONValue} type contains keys beginning with dollar signs, we then encode it to
   *  a string format so it can be passed over the wire as well.
   * @returns A paginated list of documents matching the provided filters
   */
  handler: async (
    { db },
    { paginationOpts, table, filters },
  ): Promise<PaginationResult<GenericDocument | FilterValidationError>> => {
    const parsedFilters: FilterExpression | null = filters
      ? (JSON.parse(decode(filters)) as FilterExpression)
      : null;

    // This will throw an error if parsedFilters does not match the filter expression schema,
    // which should only happens if a dashboard user manually edits the `filters` query parameter
    // the dashboard should not allow this to happen by deleting the query parameter if it is invalid.
    if (parsedFilters) {
      FilterExpressionSchema.parse(parsedFilters);
    }

    if (parsedFilters && parsedFilters.clauses?.length) {
      const errors = await findErrorsInFilters(parsedFilters);
      if (errors.length) {
        // Hack to trick usePaginatedQuery that we've actually called `paginate`, but really returning a list of
        // error-describing objects
        return Promise.resolve({
          page: errors,
          isDone: true,
          continueCursor: "",
        });
      }
    }
    const enabledFilters = parsedFilters?.clauses?.filter(
      (f) => f.enabled !== false,
    );

    const queryInitializer = db.query(table);
    let query: OrderedQuery<any> | undefined = undefined;

    // Get the order from parsedFilters, default to "desc" if not specified
    const order = parsedFilters?.order || "desc";

    const indexFilter = parsedFilters?.index;
    const hasIndexFilter =
      indexFilter &&
      ("search" in indexFilter ||
        indexFilter.clauses.filter((c) => c.enabled).length > 0);
    if (indexFilter) {
      // Let's find out if we can use an index from the schema.
      const schemaData = await getSchemaByState(
        (db as any).privateSystem,
        "active",
      );

      // Get available indexes using the helper function
      const indexes = getAvailableIndexes(table, schemaData);

      // Find the selected index by name
      const selectedIndex = findIndexByName(indexFilter.name, indexes);

      // Validate the filter filter
      const isSearchIndex = "search" in indexFilter;
      const validationError = isSearchIndex
        ? validateSearchIndexFilter(
            indexFilter.name,
            indexFilter.clauses,
            selectedIndex,
            order,
          )
        : validateIndexFilter(
            indexFilter.name,
            indexFilter.clauses,
            selectedIndex,
          );

      if (validationError) {
        return {
          page: [validationError],
          isDone: true,
          continueCursor: "",
        };
      }

      query = isSearchIndex
        ? queryInitializer.withSearchIndex(indexFilter.name, (q) =>
            applySearchIndexFilters(
              q,
              indexFilter.search,
              indexFilter.clauses,
              selectedIndex as SearchIndex,
            ),
          )
        : queryInitializer
            .withIndex(indexFilter.name, (q) =>
              applyIndexFilters(q, indexFilter.clauses, selectedIndex as Index),
            )
            .order(order);
    }

    const [builtinFilters, typeFilters] =
      partitionFiltersByOperator(enabledFilters);

    if (builtinFilters !== undefined && builtinFilters.length > 0) {
      // Edge case: requesting a single ID - use a db.get
      const isAFilterForSingleDocument =
        !hasIndexFilter &&
        builtinFilters.length === 1 &&
        "field" in builtinFilters[0] &&
        builtinFilters[0].field === "_id" &&
        builtinFilters[0].op === "eq";

      if (isAFilterForSingleDocument) {
        const emptyResult = {
          page: [],
          isDone: true,
          continueCursor: "",
        };
        const documentId = builtinFilters[0].value;
        if (typeof documentId !== "string") {
          return emptyResult;
        }
        const normalizedId = db.normalizeId(table, documentId);
        if (normalizedId === null) {
          return emptyResult;
        }
        const document = await db.get(normalizedId);
        if (document === null) {
          return emptyResult;
        }
        return {
          page: [document],
          isDone: true,
          continueCursor: "",
        };
      }

      query = (query || queryInitializer.order(order))
        // eslint-disable-next-line @convex-dev/no-filter-in-query -- we allow filtering by multiple fields/operators
        .filter((q) => applyBuiltinFilters(q, builtinFilters));
    }

    const internalPaginateOpts = {
      ...paginationOpts,
      // these are internal options
      maximumRowsRead,
      maximumBytesRead,
    };

    const { page, ...rest } = await (
      query || queryInitializer.order(order)
    ).paginate(internalPaginateOpts);

    const filteredPage = typeFilters
      ? applyTypeFilters(page, typeFilters)
      : page;

    return { page: filteredPage, ...rest };
  },
});

/**
 * Applies built-in filters to a query builder.
 *
 * @param q The query builder to apply filters to
 * @param filters Array of built-in filters to apply
 * @returns An expression or value representing the filter condition
 */
export function applyBuiltinFilters(
  q: FilterBuilder<GenericTableInfo>,
  filters: (FilterByBuiltin | FilterByOr)[],
): ExpressionOrValue<boolean> {
  const validatedFilters = filters.filter<
    ValidFilterByBuiltin | ValidFilterByOr
  >((f): f is ValidFilterByBuiltin | ValidFilterByOr => isValidFilter(f));
  // If there are no valid filters, return the query unchanged.
  if (validatedFilters.length === 0) {
    return true;
  }
  return q.and(
    ...validatedFilters.map((f) => {
      const value = jsonToConvex(f.value);

      if (f.op === "anyOf" || f.op === "noneOf") {
        if (f.value.length === 0) {
          // Even though the filter is valid, don't apply the operation if there are not values to compare to.
          return true;
        }
        if (f.op === "anyOf") {
          return q.or(
            ...f.value.map((v) =>
              q.eq(
                q.field(f.field),
                v === UNDEFINED_PLACEHOLDER ? undefined : v,
              ),
            ),
          );
        } else {
          return q.and(
            ...f.value.map((v) =>
              q.neq(
                q.field(f.field),
                v === UNDEFINED_PLACEHOLDER ? undefined : v,
              ),
            ),
          );
        }
      }

      // q.eq and q.neq support undefined, while q.lt, q.gt, etc. do not, so we have to cast.
      const comparison = q[f.op] as (
        f: Expression<Value>,
        v: ExpressionOrValue<Value | undefined>,
      ) => Expression<boolean>;
      return comparison(
        q.field(f.field),
        value === UNDEFINED_PLACEHOLDER ? undefined : value,
      );
    }),
  );
}
