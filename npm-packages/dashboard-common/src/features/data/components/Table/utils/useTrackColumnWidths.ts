import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { useEffect } from "react";
import { Table } from "@tanstack/react-table";
import { usePrevious } from "react-use";

export const useTrackColumnWidths = <TData>(
  table: Table<TData>,
  localStorageKey: string,
) => {
  const { isResizingColumn } = table.getState().columnSizingInfo;
  const { columnSizing } = table.getState();
  const [savedWidths, setSavedWidths] = useGlobalLocalStorage<
    | {
        columnWidths: { [key: string]: number };
      }
    | undefined
  >(localStorageKey, { columnWidths: {} });

  const wasResizingColumn = usePrevious(isResizingColumn);
  useEffect(() => {
    if (
      localStorageKey !== "_disabled" &&
      wasResizingColumn &&
      !isResizingColumn
    ) {
      setSavedWidths({
        columnWidths: {
          ...(savedWidths?.columnWidths || {}),
          [wasResizingColumn]: columnSizing[wasResizingColumn],
        },
      });
    }
  }, [
    isResizingColumn,
    wasResizingColumn,
    savedWidths,
    setSavedWidths,
    columnSizing,
    localStorageKey,
  ]);

  return () => {
    setSavedWidths(undefined);
    table.resetColumnSizing();
  };
};
