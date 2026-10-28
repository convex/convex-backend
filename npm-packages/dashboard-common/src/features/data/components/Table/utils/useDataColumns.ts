import { useMemo } from "react";
import { GenericDocument } from "convex/server";
import { ColumnDef, RowData } from "@tanstack/react-table";
import { useLocalStorage } from "react-use";
import { isInCommonUTCTimestampRange } from "@common/features/data/lib/helpers";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";

declare module "@tanstack/react-table" {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  interface ColumnMeta<TData extends RowData, TValue> {
    // Whether values in this column are rendered as dates.
    isDate?: boolean;
    // Whether values in this column look like timestamps (which decides
    // whether to offer the "show as dates" toggle).
    isDateLike?: boolean;
  }
}

const MIN_COLUMN_WIDTH = 50;
// Accounts for different locales, prevents truncation if _creationTime is the last column
const CREATION_TIME_COLUMN_MIN_WIDTH = 200;
const MAX_COLUMN_WIDTH = 1000;
const DEFAULT_COLUMN_WIDTH = 150;
const ID_COLUMN_WIDTH = 100;
const RECORDS_CHECKED_FOR_DATE_HEURISTIC = 200;

export const emptyColumnName =
  "__CONVEX_PLACEHOLDER_empty_I23atX0jcndVbFgXoQZffsih7eAqktCyFjgUuAeNBtfr3ySOljPSPSEOPFgprkdBO3zXNiGEJxmJ5ZFPc5C5qKesG80QRPvlJe8vgSxAt9feLTwxTg4PHfVwUaTEJU67FDwldWmTxp1guMPwxQ2jOuhEryTBf3mQ";

// TODO: avoid collisions with fields named "*select".
export const checkboxColumnName = "*select";
export const useStoredColumnOrder = (localStorageKey = "_disabled_") =>
  useLocalStorage<string[]>(`${localStorageKey}_columnOrder`);

export const useStoredHiddenColumns = (localStorageKey = "_disabled_") =>
  useLocalStorage<string[]>(`${localStorageKey}_hiddenColumns`);

// Per-field override of whether timestamp-like numbers are shown as dates.
// Maps field name to the desired display; a field without an entry falls back
// to the auto-detection heuristic. Keyed by `${deploymentName}/${tableName}` so
// the preference is scoped to a specific deployment → table → column.
export const useStoredShowFieldsAsDates = (localStorageKey = "_disabled_") =>
  useGlobalLocalStorage<Record<string, boolean>>(
    `${localStorageKey}_showFieldsAsDates`,
    {},
  );

export const useDataColumns = ({
  fields,
  localStorageKey = "_disabled_",
  data = [],
  width = 100,
}: {
  tableName: string;
  fields: string[];
  localStorageKey?: string;
  data?: GenericDocument[];
  width?: number;
}) => {
  const [settings] = useGlobalLocalStorage<
    | {
        columnWidths: { [key: string]: number };
      }
    | undefined
  >(localStorageKey, { columnWidths: {} });
  const { columnWidths } = settings || { columnWidths: {} };

  const [showFieldsAsDates] = useStoredShowFieldsAsDates(localStorageKey);

  // Fields the heuristic thinks look like dates, regardless of the user's
  // preference. Used to decide whether to offer the "show as dates" toggle.
  const dateLikeColumns = useMemo(
    () =>
      data === undefined
        ? []
        : fields.filter((field) => shouldRenderFieldAsDate(field, data)),
    [data, fields],
  );

  // Fields actually rendered as dates: the user's per-field preference when
  // set, otherwise the heuristic.
  const dateRenderedColumns = useMemo(
    () =>
      fields.filter(
        (field) =>
          showFieldsAsDates?.[field] ?? dateLikeColumns.includes(field),
      ),
    [fields, dateLikeColumns, showFieldsAsDates],
  );

  const columns = useMemo<ColumnDef<GenericDocument, any>[]>(
    () => [
      {
        id: checkboxColumnName,
        header: checkboxColumnName,
        minSize: 40,
        size: 40,
        maxSize: 40,
        enableResizing: false,
      },
      ...fields.map(
        (field): ColumnDef<GenericDocument, any> => ({
          // The id supports an empty-string field name (because there are
          // falsy checks on all these fields and empty string is falsy).
          id: field === "" ? emptyColumnName : field,
          header: field === "" ? emptyColumnName : field,
          accessorFn: (row) => row[field],
          meta: {
            isDate: dateRenderedColumns.includes(field),
            isDateLike: dateLikeColumns.includes(field),
          },
          minSize:
            field === "_creationTime"
              ? CREATION_TIME_COLUMN_MIN_WIDTH
              : field === "_id"
                ? ID_COLUMN_WIDTH
                : MIN_COLUMN_WIDTH,
          // Figure out how wide to make each column by default.
          size:
            columnWidths && columnWidths[field]
              ? columnWidths[field]
              : Math.max(
                  (width - DEFAULT_COLUMN_WIDTH) / (fields.length - 1),
                  DEFAULT_COLUMN_WIDTH,
                ),
          enableResizing: true,
          maxSize: MAX_COLUMN_WIDTH,
        }),
      ),
    ],
    // Memoize columns for use with TanStack Table so that new data
    // or other rerender does not reset column widths.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      // eslint-disable-next-line react-hooks/exhaustive-deps
      JSON.stringify(columnWidths),
      // eslint-disable-next-line react-hooks/exhaustive-deps
      JSON.stringify(fields),
      // eslint-disable-next-line react-hooks/exhaustive-deps
      JSON.stringify(dateRenderedColumns),
      // eslint-disable-next-line react-hooks/exhaustive-deps
      JSON.stringify(dateLikeColumns),
    ],
  );

  if (columns.length) {
    // Set the width of the last column to consume the
    // remainder of available space, if there is any.
    const newWidth =
      width -
      columns
        .slice(0, columns.length - 1)
        .reduce((acc, curr) => acc + (curr.size ?? 0), 0);
    if (newWidth > MIN_COLUMN_WIDTH) {
      columns[columns.length - 1].size = newWidth;
      // getSize() clamps to maxSize; let the stretched column exceed the
      // regular resize cap.
      columns[columns.length - 1].maxSize = Math.max(
        newWidth,
        MAX_COLUMN_WIDTH,
      );
    }
  }
  return columns;
};

function shouldRenderFieldAsDate(field: string, data: GenericDocument[]) {
  if (field === "_creationTime") {
    return true;
  }
  const numChecked = Math.min(data.length, RECORDS_CHECKED_FOR_DATE_HEURISTIC);
  let isDateLike = true;
  let numPopulated = 0;
  for (let i = 0; i < numChecked; i++) {
    const document = data[i];
    if (document[field] === undefined) {
      continue;
    }
    const value = document[field];
    numPopulated += 1;
    if (typeof value !== "number" || !isInCommonUTCTimestampRange(value)) {
      isDateLike = false;
    }
  }
  // If there are no values for this field, assume it's not date-like
  return numPopulated !== 0 && isDateLike;
}
