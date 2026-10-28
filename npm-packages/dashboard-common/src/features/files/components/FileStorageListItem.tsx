import React from "react";
import { useReactTable } from "@tanstack/react-table";
import { FileMetadata } from "system-udfs/convex/_system/frontend/fileStorageV2";
import { Checkbox } from "@ui/Checkbox";
import { Button } from "@ui/Button";
import { CopyIcon } from "@radix-ui/react-icons";
import { toast } from "@common/lib/utils";
import { formatBytes } from "@common/lib/format";
import { FileActions } from "./FileActions";
import { FILE_STORAGE_LIST_GRID_CLASSES } from "./FileStorageListHeader";

// Define the item props type
export type FileStorageListItemProps = {
  data: {
    files: FileMetadata[];
    table: ReturnType<typeof useReactTable<FileMetadata>>;
    selectionMap: Record<string, boolean>;
  };
  index: number;
  style: React.CSSProperties;
};

// Memoized component to prevent unnecessary re-renders
export const FileStorageListItem = React.memo(
  FileStorageListItemInner,
  (prevProps, nextProps) =>
    prevProps.index === nextProps.index &&
    prevProps.style.top === nextProps.style.top &&
    prevProps.data.files[prevProps.index] ===
      nextProps.data.files[nextProps.index] &&
    prevProps.data.selectionMap[prevProps.data.files[prevProps.index]?._id] ===
      nextProps.data.selectionMap[nextProps.data.files[nextProps.index]?._id],
);

export const FILE_ITEM_SIZE = 40; // Reduce row height from 54px to 40px

export function FileStorageListItemInner({
  data,
  index,
  style,
}: FileStorageListItemProps) {
  const file = data.files[index];
  if (!file) {
    return (
      <div
        style={{ ...style, height: FILE_ITEM_SIZE }}
        className={`${FILE_STORAGE_LIST_GRID_CLASSES} items-center gap-2 px-2 py-1`}
      >
        <div className="flex items-center justify-center pr-2">
          <div className="size-4 rounded-sm bg-background-tertiary" />
        </div>
        <div>
          <div className="h-4 w-3/4 rounded-sm bg-background-tertiary" />
        </div>
        <div>
          <div className="h-4 w-3/4 rounded-sm bg-background-tertiary" />
        </div>
        <div>
          <div className="h-4 w-3/4 rounded-sm bg-background-tertiary" />
        </div>
        <div>
          <div className="h-4 w-3/4 rounded-sm bg-background-tertiary" />
        </div>
        <div className="ml-auto">
          <div className="h-4 w-1/2 rounded-sm bg-background-tertiary" />
        </div>
      </div>
    );
  }

  // Get the row data directly using file._id rather than relying on the table's row model
  // which can cause unnecessary re-renders
  const isSelected = !!data.selectionMap[file._id];

  // Create a toggler function to update selection
  const toggleSelected = () => {
    // Get the row and toggle it using the table's row selection API
    const row = data.table.getRowModel().rowsById[file._id];
    if (row) {
      row.toggleSelected(!isSelected);
    }
  };

  return (
    <div
      style={style}
      className={`min-w-145 ${FILE_STORAGE_LIST_GRID_CLASSES} items-center gap-2 border-b bg-background-secondary px-2 py-1 text-xs last:border-b-0`}
      data-testid="filerow"
    >
      {/* Checkbox column */}
      <div className="flex items-center justify-center pr-2">
        <Checkbox checked={isSelected} onChange={() => toggleSelected()} />
      </div>

      {/* Storage ID column */}
      <div className="flex min-w-0 items-center gap-1">
        <div className="truncate font-mono text-content-primary">
          {file._id}
        </div>
        <Button
          tip="Copy Storage ID to clipboard"
          tipSide="bottom"
          aria-label="Copy Storage ID"
          onClick={() => {
            void navigator.clipboard.writeText(file._id);
            toast("success", "Storage ID copied to clipboard");
          }}
          className="shrink-0 text-content-secondary"
          size="xs"
          variant="neutral"
          inline
          icon={<CopyIcon />}
        />
      </div>

      {/* Size column */}
      <div className="min-w-0 overflow-hidden font-mono text-ellipsis">
        {formatBytes(Number(file.size))}
      </div>

      {/* Content Type column */}
      <div className="min-w-0 truncate font-mono">
        {file.contentType || "Unknown"}
      </div>

      {/* Upload Time column */}
      <div className="truncate">
        {new Date(file._creationTime).toLocaleString()}
      </div>

      {/* Actions column */}
      <div className="ml-auto flex items-center">
        <FileActions file={file} />
      </div>
    </div>
  );
}
