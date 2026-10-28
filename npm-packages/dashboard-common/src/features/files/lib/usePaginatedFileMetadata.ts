import { usePaginatedQuery } from "convex/react";
import { useContext, useState } from "react";
import udfs from "@common/udfs";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { useNents } from "@common/lib/useNents";
import { usePausedLiveData } from "@common/lib/usePausedLiveData";
import { FileFilters } from "../components/FileStorageHeader";

export const FILE_METADATA_PAGE_SIZE = 20;

export function usePaginatedFileMetadata() {
  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const deployment = useCurrentDeployment();
  const canViewData = useIsOperationAllowed("ViewData");
  const [filters, setFilters] = useState<FileFilters>({
    order: "desc",
  });

  const [isPaused] = useGlobalLocalStorage(
    `${deployment?.name}/pauseLiveFileStorage`,
    false,
  );

  const args = {
    componentId: useNents().selectedNent?.id ?? null,
    filters: {
      minCreationTime: filters.minCreationTime,
      maxCreationTime: filters.maxCreationTime,
      order: filters.order,
    },
  };

  const { results, loadMore, status } = usePaginatedQuery(
    udfs.fileStorageV2.fileMetadata,
    // If we're paused or don't have ViewData permission, don't show the live query.
    isPaused || !canViewData ? "skip" : args,
    {
      initialNumItems: FILE_METADATA_PAGE_SIZE,
    },
  );

  const {
    pausedData,
    isLoadingPausedData,
    isRateLimited,
    togglePaused,
    reload,
    loadMorePaused,
    canLoadMore,
    isLoadingMore,
  } = usePausedLiveData({
    results,
    args,
    storageKey: "pauseLiveFileStorage",
    udfName: udfs.fileStorageV2.fileMetadata,
    numItems: FILE_METADATA_PAGE_SIZE,
  });

  return {
    files: isPaused ? pausedData : results,
    status: isPaused
      ? isLoadingPausedData
        ? "LoadingFirstPage"
        : canLoadMore
          ? "CanLoadMore"
          : "Exhausted"
      : status,
    isPaused,
    isLoadingPausedData,
    isLoadingMore,
    isRateLimited,
    loadMore: isPaused ? loadMorePaused : loadMore,
    togglePaused,
    reload,
    filters,
    setFilters,
  };
}
