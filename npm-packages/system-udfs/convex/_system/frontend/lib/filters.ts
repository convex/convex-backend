import { JSONValue, ValidatorJSON, Value, jsonToConvex } from "convex/values";
import type {
  GenericDocument,
  GenericSearchIndexConfig,
  Index,
  SearchFilterBuilder,
  SearchFilterFinalizer,
  SearchIndex,
  VectorIndex,
} from "convex/server";
import * as IdEncoding from "id-encoding";

import { z } from "zod";
import { UNDEFINED_PLACEHOLDER } from "./values";

export interface FilterExpression {
  // In the future, this can be extended to support nested clauses.
  clauses: Filter[];
  order?: "asc" | "desc";
  index?: DatabaseIndexFilter | SearchIndexFilter;
}

export type DatabaseIndexFilter = {
  name: string;
  clauses: FilterByIndex[] | [...FilterByIndex[], FilterByIndexRange];
};

export type SearchIndexFilter = {
  name: string;
  search: string;
  /** The clauses on the filter fields of the search index */
  clauses: SearchIndexFilterClause[];
};

export type DatabaseIndexFilterClause = FilterByIndex | FilterByIndexRange;

export type SearchIndexFilterClause = {
  field: string;
  enabled: boolean;
  value: JSONValue | undefined;
};

export type FilterCommon = {
  id?: string;
  field?: string;
  enabled?: boolean;
};

export type FilterByIndex = {
  type: "indexEq";
  enabled: boolean;
  value?: JSONValue | Value;
};

export type FilterByIndexRange = {
  type: "indexRange";
  enabled: boolean;
  lowerOp?: "gte" | "gt";
  lowerValue?: JSONValue | Value;
  upperOp?: "lte" | "lt";
  upperValue?: JSONValue | Value;
};

export type FilterByBuiltin = {
  op: "eq" | "neq" | "gte" | "lte" | "gt" | "lt";
  value?: JSONValue;
};

export type ValidFilterByBuiltin = {
  [P in keyof Omit<FilterCommon, "enabled">]-?: FilterCommon[P];
} & { [P in keyof FilterByBuiltin]-?: FilterByBuiltin[P] } & {
  enabled?: boolean;
};

export type FilterByOr = {
  op: "anyOf" | "noneOf";
  value?: JSONValue[];
};

export type ValidFilterByOr = {
  [P in keyof Omit<FilterCommon, "enabled">]-?: FilterCommon[P];
} & { [P in keyof FilterByOr]-?: FilterByOr[P] } & {
  enabled?: boolean;
};

export type ValidFilterByBuiltInOrOr = ValidFilterByBuiltin | ValidFilterByOr;

const TypeFilterValueKeys = [
  "string",
  "boolean",
  "number",
  "bigint",
  "null",
  "id",
  "array",
  "object",
  "bytes",
  "unset",
] as const;

export type TypeFilterValue = (typeof TypeFilterValueKeys)[number];

export type FilterByType = {
  op: TypeFilterOp;
  value?: TypeFilterValue;
};

export type ValidFilterByType = {
  [P in keyof Omit<FilterCommon, "enabled">]-?: FilterCommon[P];
} & { [P in keyof FilterByType]-?: FilterByType[P] } & {
  enabled?: boolean;
};

const TypeFilterOpKeys = ["type", "notype"] as const;

export type TypeFilterOp = (typeof TypeFilterOpKeys)[number];

const objectSuperTypes: TypeFilterValue[] = ["null", "array", "id", "bytes"];

export const isTypeFilterOp = (op: Filter["op"]): op is TypeFilterOp => {
  return TypeFilterOpKeys.includes(op as TypeFilterOp);
};

export type Filter = FilterCommon &
  (FilterByBuiltin | FilterByOr | FilterByType);

const FilterSchema = z.array(
  z.union([
    z.object({
      op: z.union([
        z.literal("eq"),
        z.literal("neq"),
        z.literal("gte"),
        z.literal("lte"),
        z.literal("gt"),
        z.literal("lt"),
      ]),
      field: z.string().optional(),
      value: z.any().optional(),
      id: z.string().optional(),
      enabled: z.boolean().optional(),
    }),
    z.object({
      op: z.union([z.literal("type"), z.literal("notype")]),
      field: z.string().optional(),
      value: z.enum(TypeFilterValueKeys).optional(),
      id: z.string().optional(),
      enabled: z.boolean().optional(),
    }),
    z.object({
      op: z.union([z.literal("anyOf"), z.literal("noneOf")]),
      field: z.string().optional(),
      value: z.array(z.any()).optional(),
      id: z.string().optional(),
      enabled: z.boolean().optional(),
    }),
  ]),
);

