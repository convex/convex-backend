import { RefObject, useCallback, useEffect, useState, useRef } from "react";
import { useWindowSize } from "react-use";
import { FixedSizeList } from "react-window";
import { useTableDensity } from "@common/features/data/lib/useTableDensity";

const MIN_SCROLLBAR_SIZE = 64;

// The handlers returned by this hook were mostly copied/inspired by:
// https://www.thisdot.co/blog/creating-custom-scrollbars-with-react
function useScrollbar(
  totalRowCount: number,
  outerRef: RefObject<HTMLElement | null>,
  listRef: RefObject<FixedSizeList | null>,
) {
  const { densityValues } = useTableDensity();
  // Recompute scrollbar size when window is resize by forcing a rerender.
  useWindowSize();

  const totalRowHeight = totalRowCount * densityValues.height;

  const { current: outerEl } = outerRef;
  const { current: listEl } = listRef;

  const scrollbarHeight =
    !outerEl || totalRowHeight < outerEl.offsetHeight
      ? 0
      : (outerEl.offsetHeight / totalRowHeight) * outerEl.offsetHeight;

  const [scrollStartPosition, setScrollStartPosition] = useState<number | null>(
    null,
  );
  const [initialScrollTop, setInitialScrollTop] = useState<number>(0);
  const [isDragging, setIsDragging] = useState(false);
  const rafIdRef = useRef<number>(0);
  const lastMouseYRef = useRef<number | null>(null);

  const handleTrackClick = useCallback(
    (e: React.MouseEvent) => {
      if (outerEl && listEl) {
        // First, figure out where we clicked
        const { clientY } = e;
        // Next, figure out the distance between the top of the track and the top of the viewport
        const target = e.target as HTMLDivElement;
        const rect = target.getBoundingClientRect();
        const trackTop = rect.top;
        // We want the middle of the thumb to jump to where we clicked, so we subtract half the thumb's height to offset the position
        const thumbOffset = -(scrollbarHeight / 2);
        // Find the ratio of the new position to the total content length using the thumb and track values...
        const clickRatio =
          (clientY - trackTop + thumbOffset) / outerEl.clientHeight;
        // ...so that you can compute where the content should scroll to.
        const scrollAmount = Math.floor(
          clickRatio * totalRowCount * densityValues.height,
        );
        // And finally, scroll to the new position!
        listEl.scrollTo(scrollAmount);
      }
    },
    [outerEl, listEl, scrollbarHeight, totalRowCount, densityValues.height],
  );
  const handleThumbMousedown = useCallback(
    (e: MouseEvent) => {
      setScrollStartPosition(e.clientY);
      if (outerEl) setInitialScrollTop(outerEl.scrollTop);
      setIsDragging(true);
    },
    [outerEl],
  );

  const handleThumbMouseup = useCallback(() => {
    if (isDragging) {
      setIsDragging(false);

      // Cancel any pending animation frame
      if (rafIdRef.current) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = 0;
      }
    }
  }, [isDragging]);

  const updateScrollPosition = useCallback(() => {
    if (
      isDragging &&
      outerEl &&
      listEl &&
      scrollStartPosition &&
      lastMouseYRef.current !== null
    ) {
      const {
        scrollHeight: contentScrollHeight,
        offsetHeight: contentOffsetHeight,
      } = outerEl;

      // Subtract the current mouse y position from where you started to get the pixel difference
      const deltaY =
        (lastMouseYRef.current - scrollStartPosition) *
        (contentOffsetHeight / scrollbarHeight);

      const newScrollTop = Math.max(
        0,
        Math.min(
          initialScrollTop + deltaY,
          contentScrollHeight - contentOffsetHeight,
        ),
      );

      // Apply the scroll
      listEl?.scrollTo(newScrollTop);

      // Continue animation loop
      rafIdRef.current = requestAnimationFrame(updateScrollPosition);
    }
  }, [
    isDragging,
    scrollStartPosition,
    scrollbarHeight,
    outerEl,
    listEl,
    initialScrollTop,
  ]);

  const handleThumbMousemove = useCallback(
    (e: MouseEvent) => {
      if (isDragging) {
        e.preventDefault();
        e.stopPropagation();

        // Store mouse position for animation frame
        lastMouseYRef.current = e.clientY;

        // Start animation frame if not already running
        if (!rafIdRef.current) {
          rafIdRef.current = requestAnimationFrame(updateScrollPosition);
        }

        // Add user-select: none to body during drag
        document.body.style.userSelect = "none";
      }
    },
    [isDragging, updateScrollPosition],
  );

  // Listen for mouse events to handle scrolling by dragging the thumb
  useEffect(() => {
    document.addEventListener("mousemove", handleThumbMousemove);
    document.addEventListener("mouseup", handleThumbMouseup);
    document.addEventListener("mouseleave", handleThumbMouseup);
    return () => {
      document.removeEventListener("mousemove", handleThumbMousemove);
      document.removeEventListener("mouseup", handleThumbMouseup);
      document.removeEventListener("mouseleave", handleThumbMouseup);

      // Reset user-select
      document.body.style.userSelect = "";

      // Clean up any ongoing animation frame
      if (rafIdRef.current) {
        cancelAnimationFrame(rafIdRef.current);
      }
    };
  }, [handleThumbMousemove, handleThumbMouseup]);

  return {
    listRef,
    outerRef,
    scrollbarHeight,
    scrollbarTop: outerEl
      ? // Use Math.min and Math.max to make sure the scrollbar doesn't go out of bounds.
        Math.min(
          Math.max(
            (outerEl.scrollTop / totalRowHeight) * outerEl.offsetHeight,
            0,
          ),
          // Subtract an extra pixel to prevent scrollbar from going too far down
          outerEl.offsetHeight - scrollbarHeight - 6,
        )
      : 0,
    handleTrackClick,
    handleThumbMousedown,
    isDragging,
  };
}

