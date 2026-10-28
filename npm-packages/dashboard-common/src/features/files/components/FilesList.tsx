import React, { useEffect, useMemo, useRef, useCallback } from "react";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { FileMetadata } from "system-udfs/convex/_system/frontend/fileStorageV2";
import {
  useReactTable,
  getCoreRowModel,
  createColumnHelper,
} from "@tanstack/react-table";
import { Loading } from "@ui/Loading";
import { EmptySection } from "@common/elements/EmptySection";
import { FileIcon } from "@radix-ui/react-icons";
import { Sheet } from "@ui/Sheet";
import { InfiniteScrollList } from "@common/elements/InfiniteScrollList";
import { FILE_METADATA_PAGE_SIZE } from "../lib/usePaginatedFileMetadata";
import { FileStorageListHeader } from "./FileStorageListHeader";
import { FileFilters } from "./FileStorageHeader";
import {
  FileStorageListItemInner,
  FILE_ITEM_SIZE,
} from "./FileStorageListItem";

const columnHelper = createColumnHelper<FileMetadata>();

// Custom hook to get the file columns
function useFileColumns() {
  return useMemo(
    () => [
      columnHelper.display({
        id: "select",
        size: 40,
        header: "Select",
      }),
      columnHelper.accessor("_id", {
        header: "ID",
        size: 220,
      }),
      columnHelper.accessor("size", {
        header: "Size",
        size: 90,
      }),
      columnHelper.accessor("contentType", {
        header: "Content type",
        size: 200,
      }),
      columnHelper.accessor("_creationTime", {
        header: "Uploaded at",
        size: 180,
      }),
      columnHelper.display({
        id: "actions",
        size: 90,
        header: "Actions",
      }),
    ],
    [],
  );
}

export function FilesList({
  selectedFiles,
  setSelectedFiles,
  containerRef,
  totalNumFiles,
  files,
  status,
  loadMore,
  isPaused,
  isLoadingPausedData,
  isRateLimited,
  togglePaused,
  reload,
  hasFilters,
  filters,
  setFilters,
}: {
  selectedFiles: Record<Id<"_storage">, boolean>;
  setSelectedFiles: React.Dispatch<
    React.SetStateAction<Record<Id<"_storage">, boolean>>
  >;
  containerRef: React.RefObject<HTMLDivElement | null>;
  totalNumFiles: number | undefined;
  files: FileMetadata[];
  status: "LoadingFirstPage" | "LoadingMore" | "CanLoadMore" | "Exhausted";
  loadMore: (numItems: number) => void;
  isPaused: boolean;
  isLoadingPausedData: boolean;
  isRateLimited: boolean;
  togglePaused: () => void;
  reload: () => void;
  hasFilters: boolean;
  filters: FileFilters;
  setFilters: (filters: FileFilters) => void;
}) {
  // Remove the selection of files that no longer exist
  const prevResults = useRef<FileMetadata[] | undefined>(undefined);
  useEffect(() => {
    if (prevResults.current === files) return;
    prevResults.current = files;

    const existingFileIds = new Set(files.map((r) => r._id));

    const updatedSelectedFiles = Object.fromEntries(
      Object.entries(selectedFiles).filter(([id]) =>
        existingFileIds.has(id as Id<"_storage">),
      ),
    );

    if (
      Object.keys(updatedSelectedFiles).length !==
      Object.keys(selectedFiles).length
    ) {
      setSelectedFiles(updatedSelectedFiles);
    }
  }, [files, selectedFiles, setSelectedFiles]);

  // Calculate selection state for header
  const selectedCount = Object.values(selectedFiles).filter(Boolean).length;
  const allSelected = selectedCount === totalNumFiles && totalNumFiles! > 0;
  const someSelected = selectedCount > 0 && !allSelected;

  // Toggle all selected function
  const toggleSelectAll = useCallback(() => {
    if (allSelected || someSelected) {
      // Deselect all
      setSelectedFiles({});
    } else {
      // Select all visible files
      const newSelectedFiles = {} as Record<Id<"_storage">, boolean>;
      files.forEach((file) => {
        newSelectedFiles[file._id] = true;
      });
      setSelectedFiles(newSelectedFiles);
    }
  }, [allSelected, someSelected, files, setSelectedFiles]);

  // Setup TanStack table
  const columns = useFileColumns();

  const tableInstance = useReactTable({
    columns,
    data: files,
    getCoreRowModel: getCoreRowModel(),
    enableRowSelection: true,
    enableMultiRowSelection: true,
    state: {
      rowSelection: selectedFiles,
    },
    getRowId: (row) => row._id,
    onRowSelectionChange: setSelectedFiles,
  });

  // Memoize item data to prevent unnecessary re-renders
  const itemData = useMemo(
    () => ({
      files,
      table: tableInstance,
      selectionMap: selectedFiles,
    }),
    [files, tableInstance, selectedFiles],
  );

  return (
    <div
      className="flex max-w-240 min-w-150 grow flex-col gap-4"
      ref={containerRef}
    >
      <Sheet
        padding={false}
        className="flex grow animate-fadeInFromLoading flex-col overflow-hidden"
      >
        <div className="flex max-h-full grow flex-col">
          <FileStorageListHeader
            isPaused={isPaused}
            isLoadingPausedData={isLoadingPausedData}
            togglePaused={togglePaused}
            isRateLimited={isRateLimited}
            reload={reload}
            allSelected={allSelected}
            someSelected={someSelected}
            toggleSelectAll={toggleSelectAll}
            filters={filters}
            setFilters={setFilters}
          />
          {status === "LoadingFirstPage" ? (
            <Loading className="max-w-240" />
          ) : files.length === 0 && status !== "CanLoadMore" ? (
            hasFilters ? (
              <div className="mt-2 flex w-full items-center justify-center text-content-secondary">
                No files match your filters.
              </div>
            ) : (
              <EmptySection
                sheet={false}
                Icon={FileIcon}
                color="red"
                header="No files yet."
                body="With Convex File Storage, you can store and serve files."
                learnMoreButton={{
                  href: "https://docs.convex.dev/file-storage",
                  children: "Learn more about file storage.",
                }}
              />
            )
          ) : (
            <div className="h-full">
              <InfiniteScrollList
                className="scrollbar min-w-145"
                style={{
                  scrollbarGutter: "stable",
                }}
                overscanCount={25}
                outerRef={containerRef}
                items={files}
                totalNumItems={totalNumFiles}
                itemKey={(idx, data) =>
                  data.files[idx]?._id || `loading-${idx}`
                }
                itemSize={FILE_ITEM_SIZE}
                itemData={itemData}
                pageSize={FILE_METADATA_PAGE_SIZE}
                RowOrLoading={FileStorageListItemInner}
                loadMore={loadMore}
              />
            </div>
          )}
        </div>
      </Sheet>
    </div>
  );
}
