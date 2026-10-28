import flatMap from "lodash/flatMap";
import { createInfiniteHook } from "swr-openapi";
import {
  AuditLogEventResponse,
  ListAuditLogEventsResponse,
} from "@convex-dev/platform/managementApi";
import { managementApiClient } from "api/api";
import { useAuthHeader } from "hooks/fetching";

export type AuditLogAction = AuditLogEventResponse["action"];

const useInfinite = createInfiniteHook(managementApiClient, "management-api");

export function useTeamAuditLog(
  teamId: number,
  {
    from,
    to,
    memberId,
    action,
  }: {
    from: number;
    to: number;
    memberId: string | null;
    action: AuditLogAction | null;
  },
): {
  entries?: AuditLogEventResponse[];
  isLoading: boolean;
  loadNextPage: () => void;
  hasMore: boolean;
} {
  const authHeader = useAuthHeader();

  const { data, isLoading, setSize } = useInfinite(
    "/teams/{team_id}/list_audit_log_events",
    (
      pageIndex: number,
      previousPageData: ListAuditLogEventsResponse | null,
    ) => {
      if (previousPageData && !previousPageData.pagination.nextCursor) {
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
            from,
            to,
            ...(memberId ? { memberId: Number(memberId) } : {}),
            ...(action ? { action } : {}),
            cursor:
              pageIndex > 0
                ? (previousPageData?.pagination.nextCursor ?? undefined)
                : undefined,
          },
        },
      };
    },
  );

  return {
    entries: flatMap(
      data?.map((page) => page.items),
      (entry) => entry,
    ),
    loadNextPage: () => setSize((size) => size + 1),
    isLoading,
    hasMore: !!data?.[data.length - 1]?.pagination.hasMore,
  };
}
