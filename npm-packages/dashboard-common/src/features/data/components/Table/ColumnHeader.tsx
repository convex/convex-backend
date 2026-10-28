import {
  CaretSortIcon,
  CaretUpIcon,
  CalendarIcon,
  DragHandleDots2Icon,
} from "@radix-ui/react-icons";
import classNames from "classnames";
import { GenericDocument } from "convex/server";
import { flexRender, Header } from "@tanstack/react-table";
import { useSortable } from "@dnd-kit/sortable";
import { useRef, useState, RefObject } from "react";
import { useContextMenuTrigger } from "@common/features/data/lib/useContextMenuTrigger";
import { useTableDensity } from "@common/features/data/lib/useTableDensity";
import { Checkbox } from "@ui/Checkbox";
import { identifierNeedsEscape } from "@common/features/data/lib/helpers";
import { emptyColumnName } from "@common/features/data/components/Table/utils/useDataColumns";
import { DataCellProps } from "@common/features/data/components/Table/DataCell/DataCell";
import { columnWidthToString } from "@common/features/data/components/Table/DataRow";
import { Tooltip } from "@ui/Tooltip";
import { cn } from "@ui/cn";
import { Button } from "@ui/Button";
import { useStoredShowFieldsAsDates } from "@common/features/data/components/Table/utils/useDataColumns";
import { SortOption } from "@common/features/data/components/IndexFilterBar/filterModel";

type ColumnHeaderProps = {
  header: Header<GenericDocument, unknown>;
  columnIndex: number;
  allRowsSelected: boolean | "indeterminate";
  hasFilters: boolean;
  isSelectionExhaustive: boolean;
  toggleAll: () => void;
  isResizingColumn?: string;
  isLastColumn: boolean;
  openContextMenu: DataCellProps["onOpenContextMenu"];
  sort?: "asc" | "desc";
  sortOption: SortOption;
  onSort: () => void;
  localStorageKey: string;
  tableContainerRef: RefObject<HTMLDivElement | null>;
};

export function ColumnHeader({
  header,
  columnIndex,
  allRowsSelected = false,
  hasFilters,
  isSelectionExhaustive,
  toggleAll,
  isResizingColumn,
  isLastColumn,
  openContextMenu,
  sort,
  sortOption,
  onSort,
  localStorageKey,
  tableContainerRef,
}: ColumnHeaderProps) {
  const canDragOrDrop = columnIndex !== 0 && !isResizingColumn;

  const headerNode = useRef<HTMLDivElement | null>(null);

  const { column } = header;
  const columnName = column.id;
  const columnId = column.id;

  const { attributes, listeners, setNodeRef, isDragging, isOver, active } =
    useSortable({
      id: columnId,
      disabled: !canDragOrDrop,
    });

  // Always drop to the right of the hovered column
  const direction =
    isOver && !isDragging && active && active.id !== columnId
      ? "right"
      : undefined;
  const isHovering = isOver && !isDragging && active?.id !== columnId;
  useContextMenuTrigger(
    headerNode,
    (pos) =>
      openContextMenu(pos, null, {
        column: columnName,
        value: undefined,
      }),
    () => {},
  );

  const { densityValues } = useTableDensity();
  const width = columnWidthToString(header.getSize());

  const [isHovered, setIsHovered] = useState(false);

  return (
    // eslint-disable-next-line jsx-a11y/interactive-supports-focus -- the mouse listeners only track hover; the header's interactive controls are its inner buttons
    <div
      role="columnheader"
      style={{ width, height: densityValues.height }}
      ref={setNodeRef}
      className={classNames(
        isDragging && "opacity-50",
        "font-semibold text-left text-xs bg-background-secondary text-content-secondary tracking-wider",
        "select-none duration-300 transition-colors",
        "border-r",
        "relative shrink-0",
      )}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
    >
      {/* Show a vertical line on the right side where the column will be dropped */}
      {isHovering && direction && (
        <div
          className="absolute top-0 right-0 z-10 w-0.5 bg-util-accent"
          style={{
            height: tableContainerRef.current?.offsetHeight || "100%",
          }}
        />
      )}
      {/* Show a vertical line when resizing this column */}
      {isResizingColumn === columnName && (
        <div
          className="absolute top-0 right-0 z-10 w-0.5 bg-util-accent"
          style={{
            height: tableContainerRef.current?.offsetHeight || "100%",
          }}
        />
      )}
      <div
        ref={headerNode}
        className="flex size-full items-center"
        style={{
          padding: `${densityValues.paddingY}px ${columnIndex === 0 ? "12" : densityValues.paddingX}px`,
          width,
        }}
      >
        <div className="flex min-w-0 flex-1 items-center space-x-2">
          {columnIndex === 0 ? (
            // Disable the "Select all" checkbox when filtering
            allRowsSelected === false &&
            hasFilters &&
            !isSelectionExhaustive ? null : (
              <Checkbox checked={allRowsSelected} onChange={toggleAll} />
            )
          ) : columnName === emptyColumnName ? (
            <i>empty</i>
          ) : (
            <SortableColumnName
              columnName={columnName}
              sort={sort}
              sortOption={sortOption}
              onSort={onSort}
              isHovered={isHovered}
            >
              {identifierNeedsEscape(columnName) ? (
                <span
                  className={`before:text-content-primary before:content-['"'] after:text-content-primary after:content-['"']`}
                >
                  {flexRender(column.columnDef.header, header.getContext())}
                </span>
              ) : (
                <div>
                  {flexRender(column.columnDef.header, header.getContext())}
                </div>
              )}
            </SortableColumnName>
          )}
          {columnName !== "_creationTime" &&
            column.columnDef.meta?.isDateLike && (
              <DateDisplayToggle
                columnName={columnName}
                isDate={column.columnDef.meta?.isDate ?? false}
                localStorageKey={localStorageKey}
              />
            )}
        </div>
      </div>
      {canDragOrDrop && isHovered && (
        <Button
          {...attributes}
          {...listeners}
          className={cn(
            "absolute top-1/2 right-1.5 -translate-y-1/2 animate-fadeInFromLoading cursor-grab items-center bg-background-secondary/50 text-content-secondary backdrop-blur-[2px]",
            isDragging && "cursor-grabbing",
          )}
          aria-label="Drag column"
          variant="neutral"
          inline
          size="xs"
          icon={<DragHandleDots2Icon />}
        />
      )}
      {!isHovering && column.getCanResize() && columnName !== "*select" && (
        // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- mouse/touch-driven column resize handle
        <div
          role="separator"
          onMouseDown={header.getResizeHandler()}
          onTouchStart={header.getResizeHandler()}
          className="absolute top-0 z-20 inline-block h-full cursor-col-resize touch-none select-none"
          style={{
            width: densityValues.paddingX * (isLastColumn ? 1 : 2),
            right: isLastColumn ? 0 : -densityValues.paddingX,
          }}
        />
      )}
    </div>
  );
}

