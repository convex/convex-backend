import {
  FilterExpression,
  isValidFilter,
} from "system-udfs/convex/_system/frontend/lib/filters";

export type FiltersAppliedProperties = {
  filterCount: number;
  indexFilterCount: number;
  hasSearchQuery?: boolean;
};

export function summarizeFilters(
  filters: FilterExpression,
): FiltersAppliedProperties {
  const clauses = filters.clauses ?? [];
  const shared = {
    filterCount: clauses
      .filter(isValidFilter)
      .filter((c) => c.enabled !== false).length,
  };

  const { index } = filters;
  if (index === undefined) {
    return { ...shared, indexFilterCount: 0 };
  }

  const indexFilterCount = (index.clauses ?? []).filter(
    (c: { enabled?: boolean }) => c.enabled,
  ).length;

  return "search" in index
    ? { ...shared, indexFilterCount, hasSearchQuery: !!index.search }
    : { ...shared, indexFilterCount };
}

export function filterShapeSignature(
  properties: FiltersAppliedProperties,
): string {
  return JSON.stringify(properties);
}

export function dedupeByShape(
  report: (properties: FiltersAppliedProperties) => void,
): (properties: FiltersAppliedProperties) => void {
  let lastSignature: string | undefined;
  return (properties) => {
    const signature = filterShapeSignature(properties);
    if (signature === lastSignature) {
      return;
    }
    lastSignature = signature;
    report(properties);
  };
}