export const FilterExpressionSchema: z.ZodType<FilterExpression> = z.lazy(() =>
  z.object({
    op: z.literal("and").optional(),
    clauses: FilterSchema,
    order: z.union([z.literal("asc"), z.literal("desc")]).optional(),
    // This gets validated by code instead of Zod.
    index: z.optional(z.any()),
  }),
);

export type ValidFilter =
  | ValidFilterByBuiltin
  | ValidFilterByOr
  | ValidFilterByType;

export const isValidFilter = (f: Filter): f is ValidFilter =>
  f.field !== undefined;

const isType = (value: Value, type: TypeFilterValue): boolean => {
  switch (type) {
    case "null":
      return value === null;
    case "array":
      return Array.isArray(value);
    case "id":
      return typeof value === "string" && IdEncoding.isId(value);
    case "bytes":
      return value instanceof ArrayBuffer;
    case "object":
      return (
        typeof value === "object" &&
        !objectSuperTypes.some((t) => isType(value, t))
      );
    case "unset":
      return value === undefined;
    default:
      return typeof value === type;
  }
};

export const typeOf = (value: Value | undefined): TypeFilterValue => {
  if (value === null) {
    return "null";
  } else if (Array.isArray(value)) {
    return "array";
  } else if (value instanceof ArrayBuffer) {
    return "bytes";
  } else if (typeof value === "object") {
    return "object";
  } else if (value === undefined) {
    return "unset";
  } else {
    const t = typeof value;
    if (t === "symbol" || t === "function" || t === "undefined") {
      throw new Error(`unexpected type of value ${t}`);
    }
    return t;
  }
};

export function applyTypeFilters(
  page: GenericDocument[],
  filters: FilterByType[],
) {
  const validatedFilters = filters
    .filter<ValidFilterByType>((f): f is ValidFilterByType => isValidFilter(f))
    // Only apply filters that are enabled (or where enabled is undefined for backward compatibility)
    .filter((f) => f.enabled !== false);
  return page.filter((doc) => {
    for (const filter of validatedFilters) {
      const value = doc[filter.field];
      const matchesType = isType(value, filter.value);
      if (
        (filter.op === "type" && !matchesType) ||
        (filter.op === "notype" && matchesType)
      ) {
        return false;
      }
    }
    return true;
  });
}

export type FilterValidationError = {
  error: string;
  filter: number;
};

export const isFilterValidationError = (
  result: GenericDocument | FilterValidationError,
): result is FilterValidationError => !("_id" in result);

export async function findErrorsInFilters(
  parsedFilters: FilterExpression,
): Promise<FilterValidationError[]> {
  const errors = [];
  for (const [i, filter] of parsedFilters.clauses.entries()) {
    if (filter.value) {
      try {
        jsonToConvex(filter.value);
      } catch {
        errors.push({
          filter: i,
          error: `Invalid value: ${JSON.stringify(filter.value)}.`,
        });
        continue;
      }
    }
  }
  return errors;
}

export function partitionFiltersByOperator(
  filters?: Filter[],
): [ValidFilterByBuiltInOrOr[], ValidFilterByType[]] {
  const builtinFilters =
    filters?.filter<ValidFilterByBuiltin | ValidFilterByOr>(
      (f): f is ValidFilterByBuiltin | ValidFilterByOr => !isTypeFilterOp(f.op),
    ) || [];

  const typeFilters =
    filters?.filter<ValidFilterByType>((f): f is ValidFilterByType =>
      isTypeFilterOp(f.op),
    ) || [];

  return [builtinFilters, typeFilters];
}

/**
 * Applies index filters to a query builder.
 *
 * @param q The query builder to apply filters to
 * @param indexFilters Array of index filters to apply
 * @param selectedIndex The selected index to use
 * @returns The modified query builder
 * @throws Error if the index is undefined or if a range filter is not the last filter
 */
