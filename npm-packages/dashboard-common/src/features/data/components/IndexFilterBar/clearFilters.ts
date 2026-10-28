import { FilterExpression } from "system-udfs/convex/_system/frontend/lib/filters";
import {
  CREATION_TIME_INDEX,
  EMPTY_FILTERS,
  currentOrder,
  isDatabaseFilter,
} from "./filterModel";

// Drops every clause but keeps the chosen sort. A search is dropped too,
// since an empty search matches nothing and "clear" should show all
// documents. Default sort with nothing applied collapses to no expression so
// the URL param disappears.
export function clearFilters(
  filters: FilterExpression | undefined,
): FilterExpression {
  if (!filters || !isDatabaseFilter(filters.index)) return EMPTY_FILTERS;
  if (
    filters.index.name === CREATION_TIME_INDEX.name &&
    currentOrder(filters) === "desc"
  ) {
    return EMPTY_FILTERS;
  }
  return {
    clauses: [],
    index: { name: filters.index.name, clauses: [] },
    order: filters.order,
  };
}