// The column name doubles as the sort control. Sorting follows an index, so a
// column can only be sorted when some index starts with it (or continues the
// applied indexed filters); otherwise the tooltip says which index to add.
function SortableColumnName({
  columnName,
  sort,
  sortOption,
  onSort,
  isHovered,
  children,
}: {
  columnName: string;
  sort?: "asc" | "desc";
  sortOption: SortOption;
  onSort: () => void;
  isHovered: boolean;
  children: React.ReactNode;
}) {
  if (sortOption.kind === "switch" && sortOption.dropsClauses) {
    return (
      <span className="flex items-center gap-1">
        {children}
        {isHovered && (
          <Tooltip
            tip="Sorting by this field will clear your active indexed filters."
            side="bottom"
          >
            <span className="cursor-not-allowed opacity-40">
              <CaretSortIcon className="shrink-0 text-content-tertiary" />
            </span>
          </Tooltip>
        )}
      </span>
    );
  }
  if (sortOption.kind === "unavailable") {
    return (
      <span className="flex items-center gap-1">
        {children}
        {isHovered && (
          <Tooltip
            tip="To sort by this field, add an index for it to your schema."
            side="bottom"
          >
            <span className="cursor-not-allowed opacity-40">
              <CaretSortIcon className="shrink-0 text-content-tertiary" />
            </span>
          </Tooltip>
        )}
      </span>
    );
  }
  return (
    <span className="flex items-center gap-1">
      {children}
      {(sort || isHovered) && (
        <Button
          variant="unstyled"
          onClick={onSort}
          className="flex size-5 cursor-pointer items-center justify-center rounded-full hover:bg-background-tertiary"
          aria-label={`Sort by ${columnName}`}
          icon={
            sort ? (
              <CaretUpIcon
                className={cn(
                  "transition-all",
                  sort === "asc" ? "" : "rotate-180",
                )}
              />
            ) : (
              <CaretSortIcon className="text-content-tertiary" />
            )
          }
        />
      )}
    </span>
  );
}

// Lets the user override, per deployment → table → column, whether
// timestamp-like numbers in this column are displayed as dates. Clicking the
// icon toggles the choice, which is persisted in local storage (see
// `useStoredShowFieldsAsDates`).
function DateDisplayToggle({
  columnName,
  isDate,
  localStorageKey,
}: {
  columnName: string;
  isDate: boolean;
  localStorageKey: string;
}) {
  const [showFieldsAsDates, setShowFieldsAsDates] =
    useStoredShowFieldsAsDates(localStorageKey);

  return (
    <Button
      variant="unstyled"
      className="flex items-center text-content-secondary hover:text-content-primary"
      aria-label={
        isDate ? "Show this field as a number" : "Show this field as a date"
      }
      tip="Switch between rendering this field as a number or date."
      onClick={() =>
        setShowFieldsAsDates({ ...showFieldsAsDates, [columnName]: !isDate })
      }
      icon={
        isDate ? (
          <CalendarIcon />
        ) : (
          <span
            aria-hidden
            className="flex size-[15px] items-center justify-center font-mono text-sm leading-none font-semibold"
          >
            #
          </span>
        )
      }
    />
  );
}
