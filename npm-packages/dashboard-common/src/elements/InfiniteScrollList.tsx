import InfiniteLoader from "react-window-infinite-loader";
import AutoSizer from "react-virtualized-auto-sizer";
import {
  ListOnScrollProps,
  FixedSizeList,
  FixedSizeListProps,
} from "react-window";
import {
  useEffect,
  RefObject,
  MutableRefObject,
  useCallback,
  forwardRef,
} from "react";

export function InfiniteScrollList<T, U>({
  items,
  totalNumItems,
  pageSize = 10,
  loadMoreThreshold = 25,
  loadMore,
  itemSize,
  itemData,
  RowOrLoading,
  listRef,
  outerRef,
  onScroll,
  className,
  itemKey,
  overscanCount,
  initialScrollOffset,
  style,
}: {
  // List of loaded items to render.
  items: T[];
  // Optionally, supply the total number of items to render. Controls the size of the scrollbar.
  totalNumItems?: number;
  // Number of items to load at a time.
  pageSize?: number;
  // Controls when to load more items. When the user scrolls within this many items of the bottom of the list, loadMore will be called.
  loadMoreThreshold?: number;
  // Function to load more items. Should load pageSize items.
  loadMore?: (pageSize: number) => void;
  // Height of each item in pixels.
  itemSize: number;
  // Data to pass to each row.
  itemData: U;
  // Component to render for each row. Should handle rendering a loading state if the data is not yet loaded.
  RowOrLoading: React.ComponentType<{
    index: number;
    style: React.CSSProperties;
    data: U;
  }>;
  // Ref to the list. Can be used to sync a custom scrollbar with the list, or to scroll to a specific location in the list.
  listRef?: MutableRefObject<FixedSizeList | null>;
  // Ref to set on the outer element of the list. Used internally to load more items if the user scrolls deep into the list.
  outerRef: RefObject<HTMLElement | null>;
  // Hook called when the user scrolls.
  onScroll?: (props: ListOnScrollProps) => void;
  className?: string;
  style?: React.CSSProperties;
  // Function to generate a key for each item. Defaults to the index of the item.
  itemKey?: (index: number, data: U) => string;
  overscanCount?: number;
  initialScrollOffset?: number;
}) {
  useEffect(() => {
    if (
      loadMore &&
      outerRef.current?.clientHeight &&
      items.length * itemSize <=
        outerRef.current.scrollTop + outerRef.current.clientHeight
    ) {
      loadMore(pageSize);
    }
  }, [items, loadMore, itemSize, outerRef, pageSize]);

  return (
    <AutoSizer>
      {({ height, width }) => (
        <InfiniteLoader
          isItemLoaded={(idx) => items.length > idx}
          itemCount={Math.max(items.length, totalNumItems || 0)}
          loadMoreItems={() => loadMore && loadMore(pageSize)}
          minimumBatchSize={pageSize}
          threshold={loadMoreThreshold}
        >
          {({ onItemsRendered, ref }) => (
            <List
              outerRef={outerRef}
              listRef={listRef}
              overscanCount={overscanCount}
              initialScrollOffset={initialScrollOffset}
              ref={ref}
              onScroll={onScroll}
              className={className || "scrollbar"}
              style={style}
              onItemsRendered={onItemsRendered}
              itemData={itemData}
              itemCount={Math.max(items.length, totalNumItems || 0)}
              height={
                height! /* see https://github.com/bvaughn/react-virtualized-auto-sizer/issues/45 */
              }
              width={
                width! /* see https://github.com/bvaughn/react-virtualized-auto-sizer/issues/45 */
              }
              itemSize={itemSize}
              itemKey={itemKey}
            >
              {RowOrLoading}
            </List>
          )}
        </InfiniteLoader>
      )}
    </AutoSizer>
  );
}

// Wrapper around FixedSizeList that both
// forwards its ref and assigns it to `listRef` prop
const List = forwardRef<
  any,
  FixedSizeListProps & {
    listRef?: MutableRefObject<FixedSizeList | null>;
  }
>(function List({ children, listRef, ...props }, ref) {
  const setRefs = useCallback(
    (node: any) => {
      if (typeof ref === "function") {
        ref(node);
      }
      if (listRef) {
        listRef.current = node;
      }
    },
    [ref, listRef],
  );
  return (
    <FixedSizeList ref={setRefs} {...props}>
      {children}
    </FixedSizeList>
  );
});
