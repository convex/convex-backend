import {
  DragHandleDots2Icon,
  EyeNoneIcon,
  EyeOpenIcon,
  MagnifyingGlassIcon,
} from "@radix-ui/react-icons";
import * as Switch from "@radix-ui/react-switch";
import { useCallback, useMemo, useState, useRef, useEffect } from "react";
import fuzzy from "fuzzy";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { FixedSizeList as List } from "react-window";
import { Button } from "@ui/Button";
import { cn } from "@ui/cn";
import { Popover } from "@ui/Popover";

const ITEM_SIZE = 24;
const MAX_VISIBLE_FIELDS = 25;

export function FieldSelector({
  allFields,
  hiddenColumns,
  setHiddenColumns,
  columnOrder,
  setColumnOrder,
}: {
  allFields: string[];
  hiddenColumns: string[];
  setHiddenColumns: (hiddenColumns: string[]) => void;
  columnOrder: string[];
  setColumnOrder: (columnOrder: string[]) => void;
}) {
  const [query, setQuery] = useState("");
  const [focusedIndex, setFocusedIndex] = useState<number>(-1);
  const listRef = useRef<List>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const popoverButtonRef = useRef<HTMLButtonElement>(null);
  const lastButtonRef = useRef<HTMLButtonElement>(null);

  // Filter out special columns like checkbox column and maintain custom order
  const selectableFields = useMemo(() => {
    const newFields = allFields.filter((field) => field !== "*select");

    // If we have a custom order, use it to order the fields
    if (columnOrder.length > 0) {
      const orderedFields = [...columnOrder];
      // Add any new fields that aren't in the custom order yet
      const newFieldsNotInOrder = newFields.filter(
        (field) => !columnOrder.includes(field),
      );
      return [...orderedFields, ...newFieldsNotInOrder].filter((field) =>
        newFields.includes(field),
      );
    }

    return newFields;
  }, [allFields, columnOrder]);

  // Filter fields based on search query using fuzzy search
  const filteredFields = useMemo(() => {
    if (!query) return selectableFields;
    const results = fuzzy.filter(query, selectableFields);
    return results.map((result) => result.string);
  }, [selectableFields, query]);

  // Reset focused index when filtered fields change
  useEffect(() => {
    setFocusedIndex(-1);
  }, [filteredFields]);

  const hasHiddenFields = hiddenColumns.length > 0;

  const toggleField = useCallback(
    (field: string) => {
      if (hiddenColumns.includes(field)) {
        setHiddenColumns(hiddenColumns.filter((f) => f !== field));
      } else {
        setHiddenColumns([...hiddenColumns, field]);
      }
    },
    [hiddenColumns, setHiddenColumns],
  );

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;

      if (!over || active.id === over.id) {
        return;
      }

      // Get the current order (or use selectableFields as default)
      const currentOrder =
        columnOrder.length > 0 ? columnOrder : selectableFields;

      const oldIndex = currentOrder.indexOf(active.id as string);
      const newIndex = currentOrder.indexOf(over.id as string);

      if (oldIndex !== -1 && newIndex !== -1) {
        const newOrder = arrayMove(currentOrder, oldIndex, newIndex);
        // Ensure *select is never included in the column order
        setColumnOrder(newOrder.filter((field) => field !== "*select"));
      }
    },
    [selectableFields, columnOrder, setColumnOrder],
  );

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8, // Require 8px movement before drag starts
      },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const handleHideAll = useCallback(() => {
    setHiddenColumns(selectableFields);
  }, [selectableFields, setHiddenColumns]);

  const handleShowAll = useCallback(() => {
    if (selectableFields.length > MAX_VISIBLE_FIELDS) {
      // Show only the first MAX_VISIBLE_FIELDS fields
      const fieldsToShow = selectableFields.slice(0, MAX_VISIBLE_FIELDS);
      const fieldsToHide = selectableFields.filter(
        (field) => !fieldsToShow.includes(field),
      );
      setHiddenColumns(fieldsToHide);
    } else {
      setHiddenColumns([]);
    }
  }, [selectableFields, setHiddenColumns]);

  const hasTooManyFields = selectableFields.length > MAX_VISIBLE_FIELDS;

  // Keyboard navigation handler
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (filteredFields.length === 0) return;

      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setFocusedIndex((prev) => {
            const next = prev < filteredFields.length - 1 ? prev + 1 : prev;
            // Scroll to the focused item
            if (listRef.current && next !== prev) {
              listRef.current.scrollToItem(next, "smart");
            }
            return next;
          });
          break;
        case "ArrowUp":
          e.preventDefault();
          setFocusedIndex((prev) => {
            const next = prev > 0 ? prev - 1 : prev;
            // Scroll to the focused item
            if (listRef.current && next !== prev) {
              listRef.current.scrollToItem(next, "smart");
            }
            return next;
          });
          break;
        case "Enter":
        case " ":
          e.preventDefault();
          if (focusedIndex >= 0 && focusedIndex < filteredFields.length) {
            toggleField(filteredFields[focusedIndex]);
          }
          break;
        default:
          break;
      }
    },
    [filteredFields, focusedIndex, toggleField],
  );

  return (
    <Popover
      button={({ open }) => (
        <Button
          ref={popoverButtonRef}
          size="sm"
          variant="neutral"
          aria-label="Show or hide fields"
          icon={<EyeNoneIcon />}
          focused={open}
          tip="Toggle visible fields"
          className={cn(
            "h-[27.5px] w-fit min-w-[27.5px] justify-center rounded-lg p-1 text-xs",
            hasHiddenFields &&
              "bg-yellow-100/50 hover:bg-yellow-100 dark:bg-yellow-600/20 dark:hover:bg-yellow-600/50",
          )}
        >
          {hasHiddenFields
            ? `${hiddenColumns.length} hidden field${hiddenColumns.length > 1 ? "s" : ""}`
            : ""}
        </Button>
      )}
      className="max-h-96 w-80"
      placement="bottom-start"
      offset={[0, 4]}
      portal
      padding={false}
    >
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragEnd={handleDragEnd}
      >
        <div className="flex max-h-96 flex-col">
          {/* Search bar */}
          <div className="flex items-center gap-2 rounded-t-md border-b bg-background-secondary px-3 py-2">
            <MagnifyingGlassIcon className="size-4 text-content-secondary" />
            <input
              autoFocus
              ref={searchInputRef}
              type="text"
              placeholder="Search fields..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="flex-1 bg-transparent text-xs text-content-primary placeholder:text-content-tertiary focus:outline-none"
            />
          </div>

          {/* Fields list */}
          <div className="overflow-hidden">
            {filteredFields.length === 0 ? (
              <div className="px-3 py-2 text-xs text-content-tertiary">
                No fields found
              </div>
            ) : (
              <SortableContext
                items={filteredFields}
                strategy={verticalListSortingStrategy}
              >
                <div role="listbox" aria-label="Field visibility controls">
                  <List
                    ref={listRef}
                    height={Math.min(
                      filteredFields.length * ITEM_SIZE + 8,
                      384 - 48 - 36,
                    )}
                    itemCount={filteredFields.length}
                    itemSize={ITEM_SIZE}
                    width="100%"
                    itemData={{
                      fields: filteredFields,
                      hiddenColumns,
                      toggleField,
                      focusedIndex,
                      setFocusedIndex,
                      handleKeyDown,
                    }}
                    className="scrollbar p-1"
                  >
                    {VirtualRow}
                  </List>
                </div>
              </SortableContext>
            )}
          </div>
          <div className="flex w-full items-center gap-2 border-t p-1">
            <Button
              variant="neutral"
              className="w-full justify-center"
              size="xs"
              icon={<EyeNoneIcon />}
              onClick={handleHideAll}
            >
              Hide All
            </Button>
            <Button
              ref={lastButtonRef}
              variant="neutral"
              size="xs"
              className="w-full justify-center"
              tip={
                hasTooManyFields
                  ? `You may experience slower page when rendering more than ${MAX_VISIBLE_FIELDS} fields at once.`
                  : undefined
              }
              icon={<EyeOpenIcon />}
              onClick={handleShowAll}
              onKeyDown={(e) => {
                if (e.key === "Tab" && !e.shiftKey) {
                  e.preventDefault();
                  popoverButtonRef.current?.focus();
                }
              }}
            >
              {hasTooManyFields ? `Show ${MAX_VISIBLE_FIELDS}` : "Show All"}
            </Button>
          </div>
        </div>
      </DndContext>
    </Popover>
  );
}

