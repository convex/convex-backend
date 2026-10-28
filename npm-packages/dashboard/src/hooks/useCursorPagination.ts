import { useCallback, useState } from "react";

/**
 * Client-side navigation state for a cursor-paginated list.
 *
 * Keeps a stack of the cursors used to fetch each visited page so the user can
 * step backwards. `currentCursor` feeds the data-fetching hook, so this hook
 * must be called before fetching; the page results (`nextCursor`, emptiness)
 * are wired back in through `onNextPage` and {@link useSnapBackOnEmptyPage}.
 */
export function useCursorPagination() {
  const [cursorHistory, setCursorHistory] = useState<(string | undefined)[]>([
    undefined,
  ]);

  const currentCursor = cursorHistory[cursorHistory.length - 1];
  const currentPage = cursorHistory.length;
  const canGoPrevious = cursorHistory.length > 1;

  const onNextPage = useCallback((nextCursor: string | null | undefined) => {
    if (nextCursor) {
      setCursorHistory((prev) => [...prev, nextCursor]);
    }
  }, []);

  const onPreviousPage = useCallback(() => {
    setCursorHistory((prev) => (prev.length > 1 ? prev.slice(0, -1) : prev));
  }, []);

  const resetPagination = useCallback(() => {
    setCursorHistory([undefined]);
  }, []);

  return {
    currentCursor,
    currentPage,
    canGoPrevious,
    onNextPage,
    onPreviousPage,
    resetPagination,
  };
}

/**
 * Navigates back one page when the current page has no items but earlier pages
 * exist (e.g. the user deleted every item on this page). Steps back until a
 * non-empty page or page 1 is reached.
 *
 * Pops during render rather than in an effect, so React re-renders with the
 * previous cursor before painting — the empty state never flashes — and the
 * check re-runs on every render instead of waiting for a dependency to change.
 */
export function useSnapBackOnEmptyPage(
  pagination: Pick<
    ReturnType<typeof useCursorPagination>,
    "canGoPrevious" | "onPreviousPage"
  >,
  {
    isLoading,
    currentPageItems,
  }: {
    isLoading: boolean;
    /** `undefined` when the query is paused or errored: not an empty page. */
    currentPageItems: readonly unknown[] | undefined;
  },
) {
  const { canGoPrevious, onPreviousPage } = pagination;
  if (!isLoading && currentPageItems?.length === 0 && canGoPrevious) {
    onPreviousPage();
  }
}
