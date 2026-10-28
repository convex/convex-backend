import { useRouter } from "next/router";
import {
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { FixedSizeList } from "react-window";

import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import classNames from "classnames";
import {
  Filter,
  FilterExpression,
  SchemaJson,
} from "system-udfs/convex/_system/frontend/lib/filters";
import { LoadingLogo, LoadingTransition } from "@ui/Loading";
import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { useSelectionState } from "@common/features/data/lib/useSelectionState";
import { useDataToolbarActions } from "@common/features/data/lib/useDataToolbarActions";
import { useTableFilters } from "@common/features/data/lib/useTableFilters";
import { FiltersAppliedProperties } from "@common/features/data/lib/filterAnalytics";
import { useToolPopup } from "@common/features/data/lib/useToolPopup";
import { useEditsAuthorization } from "@common/features/data/lib/useEditsAuthorization";
import { usePatchDocumentField } from "@common/features/data/components/Table/utils/usePatchDocumentField";
import {
  Table,
  TableSkeleton,
} from "@common/features/data/components/Table/Table";
import {
  DataToolbar,
  DataToolbarSkeleton,
} from "@common/features/data/components/DataToolbar/DataToolbar";
import { EmptyDataContent } from "@common/features/data/components/EmptyData";
import {
  useDataColumns,
  useStoredHiddenColumns,
  useStoredColumnOrder,
} from "@common/features/data/components/Table/utils/useDataColumns";
import { useQueryFilteredTable } from "@common/features/data/components/Table/utils/useQueryFilteredTable";
import { useSingleTableSchemaStatus } from "@common/features/data/components/TableSchema";
import { useTableFields } from "@common/features/data/components/Table/utils/useTableFields";
import { useDefaultDocument } from "@common/features/data/lib/useDefaultDocument";
import {
  ImperativePanelHandle,
  Panel,
  PanelGroup,
} from "react-resizable-panels";
import { cn } from "@ui/cn";

import { api } from "system-udfs/convex/_generated/api";
import { useNents } from "@common/lib/useNents";
import omit from "lodash/omit";
import { IndexFilterBar } from "./IndexFilterBar/IndexFilterBar";
import { clearFilters } from "./IndexFilterBar/clearFilters";
import {
  EMPTY_FILTERS,
  buildIndexDefs,
  currentOrder,
  effectiveSortField,
} from "./IndexFilterBar/filterModel";
import { useFilterActions } from "./IndexFilterBar/useFilterActions";

export function DataContent({
  tableName,
  componentId,
  activeSchema,
  onDocumentsAdded,
  onFiltersApplied,
}: {
  tableName: string;
  componentId: string | null;
  activeSchema: SchemaJson | null;
  onDocumentsAdded?: (count: number) => void;
  onFiltersApplied?: (properties: FiltersAppliedProperties) => void;
}) {
  const { filters, applyFiltersWithHistory, hasFilters } = useTableFilters(
    tableName,
    onFiltersApplied,
  );

  const [draftFilters, setDraftFilters] = useState<
    FilterExpression | undefined
  >(filters);
  useEffect(() => {
    setDraftFilters(filters);
  }, [filters]);
  const router = useRouter();

  const tableSchemaStatus = useSingleTableSchemaStatus(tableName);
  const numRowsInTable = useQuery(udfs.tableSize.default, {
    tableName,
    componentId,
  });

  const hasFiltersAndAtLeastOneDocument =
    hasFilters && numRowsInTable !== undefined && numRowsInTable > 0;

  const {
    status,
    loadNextPage,
    staleAsOf,
    isLoading,
    data,
    errors,
    numRowsReadEstimate,
    isPaused,
  } = useQueryFilteredTable(tableName);

  const numRowsRead = Math.min(numRowsReadEstimate, numRowsInTable || 0);

  const { useCurrentDeployment, useIsProtectedDeployment } = useContext(
    DeploymentInfoContext,
  );
  const deployment = useCurrentDeployment();
  const isProd = deployment?.deploymentType === "prod";
  const isProtectedDeployment = useIsProtectedDeployment();

  const localStorageKey =
    router.query && `${router.query.deploymentName}/${tableName}`;

  const ref = useRef<ImperativePanelHandle>(null);

  const allIds = useMemo(
    () => new Set(data.map((row) => row._id as string)),
    [data],
  );

  const selectedRows = useSelectionState(allIds, status === "Exhausted");

  const tableFields = useTableFields(tableName, activeSchema, data);

  const columns = useDataColumns({
    tableName,
    localStorageKey,
    fields: tableFields,
    data,
    // Subtract 3 border pixels, one on each side of the parent box
    // and one more on the right side of the last column.
    width: (ref.current?.getSize() || 1000) - 3,
  });

  const [hiddenColumnsRaw, setHiddenColumnsRaw] =
    useStoredHiddenColumns(localStorageKey);

  // Default to showing only 25 fields (including _id and _creationTime)
  const hiddenColumns = useMemo(() => {
    if (hiddenColumnsRaw !== undefined) {
      return hiddenColumnsRaw;
    }

    // First time - hide fields beyond the first 25
    // Ensure _id and _creationTime are always visible
    const visibleFields: string[] = [];
    const allTableFields = [...tableFields];

    // Add _id and _creationTime first if they exist
    if (allTableFields.includes("_id")) {
      visibleFields.push("_id");
    }
    if (allTableFields.includes("_creationTime")) {
      visibleFields.push("_creationTime");
    }

    // Add remaining fields up to 25 total
    for (const field of allTableFields) {
      if (
        field !== "_id" &&
        field !== "_creationTime" &&
        visibleFields.length < 25
      ) {
        visibleFields.push(field);
      }
    }

    // Hide everything else
    return allTableFields.filter((field) => !visibleFields.includes(field));
  }, [hiddenColumnsRaw, tableFields]);

  // Wrap the setter to handle undefined -> [] conversion for functional updates
  const setHiddenColumns = useCallback(
    (newHiddenColumns: string[] | ((prev: string[]) => string[])) => {
      if (typeof newHiddenColumns === "function") {
        setHiddenColumnsRaw((prev) => newHiddenColumns(prev || []));
      } else {
        setHiddenColumnsRaw(newHiddenColumns);
      }
    },
    [setHiddenColumnsRaw],
  );

  // Column order management
  const [columnOrderRaw, setColumnOrderRaw] =
    useStoredColumnOrder(localStorageKey);

  const columnOrder = useMemo(() => columnOrderRaw || [], [columnOrderRaw]);

  // Wrap the setter to handle undefined -> [] conversion for functional updates
  const setColumnOrder = useCallback(
    (newColumnOrder: string[] | ((prev: string[]) => string[])) => {
      if (typeof newColumnOrder === "function") {
        setColumnOrderRaw((prev) => newColumnOrder(prev || []));
      } else {
        setColumnOrderRaw(newColumnOrder);
      }
    },
    [setColumnOrderRaw],
  );

  // Use tableFields directly for the combobox instead of deriving from columns
  // to avoid circular dependencies
  const allFields = useMemo(() => ["*select", ...tableFields], [tableFields]);

  const listRef = useRef<FixedSizeList>(null);

  const scrollToTop = useCallback(() => listRef.current?.scrollToItem(0), []);

  const [rowsThatAreSelected, { reset: clearSelectedRows, all: allSelected }] =
    selectedRows;

  const [previousTableName, setPreviousTableName] = useState(tableName);
  if (tableName !== previousTableName) {
    setPreviousTableName(tableName);
    clearSelectedRows();
  }

  const patchDocumentField = usePatchDocumentField(tableName);

  const { areEditsAuthorized, authorizeEdits } = useEditsAuthorization();

  const { addDocuments, patchFields, clearTable, deleteTable, deleteRows } =
    useDataToolbarActions({
      // Scrolling to the first item when a new document is added
      // for works now while we are guaranteed to be sorting by creation time.
      handleAddDocuments: scrollToTop,
      clearSelectedRows,
      loadMore: loadNextPage,
      tableName,
      onDocumentsAdded,
    });

  const allRowsSelected =
    allSelected === true && !hasFiltersAndAtLeastOneDocument;

  const popupState = useToolPopup({
    addDocuments: (table, docs) => addDocuments(table, docs),
    patchFields: (table, rowIds, fields) => patchFields(table, rowIds, fields),
    clearSelectedRows,
    clearTable,
    deleteRows: (rowIds) => deleteRows(rowIds),
    deleteTable,
    isProd,
    numRows: numRowsInTable,
    tableName,
    areEditsAuthorized,
    authorizeEdits,
    activeSchema,
  });
  const { popupEl } = popupState;

  // Handle query parameter to open the indexes panel
  useEffect(() => {
    if (!!router.query.showIndexes && !popupState.popup) {
      popupState.setPopup({ type: "viewIndexes", tableName });
      void router.push(
        {
          pathname: router.pathname,
          query: omit(router.query, "showIndexes"),
        },
        undefined,
        { shallow: true },
      );
    }
  }, [router.query.showIndexes, router, popupState, tableName]);

  const selectedDocumentId = rowsThatAreSelected.values().next().value;
  const selectedDocument = data.find((row) => row._id === selectedDocumentId);
  const defaultDocument = useDefaultDocument(tableName);
  const { selectedNent } = useNents();
  const indexes =
    useQuery(api._system.frontend.indexes.default, {
      tableName,
      tableNamespace: selectedNent?.id ?? null,
    }) ?? undefined;
  const indexDefs = useMemo(() => buildIndexDefs(indexes), [indexes]);
  const appliedFilters = filters ?? EMPTY_FILTERS;
  const filterActions = useFilterActions({
    filters,
    draftFilters,
    setDraftFilters,
    applyFilters: applyFiltersWithHistory,
    indexDefs,
    defaultDocument,
  });
  const sort = {
    order: currentOrder(appliedFilters),
    field: effectiveSortField(indexDefs, appliedFilters),
  };

  const { captureMessage } = useContext(DeploymentInfoContext);
  useEffect(() => {
    if (
      status !== "LoadingFirstPage" &&
      !(data.length || status === "CanLoadMore") &&
      !hasFiltersAndAtLeastOneDocument &&
      !isLoading
    ) {
      captureMessage(
        `Encountered unexpected state in data page: status: ${status}, numRowsInTable: ${numRowsInTable}, numRowsRead: ${numRowsRead}, isLoading: ${isLoading}`,
        "warning",
      );
    }
  }, [
    status,
    data.length,
    hasFiltersAndAtLeastOneDocument,
    isLoading,
    numRowsInTable,
    numRowsRead,
    captureMessage,
  ]);

  return (
    <PanelGroup
      direction="horizontal"
      className={cn(
        "scrollbar flex size-full min-w-[20rem] overflow-x-auto pl-6",
        popupEl ? "pr-0" : "pr-6",
      )}
      autoSaveId="data-content"
    >
      <Panel
        className={cn(
          "flex shrink flex-col gap-2 overflow-hidden py-4",
          "max-w-full",
          popupEl ? "min-w-[16rem]" : "min-w-[20rem]",
        )}
        ref={ref}
        defaultSize={80}
        minSize={10}
      >
        <DataToolbar
          popupState={popupState}
          deleteRows={deleteRows}
          selectedRowsIds={rowsThatAreSelected}
          allRowsSelected={allRowsSelected === true}
          selectedDocument={selectedDocument}
          numRows={numRowsInTable}
          tableSchemaStatus={tableSchemaStatus}
          tableName={tableName}
          isLoadingMore={isLoading && !isPaused}
        />

        <div className="flex h-full max-h-full flex-col overflow-y-hidden rounded-b-lg">
          {numRowsInTable !== undefined && numRowsInTable > 0 && (
            <IndexFilterBar
              actions={filterActions}
              indexDefs={indexDefs}
              tableName={tableName}
              tableFields={tableFields}
              defaultDocument={defaultDocument}
              dataFetchErrors={errors}
              activeSchema={activeSchema}
              numRows={numRowsInTable}
              numRowsLoaded={data.length}
              hasFilters={hasFiltersAndAtLeastOneDocument}
              allFields={allFields}
              hiddenColumns={hiddenColumns}
              setHiddenColumns={setHiddenColumns}
              columnOrder={columnOrder}
              setColumnOrder={setColumnOrder}
            />
          )}

          <LoadingTransition
            loadingState={
              <div className="flex h-full flex-col items-center justify-center gap-8 rounded-b-lg border bg-background-secondary">
                <LoadingLogo />
              </div>
            }
            loadingProps={{ shimmer: false }}
          >
            {status !== "LoadingFirstPage" &&
              (data.length || status === "CanLoadMore" ? (
                <Sheet
                  className={classNames("w-full relative rounded-t-none")}
                  padding={false}
                >
                  {!isPaused && staleAsOf > 0 && (
                    <LoadingFilteredData
                      numRowsRead={numRowsRead}
                      numRowsInTable={numRowsInTable}
                      overlay
                    />
                  )}
                  <Table
                    key={columnOrder.join(",")}
                    activeSchema={activeSchema}
                    listRef={listRef}
                    loadMore={loadNextPage}
                    sort={sort}
                    getSortOption={filterActions.sortOptionFor}
                    onSortColumn={filterActions.sortByField}
                    totalRowCount={
                      filters
                        ? status === "Exhausted"
                          ? data.length
                          : // If we are filtering, we need to add 1 to the total row count to
                            // allow the infinite loader to load more documents when scrolling.
                            data.length + 1
                        : numRowsInTable
                    }
                    hasFilters={hasFiltersAndAtLeastOneDocument}
                    patchDocument={patchDocumentField}
                    selectedRows={selectedRows}
                    areEditsAuthorized={areEditsAuthorized}
                    authorizeEdits={authorizeEdits}
                    tableName={tableName}
                    componentId={componentId}
                    isProtectedDeployment={isProtectedDeployment}
                    data={data}
                    columns={columns}
                    localStorageKey={localStorageKey}
                    hasPopup={!!popupEl}
                    setPopup={popupState.setPopup}
                    deleteRows={deleteRows}
                    defaultDocument={defaultDocument}
                    hiddenColumns={hiddenColumns}
                    onColumnOrderChange={setColumnOrder}
                    onAddDraftFilter={(filter: Filter) =>
                      filterActions.addComplete(filter)
                    }
                  />
                </Sheet>
              ) : hasFiltersAndAtLeastOneDocument ? (
                isLoading ? (
                  <Sheet
                    className="flex w-full grow animate-fadeIn items-center justify-center rounded-t-none"
                    padding={false}
                  >
                    <LoadingFilteredData
                      numRowsRead={numRowsRead}
                      numRowsInTable={numRowsInTable}
                    />
                  </Sheet>
                ) : isEmptySearchFilter(filters) ? (
                  <div className="flex h-full flex-1 flex-col items-center gap-3 rounded-t-none border bg-background-secondary pt-8">
                    <div className="text-content-secondary">
                      Enter a search term to find matching documents.
                    </div>
                    <Button
                      onClick={() =>
                        applyFiltersWithHistory(clearFilters(filters))
                      }
                      size="xs"
                    >
                      Clear filters
                    </Button>
                  </div>
                ) : (
                  <div className="flex h-full flex-1 flex-col items-center gap-3 rounded-t-none border bg-background-secondary pt-8">
                    <div className="text-content-secondary">
                      No documents match the selected filters.
                    </div>
                    <Button
                      onClick={() =>
                        applyFiltersWithHistory(clearFilters(filters))
                      }
                      size="xs"
                    >
                      Clear filters
                    </Button>
                  </div>
                )
              ) : isLoading ||
                (numRowsInTable !== undefined &&
                  numRowsInTable > 0) ? null /* Loading */ : (
                <EmptyDataContent
                  openAddDocuments={() =>
                    popupState.setPopup({ type: "addDocuments", tableName })
                  }
                />
              ))}
          </LoadingTransition>
          {rowsThatAreSelected.size > 0 && !allRowsSelected && (
            <p className="flex shrink-0 items-center gap-1 pt-1 text-xs text-content-secondary">
              Shift-click a checkbox to select a range of documents
            </p>
          )}
        </div>
      </Panel>
      {popupEl}
    </PanelGroup>
  );
}

function LoadingFilteredData({
  numRowsRead,
  numRowsInTable,
  overlay = false,
}: {
  numRowsRead: number;
  numRowsInTable: any;
  overlay?: boolean;
}) {
  return (
    <div
      className={classNames(
        "flex h-full w-full items-center justify-center",
        overlay &&
          "absolute left-0 top-0 z-10 bg-white/75 dark:bg-black/75 animate-fadeIn",
      )}
    >
      <div className="flex animate-pulse flex-col items-center">
        <p>Applying filters...</p>
        <p>
          Scanned {numRowsRead.toLocaleString()} of{" "}
          {numRowsInTable?.toLocaleString()} documents
        </p>
      </div>
    </div>
  );
}

export function DataContentSkeleton() {
  return (
    <div className="flex h-full grow flex-col gap-2 p-6">
      <DataToolbarSkeleton />
      <TableSkeleton />
    </div>
  );
}

function isEmptySearchFilter(filters: FilterExpression | undefined) {
  return (
    filters?.index &&
    "search" in filters.index &&
    filters.index.search.trim() === ""
  );
}
