import { useCallback, useEffect, useMemo } from "react";
import { NextRouter, useRouter } from "next/router";
import { createGlobalState } from "react-use";
import udfs from "@common/udfs";
import { useNents } from "@common/lib/useNents";
import { useSystemQuery } from "@common/lib/useSystemQuery";
import { isUserTableName } from "@common/lib/utils";

// Remembers each table's most recent (already base64-encoded) `filters` query
// param so switching back to a table restores its filters. Populated as you
// leave a table and read when you arrive; the URL stays the source of truth.
export const useFilterMap = createGlobalState(
  {} as Record<string, string | undefined>,
);

export type TableMetadata = {
  name: string | null;
  tableNames: string[];
  selectTable: (table: string) => void;
};

export function useTableMetadata(): TableMetadata | undefined {
  const router = useRouter();
  const { selectedNent } = useNents();

  const tableMappingState = useSystemQuery(udfs.getTableMapping.default, {
    componentId: selectedNent?.id ?? null,
  });
  // A member without `deployment:data:view` can't list tables, which reads the
  // same as "not loaded yet" to every caller below.
  const tableMapping =
    tableMappingState.status === "success" ? tableMappingState.data : undefined;

  const tableNames = useMemo(
    () =>
      tableMapping === undefined
        ? undefined
        : Object.values(tableMapping).filter(isUserTableName).sort(),
    [tableMapping],
  );

  const [filterMap, setFilterMap] = useFilterMap();

  const selectTable = useCallback(
    (table: string) => {
      if (tableNames === undefined) {
        return;
      }
      const currentTable = router.query.table as string | undefined;
      if (table === currentTable) {
        return;
      }
      // Remember the table we're leaving so we can restore its filters, and
      // restore the destination table's filters if we've seen them before.
      if (currentTable) {
        setFilterMap((prev) => ({
          ...prev,
          [currentTable]: router.query.filters as string | undefined,
        }));
      }
      void shallowNavigate(router, {
        ...router.query,
        table,
        filters: filterMap[table],
      });
    },
    [tableNames, router, filterMap, setFilterMap],
  );

  if (tableNames === undefined) {
    return undefined;
  }

  const tableNameFromURL = router.query.table as string | undefined;
  const firstTableName = (tableNames[0] as string | undefined) ?? null;

  const shownTableName =
    tableNameFromURL !== undefined && tableNames.includes(tableNameFromURL)
      ? tableNameFromURL
      : firstTableName;

  return {
    name: shownTableName,
    tableNames,
    selectTable,
  };
}

export function useTableMetadataAndUpdateURL(): TableMetadata | undefined {
  const tableMetadata = useTableMetadata();

  const router = useRouter();

  const tablesLoaded = tableMetadata !== undefined;
  const selectedTableName = tableMetadata?.name;

  // Remove a stale table name from the URL
  useEffect(() => {
    if (
      tablesLoaded &&
      selectedTableName !== null &&
      router.query.table !== selectedTableName
    ) {
      const table = selectedTableName ?? undefined;
      void shallowNavigate(router, {
        ...router.query,
        table,
        filters: undefined,
      });
    }
  }, [router, tablesLoaded, selectedTableName]);

  return tableMetadata;
}

export function shallowNavigate(
  router: NextRouter,
  newQuery: Record<string, string | undefined>,
) {
  return router.replace(
    // Filter out undefineds so they don't show up as empty strings in URL
    { query: objectWithoutUndefinedValues(newQuery) },
    undefined,
    { shallow: true },
  );
}

function objectWithoutUndefinedValues<T extends Record<string, any>>(
  obj: T,
): T {
  return Object.keys(obj).reduce(
    (acc, key) => (obj[key] === undefined ? acc : { ...acc, [key]: obj[key] }),
    {} as T,
  );
}
