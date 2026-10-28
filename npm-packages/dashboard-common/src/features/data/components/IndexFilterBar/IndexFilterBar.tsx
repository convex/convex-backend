import {
  ChevronRightIcon,
  QuestionMarkCircledIcon,
} from "@radix-ui/react-icons";
import { GenericDocument } from "convex/server";
import { useCallback } from "react";
import { FilterValidationError } from "system-udfs/convex/_system/frontend/lib/filters";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { SchemaJson, formatNumberCompact } from "@common/lib/format";
import {
  documentValidatorForTable,
  validatorForFilterField,
} from "@common/features/data/components/Table/utils/validators";
import { FunnelIcon } from "@common/elements/icons";
import { AddFilterMenu } from "./AddFilterMenu";
import { FieldSelector } from "./FieldSelector";
import { FilterChip } from "./FilterChip";
import { IndexSelector, OrderToggle } from "./IndexSelector";
import { FilterActions } from "./useFilterActions";
import {
  IndexDef,
  currentOrder,
  effectiveSortField,
  enabledIndexClauses,
  findIndexDef,
  isSearchFilter,
} from "./filterModel";

// The filter bar above the table: the index name, then one chip per indexed
// clause joined by chevrons, then the index's next field. Unindexed filters
// follow after a gap, ending in the menu that can add a filter on any field.
export function IndexFilterBar({
  actions,
  indexDefs,
  tableName,
  tableFields,
  defaultDocument,
  activeSchema,
  dataFetchErrors,
  numRows,
  numRowsLoaded,
  hasFilters,
  allFields,
  hiddenColumns,
  setHiddenColumns,
  columnOrder,
  setColumnOrder,
}: {
  actions: FilterActions;
  indexDefs: IndexDef[];
  tableName: string;
  tableFields: string[];
  defaultDocument: GenericDocument;
  activeSchema: SchemaJson | null;
  dataFetchErrors?: FilterValidationError[];
  numRows?: number;
  numRowsLoaded: number;
  hasFilters: boolean;
  allFields: string[];
  hiddenColumns: string[];
  setHiddenColumns: (hiddenColumns: string[]) => void;
  columnOrder: string[];
  setColumnOrder: (columnOrder: string[]) => void;
}) {
  const {
    shown,
    filterItems,
    errors,
    openFilterItemKey,
    setOpenFilterItemKey,
    closeFilterItem,
    setFilterItemError,
    indexedNext,
    searchFilterNext,
    searchActive,
    currentIndex,
    addField,
    addScanField,
    addFieldWithIndex,
    optionsFor,
    updateFilterItem,
    removeFilterItem,
    startSearch,
    chooseIndex,
    setOrder,
  } = actions;

  const documentValidator = activeSchema
    ? documentValidatorForTable(activeSchema, tableName)
    : undefined;
  const getValidator = useCallback(
    (fieldName?: string) =>
      documentValidator
        ? validatorForFilterField(documentValidator, tableName, fieldName)
        : undefined,
    [documentValidator, tableName],
  );

  const selectedIndex: IndexDef =
    (isSearchFilter(shown.index)
      ? findIndexDef(indexDefs, shown.index.name)
      : undefined) ?? currentIndex;
  const numRowsWeKnowOf = hasFilters ? numRowsLoaded : numRows;

  const nextIndexField = searchActive
    ? searchFilterNext[0]
    : indexedNext[0]?.field;
  const pathItems = filterItems.filter((c) => c.kind !== "scan");
  const scanItems = filterItems.filter((c) => c.kind === "scan");

  const renderFilterItem = (filterItem: (typeof filterItems)[number]) => (
    <FilterChip
      filterItem={filterItem}
      open={openFilterItemKey === filterItem.key}
      error={
        (errors[filterItem.key]?.shown
          ? errors[filterItem.key]?.message
          : undefined) ??
        (filterItem.kind === "scan"
          ? dataFetchErrors?.find((e) => e.filter === filterItem.position)
              ?.error
          : undefined)
      }
      defaultDocument={defaultDocument}
      getValidator={getValidator}
      shouldSurfaceValidatorErrors={activeSchema?.schemaValidation}
      onOpenChange={(open) =>
        open ? setOpenFilterItemKey(filterItem.key) : closeFilterItem()
      }
      onUpdate={(update) => updateFilterItem(filterItem.key, update)}
      canRemove={
        filterItem.kind === "indexed"
          ? filterItem.isLast
          : filterItem.kind === "search"
            ? !filterItems.some((c) => c.kind === "searchFilter")
            : true
      }
      onRemove={() => removeFilterItem(filterItem.key)}
      onDone={closeFilterItem}
      onError={(messages, shown) =>
        setFilterItemError(filterItem.key, messages, shown)
      }
    />
  );

  return (
    <div
      className="flex w-full flex-col gap-1.5 rounded-t-lg border border-b-0 bg-background-secondary/50 p-2"
      data-testid="indexFilterBar"
    >
      <div className="flex items-start gap-2">
        <div className="flex min-w-0 grow flex-wrap items-center gap-1.5">
          <IndexSelector
            indexDefs={indexDefs}
            current={selectedIndex}
            onChooseIndex={chooseIndex}
            onChooseSearch={startSearch}
          />
          {pathItems.map((item) => (
            <div
              key={item.key}
              className="flex min-w-0 animate-fadeInFromLoading items-center gap-1.5"
            >
              <Separator />
              {renderFilterItem(item)}
            </div>
          ))}
          {nextIndexField && (
            <div
              key={nextIndexField}
              className="flex animate-fadeInFromLoading items-center gap-1.5"
            >
              <Separator />
              <Button
                variant="unstyled"
                aria-label={`Filter by ${nextIndexField}`}
                onClick={() => addField(nextIndexField)}
                className="flex h-6 items-center gap-1 rounded-md border border-dashed px-1.5 font-mono text-xs text-content-secondary hover:border-border-selected hover:text-content-primary"
                icon={<FunnelIcon />}
                data-testid="next-indexed-field"
              >
                {nextIndexField}
              </Button>
            </div>
          )}
          {/* Unindexed filters sit apart from the index path behind a wider
              gap instead of a divider. It is a spacer element rather than a
              margin on the group so that wrapped lines aren't indented. */}
          <span aria-hidden className="w-0.5 shrink-0" />
          <div className="flex min-w-0 flex-wrap items-center gap-1.5">
            {scanItems.map((item) => (
              <div
                key={item.key}
                className="flex min-w-0 animate-fadeInFromLoading"
              >
                {renderFilterItem(item)}
              </div>
            ))}
            <AddFilterMenu
              fields={tableFields}
              optionsFor={optionsFor}
              onAddScan={addScanField}
              onAddIndexed={addField}
              onAddWithIndex={addFieldWithIndex}
              onStartSearch={startSearch}
            />
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <OrderToggle
            order={currentOrder(shown)}
            sortField={effectiveSortField(indexDefs, shown)}
            indexName={selectedIndex.name}
            disabled={searchActive || enabledIndexClauses(shown).length > 0}
            onChange={setOrder}
          />
          <FieldSelector
            allFields={allFields}
            hiddenColumns={hiddenColumns}
            setHiddenColumns={setHiddenColumns}
            columnOrder={columnOrder}
            setColumnOrder={setColumnOrder}
          />
          {numRowsWeKnowOf !== undefined && (
            <div className="flex items-center gap-1 text-xs whitespace-nowrap">
              <span
                className="font-semibold"
                title={numRowsWeKnowOf.toLocaleString()}
              >
                {formatNumberCompact(numRowsWeKnowOf)}
              </span>
              {numRowsWeKnowOf === 1 ? "document" : "documents"}
              {hasFilters && (
                <>
                  {numRowsWeKnowOf !== numRows && " loaded"}
                  <Tooltip
                    tip="Filtered results are paginated and more documents will be loaded as you scroll."
                    side="left"
                  >
                    <QuestionMarkCircledIcon />
                  </Tooltip>
                </>
              )}
            </div>
          )}
        </div>
      </div>
      {dataFetchErrors && dataFetchErrors.length > 0 && (
        <p
          className="text-xs wrap-break-word text-content-errorSecondary"
          role="alert"
        >
          {dataFetchErrors[0].error}
        </p>
      )}
    </div>
  );
}

function Separator() {
  return (
    <ChevronRightIcon
      className="size-3 shrink-0 text-content-tertiary"
      aria-hidden
    />
  );
}
