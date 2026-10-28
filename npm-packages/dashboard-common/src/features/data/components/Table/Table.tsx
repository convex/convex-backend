import { GenericDocument } from "convex/server";
import classNames from "classnames";
import {
  MutableRefObject,
  useCallback,
  useContext,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import {
  ColumnDef,
  getCoreRowModel,
  useReactTable,
} from "@tanstack/react-table";
import { FixedSizeList } from "react-window";
import {
  Filter,
  SchemaJson,
} from "system-udfs/convex/_system/frontend/lib/filters";
import { DndContext, closestCenter } from "@dnd-kit/core";
import {
  SortableContext,
  horizontalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  ImperativePanelHandle,
  Panel,
  PanelGroup,
} from "react-resizable-panels";
import { cn } from "@ui/cn";
import { ResizeHandle } from "@common/layouts/SidebarDetailLayout";
import { useTableDensity } from "@common/features/data/lib/useTableDensity";
import { InfiniteScrollList } from "@common/elements/InfiniteScrollList";
import { SelectionState } from "@common/features/data/lib/useSelectionState";
import { usePatchDocumentField } from "@common/features/data/components/Table/utils/usePatchDocumentField";
import { DataRow } from "@common/features/data/components/Table/DataRow";
import { TableScrollbar } from "@common/features/data/components/Table/TableScrollbar";
import { useTrackColumnWidths } from "@common/features/data/components/Table/utils/useTrackColumnWidths";
import type { PopupState } from "@common/features/data/lib/useToolPopup";
import { useMaintainScrollPositionOnChange } from "@common/features/data/components/Table/utils/useMaintainScrollPositionOnChange";
import {
  TableContextMenu,
  useTableContextMenuState,
} from "@common/features/data/components/Table/TableContextMenu";
import { TableHeader } from "@common/features/data/components/Table/TableHeader";
import { SortOption } from "@common/features/data/components/IndexFilterBar/filterModel";
import { useStoredColumnOrder } from "@common/features/data/components/Table/utils/useDataColumns";
import { ViewDocument } from "@common/features/data/components/Table/ViewDocument";
import { useDataPageSize } from "@common/features/data/components/Table/utils/useQueryFilteredTable";
import { LoadingLogo } from "@ui/Loading";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { useNents } from "@common/lib/useNents";
import { useColumnDragAndDrop } from "@common/features/data/components/Table/utils/useColumnDragAndDrop";

const getRowId = (d: GenericDocument) => d._id as string;

const pageThreshold = 25;

export function Table({
  activeSchema,
  columns = [],
  data = [],
  localStorageKey = "_disabled_",
  areEditsAuthorized,
  authorizeEdits,
  hasFilters,
  selectedRows,
  loadMore,
  totalRowCount,
  listRef,
  patchDocument,
  tableName,
  componentId,
  isProtectedDeployment,
  hasPopup,
  setPopup,
  deleteRows,
  onAddDraftFilter,
  defaultDocument,
  sort,
  getSortOption,
  onSortColumn,
  hiddenColumns,
  onColumnOrderChange,
}: {
  activeSchema: SchemaJson | null;
  areEditsAuthorized: boolean;
  authorizeEdits?(): void;
  columns: ColumnDef<GenericDocument, any>[];
  data: GenericDocument[]; // array of row data so far
  localStorageKey?: string;
  tableName: string;
  componentId: string | null;
  isProtectedDeployment: boolean;
  selectedRows: SelectionState;
  totalRowCount?: number;
  hasFilters: boolean;
  loadMore: () => void;
  listRef: MutableRefObject<FixedSizeList | null>;
  patchDocument: ReturnType<typeof usePatchDocumentField>;
  hasPopup: boolean;
  setPopup: PopupState["setPopup"];
  deleteRows: (rowIds: Set<string>) => Promise<void>;
  onAddDraftFilter: (newFilter: Filter) => void;
  defaultDocument: GenericDocument;
  sort: {
    order: "asc" | "desc";
    field: string;
  };
  getSortOption: (field: string) => SortOption;
  onSortColumn: (field: string) => void;
  hiddenColumns: string[];
  onColumnOrderChange?: (newOrder: string[]) => void;
}) {
  const [pageSize] = useDataPageSize(componentId, tableName);
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canManageTable = useIsOperationAllowed("WriteData");

  const { selectedNent } = useNents();

  const isInUnmountedComponent = !!(
    selectedNent && selectedNent.state !== "active"
  );

  const [storedColumnOrder, setStoredColumnOrder] =
    useStoredColumnOrder(localStorageKey);

  const dataColumnNames = columns.map((c) => c.id as string);
  // Filter out special columns like *select from ordering operations
  const orderableColumnNames = dataColumnNames.filter(
    (name) => name !== "*select",
  );

  // Filter out hidden columns (but never hide the checkbox column)
  // Use useMemo to avoid recreating the array on every render
  const visibleColumns = useMemo(
    () =>
      columns.filter(
        (c) => c.id === "*select" || !hiddenColumns.includes(c.id as string),
      ),
    [columns, hiddenColumns],
  );

  const columnOrder = [
    "*select",
    ...(storedColumnOrder || orderableColumnNames),
  ];

  const setColumnOrder = useCallback(
    (newColumnOrder: string[]) => {
      // Filter out *select when storing the column order
      const newOrder = newColumnOrder.filter((col) => col !== "*select");
      setStoredColumnOrder(newOrder);
      // Notify parent component of the change
      onColumnOrderChange?.(newOrder);
    },
    [setStoredColumnOrder, onColumnOrderChange],
  );

  const table = useReactTable({
    columns: visibleColumns,
    data,
    getRowId,
    getCoreRowModel: getCoreRowModel(),
    columnResizeMode: "onChange",
    state: { columnOrder },
  });

  const headerGroups = table.getHeaderGroups();
  const { rows } = table.getRowModel();

  trackDataColumnChanges(
    orderableColumnNames,
    storedColumnOrder,
    setColumnOrder,
  );

  const reorderColumns = useCallback(
    (item: { index: number }, newIndex: number) => {
      const { index: currentIndex } = item;

      const currentItem = columnOrder[currentIndex];

      const newColumnOrder = [...columnOrder];
      newColumnOrder.splice(currentIndex, 1);
      newColumnOrder.splice(newIndex, 0, currentItem);

      // Ensure *select always stays at the beginning
      const filtered = newColumnOrder.filter((col) => col !== "*select");
      setColumnOrder(["*select", ...filtered]);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [setColumnOrder, JSON.stringify(columnOrder)],
  );

  const resetColumnWidths = useTrackColumnWidths(table, localStorageKey);

  const isResizingColumn =
    table.getState().columnSizingInfo.isResizingColumn || undefined;

  // Resize handlers per column so cells can offer the same resize affordance
  // as the column header.
  const columnResizeHandlers: Record<string, (event: unknown) => void> = {};
  for (const header of table.getFlatHeaders()) {
    columnResizeHandlers[header.column.id] = header.getResizeHandler();
  }

  const [
    ,
    {
      has: isRowSelected,
      toggle: toggleIsRowSelected,
      toggleAll,
      all: allRowsSelected,
      isExhaustive: isSelectionExhaustive,
    },
  ] = selectedRows;

  const outerRef = useRef<HTMLElement>(null);
  const tableContainerRef = useRef<HTMLDivElement>(null);

  const [, forceRerender] = useReducer((x) => x + 1, 0);

  const [topBorderAnimation, setTopBorderAnimation] = useState(false);
  const animateTopBorder = useCallback(() => {
    setTopBorderAnimation(true);
    setTimeout(() => setTopBorderAnimation(false), 1000);
  }, []);

  const { densityValues } = useTableDensity();

  useMaintainScrollPositionOnChange(
    data,
    outerRef,
    getRowId,
    densityValues.height,
    animateTopBorder,
  );

  const { contextMenuState, openContextMenu, closeContextMenu } =
    useTableContextMenuState();
  const [collapsed, setCollapsed] = useState(false);

  const panelRef = useRef<ImperativePanelHandle>(null);

  // Column drag and drop setup
  const {
    sensors,
    dragOffset,
    activeColumn,
    activeColumnPosition,
    handleDragStart,
    handleDragMove,
    handleDragEnd,
    handleDragCancel,
  } = useColumnDragAndDrop({
    headerGroups,
    reorderColumns,
    columnOrder,
  });

  const onEditDocument = useCallback(
    (document: GenericDocument) => {
      setPopup({
        type: "editDocument",
        document,
        tableName,
      });
    },
    [setPopup, tableName],
  );

  return (
    <PanelGroup
      direction="horizontal"
      className="w-full"
      autoSaveId="documentViewer"
    >
      <Panel
        defaultSize={100}
        className="relative w-full overflow-x-hidden rounded-b-lg"
      >
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragStart={handleDragStart}
          onDragMove={handleDragMove}
          onDragEnd={handleDragEnd}
          onDragCancel={handleDragCancel}
        >
          <SortableContext
            items={columnOrder}
            strategy={horizontalListSortingStrategy}
          >
            <div
              role="table"
              ref={tableContainerRef}
              className={classNames(
                "flex w-full h-full overflow-y-hidden",
                "scrollbar",
              )}
            >
              <div className="flex flex-auto flex-col">
                <TableHeader
                  key={columnOrder.join(",")}
                  headerGroups={headerGroups}
                  isResizingColumn={isResizingColumn}
                  allRowsSelected={allRowsSelected}
                  hasFilters={hasFilters}
                  isSelectionExhaustive={isSelectionExhaustive}
                  toggleAll={toggleAll}
                  topBorderAnimation={topBorderAnimation}
                  openContextMenu={openContextMenu}
                  sort={sort}
                  getSortOption={getSortOption}
                  onSortColumn={onSortColumn}
                  localStorageKey={localStorageKey}
                  tableContainerRef={tableContainerRef}
                />
                {/* Body */}
                <div
                  role="rowgroup"
                  className="-mt-px w-full flex-auto"
                  id="dataTable"
                >
                  <InfiniteScrollList
                    className="scrollbar-none"
                    items={rows}
                    totalNumItems={totalRowCount}
                    overscanCount={25}
                    pageSize={pageSize}
                    loadMoreThreshold={pageThreshold}
                    itemSize={densityValues.height}
                    itemData={{
                      areEditsAuthorized,
                      isRowSelected,
                      isSelectionAllNonExhaustive:
                        !isSelectionExhaustive && allRowsSelected === true,
                      authorizeEdits,
                      patchDocument,
                      rows,
                      columnResizeHandlers,
                      tableName,
                      toggleIsRowSelected,
                      onOpenContextMenu: openContextMenu,
                      onCloseContextMenu: closeContextMenu,
                      contextMenuRow:
                        contextMenuState?.selectedCell?.rowId ?? null,
                      contextMenuColumn:
                        contextMenuState?.selectedCell?.column ?? null,
                      canManageTable: canManageTable && !isInUnmountedComponent,
                      activeSchema,
                      resizingColumn: isResizingColumn,
                      onEditDocument,
                    }}
                    RowOrLoading={DataRow}
                    loadMore={loadMore}
                    listRef={listRef}
                    outerRef={outerRef}
                    onScroll={() => {
                      // Force a re-render so the TableScrollbar gets updated.
                      forceRerender();
                    }}
                    itemKey={(idx) => (data[idx] as any)?._id || idx}
                  />
                </div>
                <TableScrollbar
                  totalRowCount={totalRowCount}
                  outerRef={outerRef}
                  listRef={listRef}
                />
              </div>

              <TableContextMenu
                data={data}
                state={contextMenuState}
                close={closeContextMenu}
                setPopup={setPopup}
                deleteRows={deleteRows}
                onAddDraftFilter={onAddDraftFilter}
                defaultDocument={defaultDocument}
                canManageTable={canManageTable}
                activeSchema={activeSchema}
                tableName={tableName}
                resetColumns={() => {
                  setColumnOrder(["*select", ...orderableColumnNames]);
                  setStoredColumnOrder(orderableColumnNames);
                  resetColumnWidths();
                }}
                isProtectedDeployment={isProtectedDeployment}
              />
            </div>
          </SortableContext>
          {/* Static drag overlay positioned over the column being dragged */}
          {activeColumn &&
            activeColumnPosition !== null &&
            (() => {
              const columnWidth = activeColumn.getSize();

              const containerWidth =
                tableContainerRef.current?.offsetWidth || 0;
              const scrollLeft = tableContainerRef.current?.scrollLeft || 0;

              // Adjust for horizontal scroll
              const unclamped = activeColumnPosition + dragOffset - scrollLeft;

              // Clamp the position so the column stays within bounds
              const left = Math.max(
                0,
                Math.min(unclamped, containerWidth - columnWidth),
              );

              return (
                <div
                  className="pointer-events-none absolute top-0 rounded-sm border border-border-selected bg-background-primary/50 shadow-lg"
                  style={{
                    left,
                    width: columnWidth,
                    height: tableContainerRef.current?.offsetHeight || "100%",
                  }}
                />
              );
            })()}
        </DndContext>
      </Panel>
      {!hasPopup && selectedRows[0].size > 0 && (
        <>
          <ResizeHandle
            collapsed={collapsed}
            direction="left"
            panelRef={panelRef}
            handleTitle="View Selected"
          />
          <Panel
            defaultSize={30}
            minSize={10}
            maxSize={80}
            collapsible
            collapsedSize={0}
            onCollapse={() => setCollapsed(true)}
            onExpand={() => setCollapsed(false)}
            ref={panelRef}
            className={cn(
              "max-h-full overflow-x-auto bg-background-primary",
              !collapsed && "min-w-56",
            )}
          >
            <ViewDocument
              rows={data.filter((d) =>
                Array.from(selectedRows[0]).includes(d._id as string),
              )}
              columns={orderableColumnNames}
              tableName={tableName}
              componentId={componentId}
              canManageTable={canManageTable}
              areEditsAuthorized={areEditsAuthorized}
              authorizeEdits={authorizeEdits}
              activeSchema={activeSchema}
            />
          </Panel>
        </>
      )}
    </PanelGroup>
  );
}

export function TableSkeleton() {
  return (
    <div className="flex h-full items-center justify-center rounded-b-lg border bg-background-secondary">
      <LoadingLogo />
    </div>
  );
}

// Checks if there are any new or removed columns in the data set
// and updates the column order accordingly.
function trackDataColumnChanges(
  columns: string[],
  storedColumnOrder: string[] | undefined,
  updateColumnOrder: (newOrder: string[]) => void,
) {
  if (!storedColumnOrder) {
    return;
  }

  // Find columns that are not in the stored order and add them to the end
  const newColumns = columns
    .filter((c) => !storedColumnOrder.includes(c))
    // New columns should be sorted alphabetically after the existing sort order.
    .sort((a, b) => a.localeCompare(b));

  const existingColumns = storedColumnOrder.filter((c) => columns.includes(c));

  // There are no new columns and no columns have been removed, so we don't need to make an update.
  if (
    newColumns.length === 0 &&
    existingColumns.length === storedColumnOrder.length
  ) {
    return;
  }

  const lastColumn = existingColumns[existingColumns.length - 1];

  // If _creationTime is the last column, we should respect that and
  //  insert the new columns before it.
  const newOrder =
    lastColumn === "_creationTime"
      ? [
          ...existingColumns.filter((c) => c !== "_creationTime"),
          ...newColumns,
          "_creationTime",
        ]
      : [...existingColumns, ...newColumns];

  // Prepend *select to ensure it's always first
  updateColumnOrder(["*select", ...newOrder]);
}
