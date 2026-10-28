import { decode, encodeURI, isValid } from "js-base64";
import { useRouter } from "next/router";

import { useMemo } from "react";
import {
  FilterExpression,
  FilterExpressionSchema,
  isValidFilter,
} from "system-udfs/convex/_system/frontend/lib/filters";
import { useFilterMap } from "@common/lib/useTableMetadata";
import {
  FiltersAppliedProperties,
  summarizeFilters,
} from "@common/features/data/lib/filterAnalytics";

// An expression with no clauses, no index, and no explicit order carries no
// selection and reads as "no filters". This mirrors the condition under which
// `applyFiltersWithHistory` clears the query param. An index selection —
// including a search index with an empty query string — is a real selection
// and must be preserved (see `hasValidEnabledFilters`).
function isEmptyFilterExpression(f: FilterExpression): boolean {
  return (
    f.clauses.length === 0 && f.index === undefined && f.order === undefined
  );
}

// Decodes the base64-encoded `filters` URL query param into a validated
// FilterExpression. Returns undefined for a missing, malformed, or empty
// param, so an invalid URL simply reads as "no filters".
export function parseFilters(
  raw: string | undefined,
): FilterExpression | undefined {
  if (!raw || !isValid(raw)) {
    return undefined;
  }
  let f: FilterExpression;
  try {
    f = JSON.parse(decode(raw));
    FilterExpressionSchema.parse(f);
  } catch {
    return undefined;
  }
  return isEmptyFilterExpression(f) ? undefined : f;
}

// The `filters` query param as it should be passed to `paginatedTableDocuments`:
// the raw (base64) string when it's a well-formed expression, otherwise null.
// The paginated query and its optimistic updates must both key off this exact
// value — a malformed or empty param (no longer scrubbed from the URL) would
// otherwise make their query args diverge and drop the optimistic update.
export function filterParamForQuery(raw: string | null): string | null {
  return raw && parseFilters(raw) ? raw : null;
}

export const useTableFilters = (
  tableName: string,
  onFiltersApplied?: (properties: FiltersAppliedProperties) => void,
) => {
  const { query, replace } = useRouter();
  const [, setFilterMap] = useFilterMap();

  const rawFilters = query.filters as string | undefined;
  const filters = useMemo(() => parseFilters(rawFilters), [rawFilters]);

  return {
    filters,
    applyFiltersWithHistory: async (newFilters?: FilterExpression) => {
      if (newFilters) {
        onFiltersApplied?.(summarizeFilters(newFilters));
        if (
          newFilters.clauses.length === 0 &&
          !newFilters.index &&
          newFilters.order === undefined
        ) {
          delete query.filters;
        } else {
          query.filters = encodeURI(JSON.stringify(newFilters));
        }
        // Keep this table's remembered filters in sync on every change so
        // switching back restores them regardless of how the table is left
        // (sidebar click, browser back/forward, etc.).
        setFilterMap((prev) => ({
          ...prev,
          [tableName]: query.filters as string | undefined,
        }));
        await replace(
          {
            query,
          },
          undefined,
          { shallow: true },
        );
      }
    },
    hasFilters: hasValidEnabledFilters(filters),
  };
};

function hasValidEnabledFilters(filters?: FilterExpression) {
  if (!filters) return false;

  // Arbitrary clause
  if (
    filters.clauses.filter(isValidFilter).filter((f) => f.enabled !== false)
      .length > 0
  )
    return true;

  if (!filters.index) return false;

  // Index clauses
  if ((filters.index.clauses.filter((f) => f.enabled).length ?? 0) > 0) {
    return true;
  }

  // Search index filters: we always return true (even if the search is empty)
  // to allow users to quickly remove the filter and see all documents
  if ("search" in filters.index) {
    return true;
  }

  return false;
}
