import { ConvexReactClient } from "convex/react";
import { useCallback, useState, useRef, useEffect, useContext } from "react";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { useDeploymentUrl, useAdminKey } from "@common/lib/deploymentApi";
import { toast } from "@common/lib/utils";
import type { FunctionReference } from "convex/server";

const RATE_LIMIT_BUCKET_MS = 10 * 1000;
// > 10 updates per second
const RATE_LIMIT_THRESHOLD = 10 * 10;

/**
 * Hook to handle rate limiting for live data
 */
function useRateLimitChanges<T>(
  items: T[],
  isPaused: boolean,
  onRateLimited: () => void,
) {
  const callCountRef = useRef(0);
  const lastResetRef = useRef(Date.now());

  const [isRateLimited, setIsRateLimited] = useState(false);

  useEffect(() => {
    if (isRateLimited || isPaused) {
      return;
    }

    const now = Date.now();
    if (now - lastResetRef.current > RATE_LIMIT_BUCKET_MS) {
      callCountRef.current = 0;
      lastResetRef.current = now;
    }

    callCountRef.current += 1;

    if (callCountRef.current > RATE_LIMIT_THRESHOLD) {
      setIsRateLimited(true);
      onRateLimited();
    }
  }, [items, isPaused, isRateLimited, onRateLimited]);

  return isRateLimited;
}

/**
 * Hook to manage paused state for live data
 * @param results - The current results from the live query
 * @param args - Arguments for the query
 * @param storageKey - Key suffix to use for localStorage
 * @param udfName - Name or reference of the UDF to call when loading paused data
 * @param numItems - Number of items to fetch per page
 */
export function usePausedLiveData<TResult, TArgs>({
  results,
  args,
  storageKey,
  udfName,
  numItems = 50,
}: {
  results: TResult[];
  args: TArgs;
  storageKey: string;
  udfName: string | FunctionReference<"query">;
  numItems?: number;
}) {
  const [pausedData, setPausedData] = useState<TResult[]>(results);
  const [lastCursor, setLastCursor] = useState<string | null>(null);
  const [canLoadMore, setCanLoadMore] = useState(true);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const currentArgsRef = useRef(args);

  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const deployment = useCurrentDeployment();

  // Store the paused state in local storage so it persists across refreshes
  const [isPaused, setIsPaused] = useGlobalLocalStorage(
    `${deployment?.name}/${storageKey}`,
    false,
  );

  const onRateLimited = () => {
    // When we get rate limited, we should immediately pause
    setIsPaused(true);
    // Store the current result set from the live query so we can show it when paused
    setPausedData(results);
    toast(
      "error",
      `There are too many updates to show live. Updates have automatically been paused.`,
      "liveUpdatesPaused",
    );
  };

  const isRateLimited = useRateLimitChanges(results, isPaused, onRateLimited);

  const [isLoadingPausedData, setIsLoadingPausedData] = useState(false);
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();

  // Convex client for making one-off queries
  const [client, setClient] = useState<ConvexReactClient>();
  useEffect(() => {
    const c = new ConvexReactClient(deploymentUrl, {
      reportDebugInfoToConvex: true,
    });
    setClient(c);
    return () => {
      void c.close();
    };
  }, [deploymentUrl]);

  useEffect(() => {
    if (client === undefined) return;
    client.setAdminAuth(adminKey);
  }, [client, adminKey]);

  // Helper function to query with proper typing
  const queryWithArgs = useCallback(
    async (c: ConvexReactClient, cursor: string | null, id: number) => {
      // Handle both string and FunctionReference types
      const result =
        typeof udfName === "string"
          ? await (c as any).query(udfName, {
              ...args,
              paginationOpts: {
                numItems,
                cursor,
                id,
              },
            })
          : await c.query(udfName, {
              ...args,
              paginationOpts: {
                numItems,
                cursor,
                id,
              },
            });
      return result;
    },
    [args, numItems, udfName],
  );

  const loadFirstPage = useCallback(async () => {
    if (client === undefined) return;

    setIsLoadingPausedData(true);
    try {
      // Fetch one page
      const result = await queryWithArgs(client, null, 0);
      setPausedData(result.page);
      setLastCursor(result.continueCursor);
      setCanLoadMore(!result.isDone);
    } catch {
      toast("error", "Failed to load data", "loadData");
    } finally {
      setIsLoadingPausedData(false);
    }
  }, [client, queryWithArgs]);

  const loadMorePaused = useCallback(async () => {
    if (client === undefined || !canLoadMore || isLoadingMore) return;

    setIsLoadingMore(true);
    try {
      const result = await queryWithArgs(client, lastCursor, pausedData.length);

      setPausedData((prev) => [...prev, ...result.page]);
      setLastCursor(result.continueCursor);
      setCanLoadMore(!result.isDone);
    } catch {
      toast("error", "Failed to load more data", "loadMoreData");
    } finally {
      setIsLoadingMore(false);
    }
  }, [
    client,
    canLoadMore,
    isLoadingMore,
    lastCursor,
    pausedData.length,
    queryWithArgs,
  ]);

  // The client only exists from the render after mount, so the initial load for
  // an already-paused view waits for it. The ref keeps this to the first load.
  const hasLoadedFirstPageRef = useRef(false);
  useEffect(() => {
    if (client === undefined || hasLoadedFirstPageRef.current) return;
    hasLoadedFirstPageRef.current = true;
    if (isPaused) {
      void loadFirstPage();
    }
  }, [client, isPaused, loadFirstPage]);

  // Check if args have changed, and if isPaused, reload the data
  useEffect(() => {
    // Check if there are meaningful differences in the args
    const argsChanged =
      JSON.stringify(args) !== JSON.stringify(currentArgsRef.current);

    if (argsChanged) {
      currentArgsRef.current = args;

      // If we're paused and args changed, reload the data with the new args
      if (isPaused && !isLoadingPausedData) {
        void loadFirstPage();
      }
    }
  }, [args, isPaused, isLoadingPausedData, loadFirstPage]);

  return {
    pausedData,
    isLoadingPausedData,
    isLoadingMore,
    canLoadMore,
    isRateLimited,
    togglePaused: useCallback(() => {
      setPausedData(results);
      setLastCursor(null);
      setCanLoadMore(true);
      setIsPaused(!isPaused);
    }, [results, setIsPaused, isPaused]),
    reload: loadFirstPage,
    loadMorePaused,
  };
}
