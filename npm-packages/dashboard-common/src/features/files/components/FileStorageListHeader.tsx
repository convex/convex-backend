import { PlayCircleIcon, PauseCircleIcon } from "@heroicons/react/24/outline";
import {
  ExclamationTriangleIcon,
  ReloadIcon,
  CaretUpIcon,
  QuestionMarkCircledIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Checkbox } from "@ui/Checkbox";
import classNames from "classnames";
import { FileFilters } from "./FileStorageHeader";

export const FILE_STORAGE_LIST_GRID_CLASSES =
  "grid grid-cols-[2.5rem_minmax(5rem,1fr)_minmax(1.875rem,5.625rem)_minmax(3.75rem,12.5rem)_minmax(3.75rem,11.25rem)_5.625rem] ";

export function FileStorageListHeader({
  isPaused,
  isLoadingPausedData,
  togglePaused,
  isRateLimited,
  reload,
  allSelected,
  someSelected,
  toggleSelectAll,
  filters,
  setFilters,
}: {
  isPaused: boolean;
  isLoadingPausedData: boolean;
  togglePaused: () => void;
  isRateLimited: boolean;
  reload: () => void;
  allSelected: boolean;
  someSelected: boolean;
  toggleSelectAll: () => void;
  filters: FileFilters;
  setFilters: (filters: FileFilters) => void;
}) {
  const toggleSortOrder = () => {
    setFilters({
      ...filters,
      order: filters.order === "asc" ? "desc" : "asc",
    });
  };

  return (
    <div className="relative min-w-145 border-b p-2 py-3 text-xs text-content-secondary">
      <div
        // The scrolling-related styles are needed to account for the scrollbar that may be present in the list itself.
        className={`grid w-full items-center gap-2 ${FILE_STORAGE_LIST_GRID_CLASSES} scrollbar overflow-auto`}
        style={{
          scrollbarGutter: "stable",
        }}
      >
        <div className="flex items-center justify-center pr-2">
          <Checkbox
            checked={
              allSelected ? true : someSelected ? "indeterminate" : false
            }
            onChange={toggleSelectAll}
          />
        </div>
        <div className="flex items-center gap-1">
          ID{" "}
          <Tooltip
            tip="The ID of this file in Convex storage. Can be used to reference this file in your code."
            side="right"
          >
            <QuestionMarkCircledIcon />
          </Tooltip>
        </div>
        <div>Size</div>
        <div>Content type</div>
        <div className="flex items-center gap-1">
          Uploaded at{" "}
          <Button
            variant="neutral"
            size="xs"
            className="-ml-1.5 h-auto border-none bg-transparent p-0 text-content-secondary"
            onClick={toggleSortOrder}
            aria-label={`Sort by upload time ${filters.order === "asc" ? "descending" : "ascending"}`}
            tip={`Click to sort ${filters.order === "asc" ? "newest first" : "oldest first"}`}
            icon={
              <CaretUpIcon
                className={classNames(
                  "transition-all m-1.5 border rounded-sm",
                  filters.order === "desc" ? "rotate-180" : "",
                )}
              />
            }
          />
        </div>
        <div className="absolute right-2 ml-auto flex items-center justify-between">
          <div className="flex items-center gap-2">
            {isRateLimited && (
              <Tooltip tip="Live updates have automatically been paused because the files are updating too frequently in this deployment.">
                <ExclamationTriangleIcon className="mt-0.5 text-content-warning" />
              </Tooltip>
            )}
            {isPaused && (
              <Button
                icon={<ReloadIcon />}
                loading={isLoadingPausedData}
                variant="neutral"
                className="animate-fadeInFromLoading text-xs"
                size="xs"
                onClick={() => {
                  reload();
                }}
                tip="Refresh the list of files."
              >
                <span className="sr-only">Refresh</span>
              </Button>
            )}
            <Button
              size="xs"
              className="text-xs"
              icon={
                isPaused ? (
                  <PlayCircleIcon className="size-4" />
                ) : (
                  <PauseCircleIcon className="size-4" />
                )
              }
              onClick={togglePaused}
              tip={
                isRateLimited
                  ? "Files are being updated too frequently to show live updates."
                  : isPaused
                    ? "Resume to show live updates."
                    : "Pause to prevent live updates."
              }
              disabled={isRateLimited}
            >
              <span className="sr-only">{isPaused ? "Go Live" : "Pause"}</span>
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
