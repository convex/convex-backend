import { useRouter } from "next/router";
import { SWRConfiguration } from "swr";
import { useMemo, useEffect, useRef, useState, useCallback } from "react";
import { useDebounce } from "react-use";
import { useProjectsPageSize } from "hooks/useProjectsPageSize";
import { useAuthHeader } from "hooks/fetching";
import flatMap from "lodash/flatMap";
import { useCurrentTeam } from "./teams";
import type { PlatformPaginatedProjectsResponse } from "@convex-dev/platform/managementApi";
import {
  useBBMutation,
  useManagementApiInfiniteQuery,
  useManagementApiMutation,
  useManagementApiQuery,
  useMutateManagementApi,
} from "./api";

export function useCurrentProject() {
  return useCurrentProjectWithStatus().project;
}

/**
 * The current project plus whether it's still resolving, for callers that have
 * to tell "not here yet" from "not there" — a distinction `useCurrentProject`
 * erases by returning `undefined` for both.
 */
export function useCurrentProjectWithStatus() {
  const team = useCurrentTeam();
  const { query } = useRouter();
  const { project: projectSlug } = query;
  return useProjectBySlugWithStatus(team?.id, projectSlug as string);
}

export function useProjectById(projectId: number | undefined) {
  const { data, isLoading, error } = useManagementApiQuery({
    path: "/projects/{project_id}",
    pathParams: {
      project_id: projectId ?? 0,
    },
  });
  return { project: data, isLoading, error };
}

export function useProjectBySlug(
  teamId: number | undefined,
  projectSlug: string | undefined,
) {
  return useProjectBySlugWithStatus(teamId, projectSlug).project;
}

function useProjectBySlugWithStatus(
  teamId: number | undefined,
  projectSlug: string | undefined,
) {
  const { data, isLoading, isValidating } = useManagementApiQuery({
    path: "/teams/{team_id_or_slug}/projects/{project_slug}",
    pathParams: {
      team_id_or_slug: teamId?.toString() || "",
      project_slug: projectSlug || "",
    },
  });
  // keepPreviousData hands back the previously viewed project while the new
  // slug loads, and keeps handing it back if that request fails. Withhold it
  // either way, since returning the wrong project races project/deployment
  // 404s -- but only report loading while a request is actually in flight.
  // Reporting the failure as loading would strand callers that wait for this to
  // settle, since the retained project never starts matching the new slug.
  const isStale = data !== undefined && data.slug !== projectSlug;
  return {
    project: isLoading || isStale ? undefined : data,
    isLoading: isLoading || (isStale && isValidating),
  };
}

export function usePaginatedProjects(
  teamId: number | undefined,
  options: {
    cursor?: string;
    q?: string;
    limitOverride?: number;
  },
  refreshInterval?: SWRConfiguration["refreshInterval"],
): (PlatformPaginatedProjectsResponse & { isLoading: boolean }) | undefined {
  const { pageSize } = useProjectsPageSize();

  const queryParams = useMemo(
    () => ({
      cursor: options.cursor,
      limit: options.limitOverride || pageSize,
      q: options.q,
    }),
    [options, pageSize],
  );

  const { data, isLoading, isValidating } = useManagementApiQuery({
    path: "/teams/{team_id}/projects",
    pathParams: {
      team_id: teamId ?? 0,
    },
    queryParams,
    swrOptions: {
      refreshInterval,
      // The SSR-seeded projects list is served as SWR fallback data with
      // `hasMore: false` (see lib/ssr.ts + the bigBrainAuth middleware). Always
      // revalidate on mount so the correct pagination metadata replaces that
      // seed promptly rather than waiting for a focus/interval revalidation.
      revalidateOnMount: true,
    },
  });

  if (data === undefined) {
    return undefined;
  }

  // Report loading while the mount revalidation is in flight, even though the
  // SSR seed already populated `data`, so callers don't trust its stale
  // pagination metadata.
  return { ...data, isLoading: isLoading || isValidating };
}

