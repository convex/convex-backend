import { useRouter } from "next/router";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useDebounce } from "react-use";
import flatMap from "lodash/flatMap";
import { useInitialData } from "hooks/useServerSideData";
import { useAuthHeader } from "hooks/fetching";
import type { PaginatedDeploymentsResponse } from "@convex-dev/platform/managementApi";
import { useCurrentProject } from "./projects";
import {
  useManagementApiInfiniteQuery,
  useManagementApiMutation,
  useManagementApiQuery,
} from "./api";
import { useDeploymentsPageSize } from "hooks/useDeploymentsPageSize";

export function useDeployments(projectId?: number) {
  const [initialData] = useInitialData();
  const { data, isLoading } = useManagementApiQuery({
    path: "/projects/{project_id}/list_deployments",
    pathParams: {
      project_id: projectId || 0,
    },
    queryParams: {
      includeLocal: true,
    },
    swrOptions: {
      revalidateOnMount: initialData === undefined,
      refreshInterval: 30 * 1000,
      keepPreviousData: false,
    },
  });

  return { deployments: data, isLoading };
}

export function useCurrentDeployment() {
  const project = useCurrentProject();
  const { deployments, isLoading } = useDeployments(project?.id);
  const { push, query } = useRouter();
  const deploymentName =
    typeof query.deploymentName === "string" ? query.deploymentName : undefined;
  const deployment = deployments?.find((d) => d.name === deploymentName);
  const projectSlug =
    typeof query.project === "string" ? query.project : undefined;

  // The deployment doesn't exist.
  if (
    !isLoading &&
    project &&
    deployments &&
    deployments.length > 0 &&
    !deployment &&
    deploymentName
  ) {
    if (projectSlug && typeof window !== "undefined") {
      const key = `/lastViewedDeploymentForProject/${projectSlug}`;
      const lastViewedDeploymentForProject = window.localStorage.getItem(key);
      if (lastViewedDeploymentForProject === deploymentName) {
        window.localStorage.removeItem(key);
      }
    }
    void push("/404");
  }

  return deployment;
}

export function useProvisionDeployment(projectId: number) {
  return useManagementApiMutation({
    path: "/projects/{project_id}/create_deployment",
    pathParams: {
      project_id: projectId,
    },
    mutateKey: `/projects/{project_id}/list_deployments`,
    mutatePathParams: {
      project_id: projectId,
    },
  });
}

export function useModifyDeploymentSettings({
  deploymentName,
  projectId,
}: {
  deploymentName: string | undefined;
  projectId: number | undefined;
}) {
  return useManagementApiMutation({
    path: "/deployments/{deployment_name}",
    pathParams: {
      deployment_name: deploymentName ?? "",
    },
    method: "patch",
    mutateKey: `/projects/{project_id}/list_deployments`,
    mutatePathParams: {
      project_id: projectId ?? 0,
    },
    successToast: "Deployment settings updated successfully",
  });
}

export function useDeploymentRegions(teamId: number | undefined) {
  const { data, isLoading } = useManagementApiQuery({
    path: "/teams/{team_id}/list_deployment_regions",
    pathParams: {
      team_id: teamId?.toString() || "",
    },
    swrOptions: {
      isPaused: () => teamId === undefined,
    },
  });

  return { regions: data?.items, isLoading };
}

export function useDeploymentByName(deploymentName?: string) {
  const { data: deployment } = useManagementApiQuery({
    path: "/deployments/{deployment_name}",
    pathParams: {
      deployment_name: deploymentName || "",
    },
  });

  return deployment;
}

export function useDeleteDeployment(
  projectId: number,
  deploymentName: string,
  settingsUrl: string,
) {
  const deleteDeployment = useManagementApiMutation({
    path: "/deployments/{deployment_name}/delete",
    method: "post",
    pathParams: { deployment_name: deploymentName || "" },
    mutateKey: `/projects/{project_id}/list_deployments`,
    mutatePathParams: { project_id: projectId },
    successToast: "Deleted deployment.",
    redirectTo: settingsUrl,
  });

  return deleteDeployment;
}

export function useTransferDeployment(deploymentName: string) {
  return useManagementApiMutation({
    path: "/deployments/{deployment_name}/transfer",
    method: "post",
    pathParams: { deployment_name: deploymentName },
    successToast: "Deployment transferred.",
  });
}

