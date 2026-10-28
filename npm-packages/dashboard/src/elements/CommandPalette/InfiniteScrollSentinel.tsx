import { useCallback, useRef } from "react";
import { Spinner } from "@ui/Spinner";
import { LoadingSignal } from "./items";

// A non-selectable row that loads the next page as it scrolls into view, so a
// paginated cmdk list scrolls infinitely.
export function InfiniteScrollSentinel({
  hasMore,
  isLoadingMore,
  loadMore,
}: {
  hasMore: boolean;
  isLoadingMore: boolean;
  loadMore: () => void;
}) {
  const observerRef = useRef<IntersectionObserver | null>(null);

  const setSentinel = useCallback(
    (el: HTMLDivElement | null) => {
      observerRef.current?.disconnect();
      observerRef.current = null;
      if (
        !el ||
        !hasMore ||
        isLoadingMore ||
        typeof IntersectionObserver === "undefined"
      ) {
        return;
      }
      // Observe within cmdk's scroll container so intersection is measured
      // against the list viewport.
      const root = el.closest("[cmdk-list]");
      const observer = new IntersectionObserver(
        (entries) => {
          if (entries[0]?.isIntersecting) {
            loadMore();
          }
        },
        { root, rootMargin: "150px" },
      );
      observer.observe(el);
      observerRef.current = observer;
    },
    [hasMore, isLoadingMore, loadMore],
  );

  if (!hasMore) {
    return null;
  }

  return (
    <div ref={setSentinel} aria-hidden className="flex justify-center py-2">
      {isLoadingMore && (
        <>
          <Spinner className="size-4" />
          <LoadingSignal rows={0} />
        </>
      )}
    </div>
  );
}