export function TableScrollbar({
  totalRowCount,
  outerRef,
  listRef,
}: {
  totalRowCount?: number;
  outerRef: RefObject<HTMLElement | null>;
  listRef: RefObject<FixedSizeList | null>;
}) {
  const {
    scrollbarHeight,
    scrollbarTop,
    handleTrackClick,
    handleThumbMousedown,
    isDragging,
  } = useScrollbar(totalRowCount || 0, outerRef, listRef);

  // Create a React handler from a native event handler, just for the types.
  const handleReactThumbMousedown = useCallback(
    (e: React.MouseEvent) => handleThumbMousedown(e.nativeEvent),
    [handleThumbMousedown],
  );

  const { densityValues } = useTableDensity();
  return scrollbarHeight > 0 ? (
    <div
      className="absolute -right-px w-3 border-t border-l bg-macosScrollbar-track/75 py-0.5"
      role="scrollbar"
      aria-controls="dataTable"
      aria-valuenow={scrollbarTop}
      style={{
        height: `${outerRef.current?.offsetHeight}px`,
        top: densityValues.height,
      }}
    >
      {/* eslint-disable  */}
      {/* I have no clue how to properly do a11y for this scrollbar,
            but it seems to work well for scrollbars */}
      <div onClick={handleTrackClick} className="fixed h-full w-2.5" />
      <div
        style={{
          height:
            scrollbarHeight === 0
              ? // No scrollbar needed
                0
              : // Use the calculated size of the scrollbar, or the min size,
                // whichever is greater.
                Math.max(scrollbarHeight, MIN_SCROLLBAR_SIZE),
          marginTop:
            scrollbarHeight < MIN_SCROLLBAR_SIZE
              ? // If the scrollbar is using the minimum size, add some margin
                // to the top so it snaps to the top and bottom of the table.
                Math.max(scrollbarTop - MIN_SCROLLBAR_SIZE, 0)
              : scrollbarTop,
          cursor: isDragging ? "grabbing" : "grab",
        }}
        onMouseDown={handleReactThumbMousedown}
        className="fixed w-1.5 rounded-full transition-colors bg-macosScrollbar-thumb hover:bg-macosScrollbar-thumbHover ml-0.5"
      />
      {/* eslint-enable */}
    </div>
  ) : null;
}