export function usePaginatedDeployments(
  teamId: number | undefined,
  options: {
    cursor?: string;
    sortBy?: string;
    sortOrder?: string;
    deploymentType?: string;
    q?: string;
    projectId?: number;
    creator?: number;
    isDefault?: boolean;
    keepPreviousData?: boolean;
  },
  refreshInterval?: number,
) {
  const { pageSize } = useDeploymentsPageSize();

  const {
    cursor,
    sortBy,
    sortOrder,
    deploymentType,
    q,
    projectId,
    creator,
    isDefault,
    keepPreviousData,
  } = options;

  // Note: the OpenAPI spec uses snake_case names, but the Rust handler has
  // #[serde(rename_all = "camelCase")] so it actually expects camelCase params.
  const queryParams = useMemo(
    () => ({
      cursor,
      limit: pageSize,
      sortBy,
      sortOrder,
      deploymentType,
      q,
      projectId,
      creator,
      isDefault,
    }),
    [
      cursor,
      pageSize,
      sortBy,
      sortOrder,
      deploymentType,
      q,
      projectId,
      creator,
      isDefault,
    ],
  );

  const { data, isLoading } = useManagementApiQuery({
    path: "/teams/{team_id}/list_deployments",
    pathParams: {
      team_id: teamId ?? 0,
    },
    queryParams,
    swrOptions: { refreshInterval, keepPreviousData },
  });

  if (data === undefined) {
    return undefined;
  }

  return { ...data, isLoading };
}

export function useInfiniteDeployments(
  teamId: number | undefined,
  searchQuery: string = "",
  options: {
    projectId?: number;
    keepPreviousData?: boolean;
  } = {},
) {
  const authHeader = useAuthHeader();
  const { pageSize } = useDeploymentsPageSize();
  const { projectId, keepPreviousData = true } = options;
  const [debouncedQuery, setDebouncedQuery] = useState("");

  // Debounce search query (300ms delay)
  useDebounce(
    () => {
      setDebouncedQuery(searchQuery);
    },
    300,
    [searchQuery],
  );

  const { data, isLoading, size, setSize } = useManagementApiInfiniteQuery(
    "/teams/{team_id}/list_deployments",
    (
      pageIndex: number,
      previousPageData: PaginatedDeploymentsResponse | null,
    ): any => {
      // Stop if we've reached the end (but allow first page)
      if (
        pageIndex > 0 &&
        previousPageData &&
        !previousPageData.pagination.hasMore
      ) {
        return null;
      }

      return {
        headers: {
          Authorization: authHeader,
          "Convex-Client": "dashboard-0.0.0",
        },
        params: {
          path: {
            team_id: teamId ?? 0,
          },
          // The OpenAPI spec names these snake_case, but the Rust handler is
          // #[serde(rename_all = "camelCase")], so send camelCase.
          query: {
            limit: pageSize,
            cursor:
              pageIndex > 0 && previousPageData
                ? previousPageData.pagination.nextCursor
                : undefined,
            ...(projectId !== undefined ? { projectId } : {}),
            ...(debouncedQuery.trim() ? { q: debouncedQuery.trim() } : {}),
          },
        },
      };
    },
    {
      keepPreviousData,
    },
  );

  // Manual reset when the query or scoped project changes
  const prevKey = useRef(`${debouncedQuery}:${projectId ?? ""}`);
  useEffect(() => {
    const key = `${debouncedQuery}:${projectId ?? ""}`;
    if (prevKey.current !== key) {
      prevKey.current = key;
      void setSize(1);
    }
  }, [debouncedQuery, projectId, setSize]);

  const deployments = useMemo(
    () => flatMap(data?.map((page) => page.items)),
    [data],
  );

  const hasMore = data?.[data.length - 1]?.pagination.hasMore ?? false;

  // isLoading is only true when the first page is loading
  // (see https://swr.vercel.app/examples/infinite-loading)
  const isLoadingMore =
    isLoading || (size > 0 && data && typeof data[size - 1] === "undefined");

  const loadMore = useCallback(() => {
    if (hasMore && !isLoadingMore) {
      void setSize((prevSize) => prevSize + 1);
    }
  }, [hasMore, isLoadingMore, setSize]);

  return {
    deployments,
    isLoading,
    isLoadingMore,
    hasMore,
    loadMore,
    debouncedQuery,
    pageSize,
  };
}