/**
 * Hook for infinite scroll pagination of projects with search support
 * Returns paginated projects data with loading state and pagination controls
 */
export function useInfiniteProjects(
  teamId: number,
  searchQuery: string = "",
  keepPreviousData: boolean = true,
) {
  const authHeader = useAuthHeader();
  const { pageSize } = useProjectsPageSize();
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
    "/teams/{team_id}/projects",
    (
      pageIndex: number,
      previousPageData: PlatformPaginatedProjectsResponse | null,
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
            team_id: teamId,
          },
          query: {
            limit: pageSize,
            cursor:
              pageIndex > 0 && previousPageData
                ? previousPageData.pagination.nextCursor
                : undefined,
            ...(debouncedQuery.trim() ? { q: debouncedQuery.trim() } : {}),
          },
        },
      };
    },
    {
      keepPreviousData,
    },
  );

  // Manual reset when query changes
  const prevQuery = useRef(debouncedQuery);
  useEffect(() => {
    if (prevQuery.current !== debouncedQuery) {
      prevQuery.current = debouncedQuery;
      void setSize(1);
    }
  }, [debouncedQuery, setSize]);

  const projects = useMemo(
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
    projects,
    isLoading,
    isLoadingMore,
    hasMore,
    loadMore,
    debouncedQuery,
    pageSize,
  };
}

export function useCreateProject(teamId?: number) {
  const teamIdNum = teamId ?? 0;
  return useManagementApiMutation({
    path: "/teams/{team_id}/create_project",
    pathParams: { team_id: teamIdNum },
    mutateKey: "/teams/{team_id}/projects",
    mutatePathParams: { team_id: teamIdNum },
    googleAnalyticsEvent: "create_project_dash",
  });
}

export function useUpdateProject(projectId: number) {
  return useManagementApiMutation({
    path: "/projects/{project_id}",
    pathParams: {
      project_id: projectId,
    },
    successToast: "Project updated.",
    method: "patch",
  });
}

export function useDeleteProject(
  teamId?: number,
  projectId?: number,
  projectName?: string,
) {
  return useManagementApiMutation({
    path: "/projects/{project_id}/delete",
    pathParams: {
      project_id: projectId ?? 0,
    },
    mutateKey: "/teams/{team_id}/projects",
    mutatePathParams: {
      team_id: teamId ?? 0,
    },
    successToast: projectName ? `Deleted project: ${projectName}.` : undefined,
    redirectTo: "/",
  });
}

export function useDeleteProjects(teamId: number | undefined) {
  const deleteProjects = useBBMutation({
    path: "/delete_projects",
    pathParams: undefined,
    successToast: "Projects deleted.",
  });
  const mutateManagement = useMutateManagementApi();
  const teamIdNum = teamId ?? 0;
  return useCallback(
    async (body: { projectIds: number[] }) => {
      const result = await deleteProjects(body);
      await mutateManagement([
        "/teams/{team_id}/projects",
        { params: { path: { team_id: teamIdNum } } },
      ] as any);
      return result;
    },
    [deleteProjects, mutateManagement, teamIdNum],
  );
}

export function useTransferProject(
  projectId?: number,
  destinationTeamId?: number,
  originTeamId?: number,
) {
  const transfer = useBBMutation({
    path: "/projects/{project_id}/transfer",
    pathParams: {
      project_id: projectId?.toString() || "",
    },
    successToast: "Project transferred.",
  });
  const mutateManagement = useMutateManagementApi();
  return useCallback(
    async (body: { destinationTeamId: number }) => {
      const result = await transfer(body);
      // Invalidate both the destination team's list (the project now appears
      // there) and the origin team's list (it no longer belongs there).
      await Promise.all(
        [destinationTeamId, originTeamId]
          .filter((id): id is number => id !== undefined)
          .map((teamId) =>
            mutateManagement([
              "/teams/{team_id}/projects",
              { params: { path: { team_id: teamId } } },
            ] as any),
          ),
      );
      return result;
    },
    [transfer, mutateManagement, destinationTeamId, originTeamId],
  );
}
