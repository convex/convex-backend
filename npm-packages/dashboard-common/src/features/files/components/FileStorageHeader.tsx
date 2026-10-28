import { Id } from "system-udfs/convex/_generated/dataModel";
import { useState, useEffect } from "react";
import {
  DateRangePicker,
  DateRangeShortcut,
} from "@common/elements/DateRangePicker";
import { startOfDay, endOfDay, subDays } from "date-fns";
import { TextInput } from "@ui/TextInput";
import { NentSwitcher } from "@common/elements/NentSwitcher";
import { Uploader, useUploadFiles } from "./Uploader";
import { DeleteFilesButton } from "./DeleteFilesButton";

export interface FileFilters {
  minCreationTime?: number;
  maxCreationTime?: number;
  order?: "asc" | "desc";
}

export function FileStorageHeader({
  selectedFiles,
  useUploadFilesResult,
  totalNumFiles,
  filters = {},
  setFilters,
  fileId,
  setFileId,
}: {
  selectedFiles: Id<"_storage">[];
  useUploadFilesResult: ReturnType<typeof useUploadFiles>;
  totalNumFiles: number | undefined;
  filters: FileFilters;
  setFilters: (filters: FileFilters) => void;
  fileId: string;
  setFileId: (fileId: string) => void;
}) {
  // Track if date filtering is enabled
  const [dateFilterEnabled, setDateFilterEnabled] = useState<boolean>(
    !!(filters?.minCreationTime || filters?.maxCreationTime) && !fileId,
  );

  // Set default date range (last 30 days or null if filters disabled)
  const [dateRange, setDateRange] = useState<{ from?: Date; to?: Date }>(() => {
    if (dateFilterEnabled) {
      return {
        from: filters?.minCreationTime
          ? new Date(filters.minCreationTime)
          : subDays(new Date(), 30),
        to: filters?.maxCreationTime
          ? new Date(filters.maxCreationTime)
          : new Date(),
      };
    }
    return {};
  });

  // Generate date range shortcuts
  const dateRangeShortcuts = [
    {
      value: "anytime",
      label: "Any time",
      from: new Date(),
      to: new Date(),
      disableFilters: true,
    },
    {
      value: "last24hours",
      label: "Last 24 hours",
      from: subDays(new Date(), 1),
      to: new Date(),
    },
    {
      value: "last7days",
      label: "Last 7 days",
      from: subDays(new Date(), 7),
      to: new Date(),
    },
    {
      value: "last30days",
      label: "Last 30 days",
      from: subDays(new Date(), 30),
      to: new Date(),
    },
    {
      value: "last90days",
      label: "Last 90 days",
      from: subDays(new Date(), 90),
      to: new Date(),
    },
  ];

  // Handle date range change
  const handleDateRangeChange = (
    range: { from?: Date; to?: Date },
    shortcut?: DateRangeShortcut,
  ) => {
    if (shortcut?.disableFilters) {
      // Handle "Any time" shortcut
      setDateFilterEnabled(false);
      setDateRange({}); // Clear date range
      const {
        minCreationTime: _min,
        maxCreationTime: _max,
        ...restFilters
      } = filters;
      setFilters(restFilters);
      return;
    }

    // Always enable filtering when a date is selected manually or through a shortcut
    if (range.from || range.to) {
      setDateFilterEnabled(true);

      const newRange = {
        from: range.from || new Date(),
        to: range.to || range.from || new Date(),
      };
      setDateRange(newRange);

      // Update filters with timestamps
      setFilters({
        ...filters,
        minCreationTime: startOfDay(newRange.from).getTime(),
        maxCreationTime: endOfDay(newRange.to).getTime(),
      });
    }
  };

  // Clear date filters when file ID is entered
  useEffect(() => {
    if (fileId) {
      setDateFilterEnabled(false);
      setDateRange({}); // Clear date range

      // Remove date filters when searching by ID
      if (filters.minCreationTime || filters.maxCreationTime) {
        const {
          minCreationTime: _min,
          maxCreationTime: _max,
          ...restFilters
        } = filters;
        setFilters(restFilters);
      }
    }
  }, [fileId, filters, setFilters]);

  // Update internal state when filters change externally
  useEffect(() => {
    const hasDateFilters = !!(
      filters?.minCreationTime || filters?.maxCreationTime
    );

    // Only enable date filter if there are date filters AND no file ID
    setDateFilterEnabled(hasDateFilters && !fileId);

    // Update date range if filters change
    if (hasDateFilters) {
      setDateRange({
        from: filters.minCreationTime
          ? new Date(filters.minCreationTime)
          : subDays(new Date(), 30),
        to: filters.maxCreationTime
          ? new Date(filters.maxCreationTime)
          : new Date(),
      });
    } else if (!dateFilterEnabled) {
      // Clear date range when filters are disabled
      setDateRange({});
    }
  }, [
    filters?.minCreationTime,
    filters?.maxCreationTime,
    fileId,
    dateFilterEnabled,
  ]);

  return (
    <div className="flex max-w-240 min-w-fit flex-col gap-3">
      <div className="flex w-full flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-4">
          <div className="flex flex-1 flex-col gap-1">
            <h3>File Storage</h3>
            <div
              className="flex items-center gap-1 text-xs text-content-secondary"
              data-testid="fileCount"
            >
              <span className="text-xs">Total Files</span>
              {totalNumFiles !== undefined && (
                <span className="font-semibold tabular-nums">
                  {totalNumFiles.toLocaleString()}
                </span>
              )}
            </div>
          </div>
          <div className="w-fit min-w-60">
            <NentSwitcher className="mb-4" />
          </div>
        </div>
      </div>
      <div className="flex flex-wrap items-end justify-between gap-2 gap-y-3">
        <div className="flex items-end gap-2">
          <div className="w-[20rem] max-w-[20rem]">
            <TextInput
              label="Storage ID"
              id="Lookup by ID"
              placeholder="Lookup by ID"
              labelHidden={false}
              type="search"
              onChange={(e) => {
                setFileId(e.target.value);
              }}
              value={fileId}
            />
          </div>
          <DateRangePicker
            date={dateRange}
            setDate={handleDateRangeChange}
            shortcuts={dateRangeShortcuts}
            disabled={!!fileId}
            dateFilterEnabled={dateFilterEnabled}
            prefix="Uploaded at:"
          />
        </div>
        <div className="flex items-start gap-2">
          <DeleteFilesButton selectedFiles={selectedFiles} />
          <Uploader useUploadFilesResult={useUploadFilesResult} />
        </div>
      </div>
    </div>
  );
}