export function applyIndexFilters(
  q: any,
  indexFilters: FilterByIndex[] | [...FilterByIndex[], FilterByIndexRange],
  selectedIndex: Index,
): any {
  if (!selectedIndex) {
    throw new Error("Index is undefined");
  }

  let builder = q;
  if (indexFilters.length === 0) {
    return builder;
  }

  const enabledClauses = indexFilters.filter((f) => f.enabled);

  for (let i = 0; i < enabledClauses.length; i++) {
    const filter = indexFilters[i];
    if (filter.type === "indexEq") {
      builder = builder.eq(
        selectedIndex.fields[i],
        filter.value === UNDEFINED_PLACEHOLDER
          ? undefined
          : jsonToConvexOrValue(filter.value),
      );
    } else {
      if (i !== enabledClauses.length - 1) {
        throw new Error("Index range not supported");
      }
      if (filter.lowerOp) {
        builder = builder[filter.lowerOp](
          selectedIndex.fields[i],
          filter.lowerValue === UNDEFINED_PLACEHOLDER
            ? undefined
            : jsonToConvexOrValue(filter.lowerValue),
        );
      }
      if (filter.upperOp) {
        builder = builder[filter.upperOp](
          selectedIndex.fields[i],
          filter.upperValue === UNDEFINED_PLACEHOLDER
            ? undefined
            : jsonToConvexOrValue(filter.upperValue),
        );
      }
    }
  }

  return builder;
}

/**
 * Applies search index filters to a query builder.
 *
 * @param q The query builder to apply filters to
 * @param search The search string
 * @param filters The search filters to apply
 * @param selectedIndex The selected search index to use
 * @returns The modified query builder
 */
export function applySearchIndexFilters<
  Document extends GenericDocument,
  SearchIndexConfig extends GenericSearchIndexConfig,
>(
  q: SearchFilterBuilder<Document, SearchIndexConfig>,
  search: string,
  filters: SearchIndexFilterClause[],
  selectedIndex: SearchIndex,
): SearchFilterFinalizer<Document, SearchIndexConfig> {
  let builder = q.search(selectedIndex.searchField, search);

  // Apply filters
  for (const { field, enabled, value } of filters) {
    if (enabled) {
      builder = builder.eq(
        field,
        (value === UNDEFINED_PLACEHOLDER
          ? undefined
          : jsonToConvexOrValue(value)) as any,
      );
    }
  }

  return builder;
}

/**
 * Validates index filter clauses to ensure they are properly structured.
 *
 * @param indexName The name of the index being used
 * @param indexClauses The filter clauses to validate
 * @param selectedIndex The selected index to use, or undefined if not found
 * @returns An error object if validation fails, undefined if validation passes
 */
export function validateIndexFilter(
  indexName: string,
  indexClauses: (FilterByIndex | FilterByIndexRange)[],
  selectedIndex: Index | SearchIndex | undefined,
): { filter: number; error: string } | undefined {
  // Check if the index exists
  if (!selectedIndex) {
    return {
      filter: -1,
      error: `Index ${indexName} does not exist.`,
    };
  }

  if ("searchField" in selectedIndex) {
    return {
      filter: -1,
      error: `Index ${indexName} is a search index, but the query is trying to use it as a database index.`,
    };
  }

  if (indexClauses.length > selectedIndex.fields.length) {
    return {
      filter: -1,
      error: `Index ${indexName} has ${selectedIndex.fields.length} fields, but the query has ${indexClauses.length} clauses.`,
    };
  }

  let finishedEnabledClausesIdx = -1;
  for (let i = 0; i < indexClauses.length; i++) {
    const clause = indexClauses[i];
    if (clause.enabled) {
      // If we have already seen a disabled clause, then this is an invalid filter.
      if (finishedEnabledClausesIdx !== -1) {
        return {
          filter: -1,
          error: `Invalid index filter selection: found an enabled clause after an disabled clause.`,
        };
      }
    } else if (finishedEnabledClausesIdx === -1) {
      finishedEnabledClausesIdx = i;
    }
  }
  // Make sure that only the last clause can be a range filter.
  for (let i = 0; i < finishedEnabledClausesIdx; i++) {
    const clause = indexClauses[i];
    if (clause.type === "indexRange" && i !== finishedEnabledClausesIdx - 1) {
      return {
        filter: -1,
        error: `Invalid index filter selection: found a range filter after a non-range filter.`,
      };
    }
  }

  return undefined;
}

/**
 * Validates search index filter clauses to ensure they are properly structured.
 *
 * @param indexName The name of the index being used
 * @param filters The filter clauses to validate
 * @param selectedIndex The selected index to use, or undefined if not found
 * @param order The order in which the index is queried
 * @returns An error object if validation fails, undefined if validation passes
 */