// Virtual list row component
function VirtualRow({
  index,
  style,
  data,
}: {
  index: number;
  style: React.CSSProperties;
  data: {
    fields: string[];
    hiddenColumns: string[];
    toggleField: (field: string) => void;
    focusedIndex: number;
    setFocusedIndex: (index: number) => void;
    handleKeyDown: (e: React.KeyboardEvent) => void;
  };
}) {
  const field = data.fields[index];
  return (
    <div style={style}>
      <FieldItem
        field={field}
        isVisible={!data.hiddenColumns.includes(field)}
        onToggle={() => data.toggleField(field)}
        index={index}
        setFocusedIndex={data.setFocusedIndex}
        handleKeyDown={data.handleKeyDown}
      />
    </div>
  );
}

function FieldItem({
  field,
  isVisible,
  onToggle,
  index,
  setFocusedIndex,
  handleKeyDown,
}: {
  field: string;
  isVisible: boolean;
  onToggle: () => void;
  index: number;
  setFocusedIndex: (index: number) => void;
  handleKeyDown: (e: React.KeyboardEvent) => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: field });

  // Clamp the transform to prevent overflow
  const clampedTransform = transform
    ? { ...transform, y: Math.max(-350, Math.min(350, transform.y)) }
    : null;

  const style = {
    transform: clampedTransform
      ? `translate3d(0, ${clampedTransform.y}px, 0)`
      : undefined,
    transition,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      {...attributes}
      {...listeners}
      role="option"
      aria-selected={isVisible}
      tabIndex={0}
      onClick={onToggle}
      onFocus={() => setFocusedIndex(index)}
      onKeyDown={handleKeyDown}
      className={cn(
        "mx-1 flex cursor-pointer items-center gap-2 rounded-sm px-2 py-1.5 text-xs transition-colors",
        "hover:bg-background-tertiary",
        "focus:outline-none focus-visible:bg-background-tertiary focus-visible:ring-2 focus-visible:ring-border-selected",
        isDragging && "cursor-grabbing opacity-50",
      )}
    >
      {/* Toggle switch */}
      <Switch.Root
        checked={isVisible}
        onCheckedChange={onToggle}
        onClick={(e) => e.stopPropagation()}
        onPointerDown={(e) => e.stopPropagation()}
        tabIndex={-1}
        className={cn(
          "relative h-3 w-5 rounded-full transition-colors",
          "focus:outline-none",
          isVisible ? "bg-util-accent" : "bg-neutral-4 dark:bg-neutral-7",
        )}
      >
        <Switch.Thumb
          className={cn(
            "my-0.5 block size-2 rounded-full bg-white shadow-sm transition-transform",
            isVisible ? "translate-x-[10px]" : "translate-x-[2px]",
          )}
        />
      </Switch.Root>

      {/* Field name */}
      <span className="flex-1 truncate text-content-primary">{field}</span>

      {/* Drag handle icon */}
      <DragHandleDots2Icon className="size-4 cursor-grab text-content-secondary active:cursor-grabbing" />
    </div>
  );
}
