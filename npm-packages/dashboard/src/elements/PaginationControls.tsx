import {
  ChevronLeftIcon,
  ChevronRightIcon,
  DoubleArrowLeftIcon,
  DoubleArrowRightIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Combobox, Option } from "@ui/Combobox";
import { cn } from "@ui/cn";

interface OffsetPaginationControlsProps {
  currentPage: number;
  totalPages: number;
  onPageChange: (page: number) => void;
  className?: string;
  pageSize?: never;
  onPageSizeChange?: never;
}

interface CursorPaginationControlsProps {
  currentPage: number;
  hasMore: boolean;
  pageSize: number;
  onPageSizeChange: (pageSize: number) => void;
  onPreviousPage: () => void;
  onNextPage: () => void;
  canGoPrevious: boolean;
  className?: string;
  showPageSize?: boolean;
  pageSizeOptions?: readonly Option<number>[];
}

interface CursorPaginationControlsPropsWithDiscriminator extends CursorPaginationControlsProps {
  isCursorBasedPagination: true;
}

type PaginationControlsProps =
  | OffsetPaginationControlsProps
  | CursorPaginationControlsPropsWithDiscriminator;

const PAGE_SIZE_OPTIONS: Option<number>[] = [
  { label: "5", value: 5 },
  { label: "10", value: 10 },
  { label: "25", value: 25 },
  { label: "50", value: 50 },
  { label: "100", value: 100 },
];

export function PaginationControls(props: PaginationControlsProps) {
  const { isCursorBasedPagination } = props as {
    isCursorBasedPagination?: boolean;
  };
  if (isCursorBasedPagination) {
    const {
      currentPage,
      hasMore,
      pageSize,
      onPageSizeChange,
      onPreviousPage,
      onNextPage,
      canGoPrevious,
      className,
      showPageSize,
      pageSizeOptions,
    } = props as CursorPaginationControlsPropsWithDiscriminator;
    return (
      <CursorPaginationControls
        currentPage={currentPage}
        hasMore={hasMore}
        pageSize={pageSize}
        onPageSizeChange={onPageSizeChange}
        onPreviousPage={onPreviousPage}
        onNextPage={onNextPage}
        canGoPrevious={canGoPrevious}
        className={className}
        showPageSize={showPageSize}
        pageSizeOptions={pageSizeOptions}
      />
    );
  }
  const offsetProps = props as OffsetPaginationControlsProps;
  const { currentPage, totalPages, onPageChange, className } = offsetProps;
  return (
    <OffsetPaginationControls
      currentPage={currentPage}
      totalPages={totalPages}
      onPageChange={onPageChange}
      className={className}
    />
  );
}

function OffsetPaginationControls({
  currentPage,
  totalPages,
  onPageChange,
  className = "",
}: OffsetPaginationControlsProps) {
  if (totalPages <= 1) {
    return null;
  }

  return (
    <div className={cn("flex items-center justify-center gap-2", className)}>
      {/* First page button */}
      <Button
        variant="neutral"
        inline
        size="sm"
        icon={<DoubleArrowLeftIcon />}
        onClick={() => onPageChange(1)}
        disabled={currentPage === 1}
        aria-label="Go to first page"
      />

      {/* Previous page button */}
      <Button
        variant="neutral"
        inline
        size="sm"
        icon={<ChevronLeftIcon />}
        onClick={() => onPageChange(Math.max(1, currentPage - 1))}
        disabled={currentPage === 1}
        aria-label="Go to previous page"
      />

      {/* Page indicator */}
      <span className="text-sm text-content-secondary tabular-nums">
        Page {currentPage} of {totalPages}
      </span>

      {/* Next page button */}
      <Button
        variant="neutral"
        inline
        size="sm"
        icon={<ChevronRightIcon />}
        onClick={() => onPageChange(Math.min(totalPages, currentPage + 1))}
        disabled={currentPage === totalPages}
        aria-label="Go to next page"
      />

      {/* Last page button */}
      <Button
        variant="neutral"
        inline
        size="sm"
        icon={<DoubleArrowRightIcon />}
        onClick={() => onPageChange(totalPages)}
        disabled={currentPage === totalPages}
        aria-label="Go to last page"
      />
    </div>
  );
}

function CursorPaginationControls({
  currentPage,
  hasMore,
  pageSize,
  onPageSizeChange,
  onPreviousPage,
  onNextPage,
  canGoPrevious,
  className = "",
  showPageSize = true,
  pageSizeOptions,
}: CursorPaginationControlsProps) {
  // Use provided options or default to PAGE_SIZE_OPTIONS
  let options = pageSizeOptions || PAGE_SIZE_OPTIONS;

  // If current pageSize isn't in the options list, insert it in sorted order
  if (!options.some((opt) => opt.value === pageSize)) {
    options = [
      ...options,
      { label: pageSize.toString(), value: pageSize },
    ].sort((a, b) => a.value - b.value);
  }

  return (
    <div className={cn("flex items-center justify-center gap-2", className)}>
      {/* Page size selector - only show if showPageSize is true */}
      {showPageSize && (
        <div className="flex items-center gap-2">
          <span className="text-sm text-content-secondary tabular-nums">
            Page size:
          </span>
          <Combobox
            label="Page size"
            labelHidden
            options={options}
            selectedOption={pageSize}
            setSelectedOption={(newValue) => {
              if (newValue) {
                onPageSizeChange(newValue);
              }
            }}
            disableSearch
            buttonClasses="w-fit"
            optionsWidth="fit"
          />
        </div>
      )}

      {/* Previous page button */}
      <Button
        variant="neutral"
        inline
        size="sm"
        icon={<ChevronLeftIcon />}
        onClick={onPreviousPage}
        disabled={!canGoPrevious}
        aria-label="Go to previous page"
      />

      {/* Page indicator */}
      <span className="min-w-fit text-sm text-content-secondary tabular-nums">
        Page {currentPage.toLocaleString()}
      </span>

      {/* Next page button */}
      <Button
        variant="neutral"
        inline
        size="sm"
        icon={<ChevronRightIcon />}
        onClick={onNextPage}
        disabled={!hasMore}
        aria-label="Go to next page"
      />
    </div>
  );
}