export function validateSearchIndexFilter(
  indexName: string,
  filters: SearchIndexFilterClause[],
  selectedIndex: Index | SearchIndex | undefined,
  order: "asc" | "desc",
) {
  // Check if the index exists
  if (!selectedIndex) {
    return {
      filter: -1,
      error: `Index ${indexName} does not exist.`,
    };
  }

  if (!("searchField" in selectedIndex)) {
    return {
      filter: -1,
      error: `Index ${indexName} is not a search index, but the query is trying to use it as search index.`,
    };
  }

  if (order !== "asc") {
    return {
      filter: -1,
      error: `Trying to query search index \`${indexName}\` in descending order.`,
    };
  }

  for (const [filterIndex, filter] of filters.entries()) {
    if (!filter.enabled) continue;

    // Field not a filter field?
    if (!selectedIndex.filterFields.includes(filter.field)) {
      return {
        filter: filterIndex,
        error: `Invalid index filter selection: found a filter for field \`${filter.field}\` which is not part of the filter fields of the search index \`${indexName}\`.`,
      };
    }

    // Duplicated filter for field?
    if (
      filters.some(
        (otherFilter, otherFilterIndex) =>
          filterIndex !== otherFilterIndex &&
          otherFilter.enabled &&
          filter.field === otherFilter.field,
      )
    ) {
      return {
        filter: filterIndex,
        error: `Invalid filter: there are multiple filters for field \`${filter.field}\`.`,
      };
    }
  }

  return undefined;
}

export function parseAndFilterToSingleTable(
  tableName: string,
  schema: any,
): undefined | SchemaJson {
  if (!schema) {
    return undefined;
  }
  const result = JSON.parse(schema) as SchemaJson;
  result.tables = result.tables.filter(
    (table) => table.tableName === tableName,
  );
  return result;
}
type TableDefinition = {
  tableName: string;
  indexes: Index[];
  searchIndexes: SearchIndex[];
  vectorIndexes?: VectorIndex[];
  documentType: ValidatorJSON | null;
};

export type SchemaJson = {
  tables: TableDefinition[];
  schemaValidation: boolean;
};

/**
 * Gets the default index for a table.
 *
 * @returns The default index for creation time
 */
export function getDefaultIndex(): Index {
  return {
    indexDescriptor: "by_creation_time",
    fields: ["_creationTime"],
  };
}

/**
 * Gets the by_id system index.
 *
 * @returns The by_id system index
 */
export function getByIdIndex(): Index {
  return {
    indexDescriptor: "by_id",
    fields: ["_id"],
  };
}

/**
 * Gets all available indexes for a table, including the default creation time index.
 *
 * @param table The table name
 * @param schemaData The schema data
 * @returns Array of available indexes
 */
export function getAvailableIndexes(
  table: string,
  schemaData: any,
): (Index | SearchIndex)[] {
  if (!schemaData?.schema) {
    return [getDefaultIndex(), getByIdIndex()];
  }

  const parsed = parseAndFilterToSingleTable(table, schemaData.schema)
    ?.tables[0];

  return [
    ...(parsed?.indexes || []),
    ...(parsed?.searchIndexes || []),
    getDefaultIndex(),
    getByIdIndex(),
  ];
}

/**
 * Finds an index by name from the available indexes.
 *
 * @param indexName The name of the index to find
 * @param indexes Array of available indexes
 * @returns The found index or undefined if not found
 */
export function findIndexByName(
  indexName: string,
  indexes: (Index | SearchIndex)[],
): Index | SearchIndex | undefined {
  return indexes.find((i) => i.indexDescriptor === indexName);
}

// If the value is not a valid JSON value, return the value as is.
function jsonToConvexOrValue(
  value: JSONValue | Value | undefined,
): Value | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (isBigIntOrArrayBufferInValue(value)) {
    return value;
  }
  try {
    return jsonToConvex(value);
  } catch {
    return value;
  }
}

// If the value has a bigint or array buffer in it, it's definitely not a JSONValue
function isBigIntOrArrayBufferInValue(
  value: JSONValue | Value,
): value is Value {
  if (Array.isArray(value)) {
    return value.some(isBigIntOrArrayBufferInValue);
  }
  return typeof value === "bigint" || value instanceof ArrayBuffer;
}
