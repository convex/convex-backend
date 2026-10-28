import { useCallback, useMemo, useState } from "react";
import {
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  DragEndEvent,
  DragStartEvent,
} from "@dnd-kit/core";
import { sortableKeyboardCoordinates } from "@dnd-kit/sortable";
import { HeaderGroup } from "@tanstack/react-table";
import { GenericDocument } from "convex/server";

export function useColumnDragAndDrop({
  headerGroups,
  reorderColumns,
  columnOrder,
}: {
  headerGroups: HeaderGroup<GenericDocument>[];
  reorderColumns: (item: { index: number }, newIndex: number) => void;
  columnOrder: string[];
}) {
  const [activeColumnId, setActiveColumnId] = useState<string | null>(null);
  const [dragOffset, setDragOffset] = useState<number>(0);

  const sensors = useSensors(
    useSensor(PointerSensor),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const handleDragStart = useCallback((event: DragStartEvent) => {
    setActiveColumnId(event.active.id as string);
    setDragOffset(0);
  }, []);

  const handleDragMove = useCallback((event: any) => {
    if (event.delta) {
      setDragOffset(event.delta.x);
    }
  }, []);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      setActiveColumnId(null);
      setDragOffset(0);

      if (!over || active.id === over.id) {
        return;
      }

      const oldIndex = columnOrder.indexOf(active.id as string);
      const overIndex = columnOrder.indexOf(over.id as string);

      // Since we always drop to the right of the hovered column,
      // we need to adjust the target index when dragging from right to left
      let targetIndex = overIndex;
      if (oldIndex > overIndex) {
        // Dragging from right to left: place after the hovered column
        targetIndex = overIndex + 1;
      }

      // Prevent reordering the *select column (always at index 0)
      if (
        oldIndex !== -1 &&
        targetIndex !== -1 &&
        oldIndex !== 0 &&
        targetIndex !== 0
      ) {
        reorderColumns({ index: oldIndex }, targetIndex);
      }
    },
    [reorderColumns, columnOrder],
  );

  const handleDragCancel = useCallback(() => {
    setActiveColumnId(null);
    setDragOffset(0);
  }, []);

  // Get the active column for drag overlay
  const activeColumn = useMemo(
    () => headerGroups[0]?.headers.find((h) => h.id === activeColumnId),
    [headerGroups, activeColumnId],
  );

  // Calculate the position of the active column
  const activeColumnPosition = useMemo(() => {
    if (!activeColumn) return null;

    const columnIndex = headerGroups[0]?.headers.findIndex(
      (h) => h.id === activeColumnId,
    );
    if (columnIndex === -1) return null;

    // Calculate the left position by summing up widths of previous columns
    let left = 0;
    for (let i = 0; i < columnIndex; i++) {
      left += headerGroups[0].headers[i].getSize();
    }

    return left;
  }, [activeColumn, activeColumnId, headerGroups]);

  return {
    sensors,
    dragOffset,
    activeColumn,
    activeColumnPosition,
    handleDragStart,
    handleDragMove,
    handleDragEnd,
    handleDragCancel,
  };
}
