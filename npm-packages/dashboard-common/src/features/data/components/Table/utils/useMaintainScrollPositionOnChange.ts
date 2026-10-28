import { RefObject, useCallback, useEffect, useRef } from "react";

/**
 * Ensures that the scroll position changes automatically when `data` updates
 * so that the topmost row stays at its current position.
 *
 * @param data The source data of the list
 * @param scrollRef The scroll view
 * @param getRowId Returns a unique identifier given a row
 * @param rowHeight The (fixed) height of a row
 * @param onRowChangeAbove Called when a row is added/removed above the scrolling position
 */
export function useMaintainScrollPositionOnChange<T>(
  data: T[],
  scrollRef: RefObject<HTMLElement | null>,
  getRowId: (row: T) => string,
  rowHeight: number,
  onRowChangeAbove: () => void,
) {
  const computeTopmostRowId = useCallback(() => {
    if (!scrollRef.current) return null;
    const topmostRowIndex = Math.floor(scrollRef.current.scrollTop / rowHeight);
    const topmostRow = data[topmostRowIndex];
    return topmostRow ? getRowId(topmostRow) : null;
  }, [data, getRowId, rowHeight, scrollRef]);

  const topmostRowId = useRef<string | null>(null);
  // The scrollTop we last set ourselves. The scroll event it triggers should
  // not be treated as the user scrolling. We match on the value rather than a
  // simple "ignore next event" boolean: that boolean got stuck whenever our
  // write was a no-op (no event to clear it) or coalesced with a real user
  // scroll, which then swallowed the user's scroll and bounced the view back.
  const programmaticScrollTop = useRef<number | null>(null);

  // Remember the topmost row
  useEffect(() => {
    const onScroll = () => {
      if (
        programmaticScrollTop.current !== null &&
        scrollRef.current?.scrollTop === programmaticScrollTop.current
      ) {
        // This is the scroll event from our own scrollTop write; ignore it.
        programmaticScrollTop.current = null;
        return;
      }

      // Any other scroll position means the user (or the scrollbar) scrolled.
      programmaticScrollTop.current = null;
      topmostRowId.current = computeTopmostRowId();
    };

    const list = scrollRef.current;
    if (!list) return undefined;
    list.addEventListener("scroll", onScroll);
    return () => list?.removeEventListener("scroll", onScroll);
  }, [computeTopmostRowId, scrollRef]);

  // Enforce stickiness
  useEffect(() => {
    if (!scrollRef.current) {
      return;
    }

    if (!topmostRowId.current || scrollRef.current.scrollTop <= 0) {
      topmostRowId.current = computeTopmostRowId();
    }

    // Exit early if there is nothing to do to avoid searching for the new row position
    const currentTopmostRowId = computeTopmostRowId();
    if (currentTopmostRowId === topmostRowId.current) return;

    // Find the new position of the topmost row and scroll to it
    const newTopmostRowIndex = data.findIndex(
      (row) => getRowId(row) === topmostRowId.current,
    );
    if (newTopmostRowIndex === -1) {
      // Row deleted?
      topmostRowId.current = computeTopmostRowId();
      return;
    }

    const newScrollTop =
      newTopmostRowIndex * rowHeight +
      (scrollRef.current.scrollTop % rowHeight);

    // Only write when the position actually changes, so we never record a
    // programmatic scrollTop that won't produce a scroll event.
    if (newScrollTop !== scrollRef.current.scrollTop) {
      programmaticScrollTop.current = newScrollTop;
      scrollRef.current.scrollTop = newScrollTop;
      onRowChangeAbove();
    }
  }, [
    computeTopmostRowId,
    data,
    getRowId,
    rowHeight,
    scrollRef,
    onRowChangeAbove,
  ]);
}
