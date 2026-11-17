import { Value, GenericId } from "convex/values";
import { GenericDocument } from "convex/server";
import React, {
  CSSProperties,
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Row } from "@tanstack/react-table";
import classNames from "classnames";
import { useFirstMountState, usePrevious } from "react-use";
import { areEqual } from "react-window";
import omit from "lodash/omit";
import { useContextMenuTrigger } from "@common/features/data/lib/useContextMenuTrigger";
import { Target } from "@common/features/data/components/ContextMenu";
import { useTableDensity } from "@common/features/data/lib/useTableDensity";
import { TableCheckbox } from "@common/features/data/components/Table/TableCheckbox";
import {
  DataCell,
  DataCellProps,
} from "@common/features/data/components/Table/DataCell/DataCell";
import { usePatchDocumentField } from "@common/features/data/components/Table/utils/usePatchDocumentField";
import { arrowKeyHandler } from "@common/features/data/components/Table/utils/arrowKeyHandler";
import { toggleAdjacent } from "@common/features/data/components/Table/utils/toggleAdjacent";
import { SchemaJson } from "@common/lib/format";

type DataRowProps = {
  data: {
    areEditsAuthorized: boolean;
    isRowSelected(row: string): boolean;
    isSelectionAllNonExhaustive: boolean;
    resizingColumn: string | undefined;
    authorizeEdits?(): void;
    patchDocument: ReturnType<typeof usePatchDocumentField>;
    rows: Row<GenericDocument>[];
    columnResizeHandlers: Record<string, (event: unknown) => void>;
    tableName: string;
    toggleIsRowSelected(key: string): void;
    onOpenContextMenu: DataCellProps["onOpenContextMenu"];
    onCloseContextMenu: () => void;
    contextMenuRow: string | null;
    contextMenuColumn: string | null;
    canManageTable: boolean;
    activeSchema: SchemaJson | null;
    onEditDocument: (document: GenericDocument) => void;
  };
  index: number;
  style: CSSProperties;
};

export const DataRow = memo(DataRowImpl, areEqual);

function DataRowImpl(props: DataRowProps) {
  const { data, index, style } = props;

  const firstRow = data.rows.length ? data.rows[0] : undefined;
  const firstRowCells = firstRow?.getVisibleCells();
  const { densityValues } = useTableDensity();
  return index >= data.rows.length ? (
    <div
      className="DataRow flex"
      style={{ ...style, height: densityValues.height }}
    >
      {firstRowCells ? (
        firstRowCells.map((cell, idx) => (
          <div
            key={cell.id}
            role="cell"
            className={classNames(
              "h-full flex shrink-0 items-center justify-center",
              {
                "border-r": cell !== firstRowCells[firstRowCells.length - 1],
              },
            )}
            style={{
              width: cell.column.getSize(),
              paddingTop: densityValues.paddingY,
              paddingBottom: densityValues.paddingY,
              paddingLeft: densityValues.paddingX,
              paddingRight: densityValues.paddingX,
            }}
          >
            <div
              className="h-4 bg-background-tertiary"
              style={{
                width: idx === 0 ? "1rem" : "100%",
              }}
            />
          </div>
        ))
      ) : (
        <div className="mt-4 ml-4 h-4 w-full rounded-sm bg-neutral-8/20 dark:bg-neutral-3/20" />
      )}
    </div>
  ) : (
    <DataRowLoaded {...props} />
  );
}

