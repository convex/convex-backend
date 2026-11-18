import { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { PaginationControls } from "./PaginationControls";

const meta = {
  component: PaginationControls,
} satisfies Meta<typeof PaginationControls>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CursorBasedDefault: Story = {
  args: {
    isCursorBasedPagination: true,
    currentPage: 2,
    hasMore: true,
    pageSize: 25,
    onPageSizeChange: fn(),
    onPreviousPage: fn(),
    onNextPage: fn(),
    canGoPrevious: true,
    showPageSize: true,
  },
};

export const CursorBasedCustomOptions: Story = {
  args: {
    isCursorBasedPagination: true,
    currentPage: 1,
    hasMore: true,
    pageSize: 12,
    onPageSizeChange: fn(),
    onPreviousPage: fn(),
    onNextPage: fn(),
    canGoPrevious: false,
    showPageSize: true,
    pageSizeOptions: [
      { label: "6", value: 6 },
      { label: "12", value: 12 },
      { label: "24", value: 24 },
      { label: "48", value: 48 },
      { label: "96", value: 96 },
    ],
  },
};

export const CursorBasedCustomCurrentSize: Story = {
  args: {
    isCursorBasedPagination: true,
    currentPage: 3,
    hasMore: false,
    pageSize: 25,
    onPageSizeChange: fn(),
    onPreviousPage: fn(),
    onNextPage: fn(),
    canGoPrevious: true,
    showPageSize: true,
    pageSizeOptions: [
      { label: "6", value: 6 },
      { label: "12", value: 12 },
      { label: "24", value: 24 },
      { label: "48", value: 48 },
      { label: "96", value: 96 },
    ],
  },
};

export const CursorBasedWithoutPageSize: Story = {
  args: {
    isCursorBasedPagination: true,
    currentPage: 5,
    hasMore: true,
    pageSize: 10,
    onPageSizeChange: fn(),
    onPreviousPage: fn(),
    onNextPage: fn(),
    canGoPrevious: true,
    showPageSize: false,
  },
};

export const CursorBasedFirstPage: Story = {
  args: {
    isCursorBasedPagination: true,
    currentPage: 1,
    hasMore: true,
    pageSize: 50,
    onPageSizeChange: fn(),
    onPreviousPage: fn(),
    onNextPage: fn(),
    canGoPrevious: false,
    showPageSize: true,
  },
};

export const CursorBasedLastPage: Story = {
  args: {
    isCursorBasedPagination: true,
    currentPage: 10,
    hasMore: false,
    pageSize: 100,
    onPageSizeChange: fn(),
    onPreviousPage: fn(),
    onNextPage: fn(),
    canGoPrevious: true,
    showPageSize: true,
  },
};

export const OffsetBased: Story = {
  args: {
    currentPage: 3,
    totalPages: 10,
    onPageChange: fn(),
  },
};

export const OffsetBasedFirstPage: Story = {
  args: {
    currentPage: 1,
    totalPages: 5,
    onPageChange: fn(),
  },
};

export const OffsetBasedLastPage: Story = {
  args: {
    currentPage: 8,
    totalPages: 8,
    onPageChange: fn(),
  },
};