function DataRowLoaded({ index, style, data }: DataRowProps) {
  const {
    areEditsAuthorized,
    isRowSelected,
    isSelectionAllNonExhaustive,
    authorizeEdits,
    patchDocument,
    rows,
    columnResizeHandlers,
    tableName,
    toggleIsRowSelected,
    onOpenContextMenu,
    onCloseContextMenu,
    canManageTable,
    activeSchema,
    resizingColumn: _resizingColumn,
    onEditDocument,
    contextMenuColumn,
    contextMenuRow,
  } = data;

  const row: Row<GenericDocument> = rows[index];
  const previousRow = usePrevious(row);
  const previousRows = usePrevious(rows);

  const didNumberOfRowsChange = previousRows?.length !== rows.length;

  const _id = row.original._id as GenericId<string>;
  const previousRowId = previousRow?.original._id as
    | GenericId<string>
    | undefined;

  const [didJustCreate, setDidJustCreate] = useState(false);
  useEffect(() => {
    // The entire row should be highlighted if the row was recently created and
    // not already rendered.
    if (
      !previousRowId &&
      Date.now() - (row.original._creationTime as number) < 1000
    ) {
      setDidJustCreate(true);
      // To reset the animatation, reset the state after one second.
      setTimeout(() => setDidJustCreate(false), 1000);
    }
  }, [row, previousRow, previousRowId, _id]);

  const mounting = useFirstMountState();
  const checked = isRowSelected(_id);

  // Context menu trigger for the checkbox cell
  const checkboxRef = useRef<HTMLLabelElement | null>(null);
  const contextMenuCallback = useCallback(
    (position: Target) => onOpenContextMenu(position, _id, null),
    [onOpenContextMenu, _id],
  );
  useContextMenuTrigger(checkboxRef, contextMenuCallback, onCloseContextMenu);
  const document = useMemo(() => omit(row.original, "*select"), [row.original]);

  const editDocument = useCallback(() => {
    if (canManageTable) {
      onEditDocument(document);
    }
  }, [canManageTable, onEditDocument, document]);

  return (
    <div
      role="row"
      className={classNames(
        "animate-fadeInFromLoading",
        // Make sure the focus ring is visible on first and last cell
        "focus:ring-none focus:border",
        didJustCreate && "animate-highlight",
        "DataRow flex",
      )}
      style={style}
    >
      {row.getVisibleCells().map((cell, columnIndex) => {
        const width = columnWidthToString(cell.column.getSize());
        return (
          <div
            key={cell.id}
            role="cell"
            style={{ width }}
            className="shrink-0 border-r transition-colors duration-300"
          >
            {columnIndex === 0 ? (
              <TableCheckbox
                width={width}
                ref={checkboxRef}
                onKeyDown={arrowKeyHandler(checkboxRef)}
                isSelectionAllNonExhaustive={isSelectionAllNonExhaustive}
                onToggle={() => toggleIsRowSelected(_id)}
                onToggleAdjacent={() =>
                  toggleAdjacent(
                    rows.map((r) => r.original._id as string),
                    index,
                    isRowSelected,
                    toggleIsRowSelected,
                  )
                }
                checked={checked}
              />
            ) : (
              <DataCell
                activeSchema={activeSchema}
                rowId={_id}
                document={document}
                didRowChange={
                  // The row changed if it's already been mounted,
                  // the previous row is not the same as the current row,
                  // and the number of rows has not changed.
                  !mounting && previousRowId !== _id && !didNumberOfRowsChange
                }
                areEditsAuthorized={areEditsAuthorized}
                authorizeEdits={authorizeEdits}
                editDocument={editDocument}
                value={cell.getValue() as Value}
                column={cell.column}
                resizeHandler={columnResizeHandlers[cell.column.id]}
                width={width}
                inferIsDate={cell.column.columnDef.meta?.isDate ?? false}
                patchDocument={patchDocument}
                tableName={tableName}
                onOpenContextMenu={onOpenContextMenu}
                onCloseContextMenu={onCloseContextMenu}
                isContextMenuOpen={
                  contextMenuColumn === cell.column.id && contextMenuRow === _id
                }
                canManageTable={canManageTable}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

// The goal here is to floor the width of the column to the nearest pixel to avoid
// sub-pixel rendering issues in the browser.
export const columnWidthToString = (width?: string | number) =>
  width
    ? `${Math.floor(
        typeof width === "string" ? Number(width.replace("px", "")) : width,
      ).toString()}px`
    : `0px`;
