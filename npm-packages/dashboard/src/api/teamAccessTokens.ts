import { useMemo } from "react";
import { useManagementApiMutation, useManagementApiQuery } from "./api";

export function useTeamAccessTokens(teamId?: number, cursor?: string) {
  const queryParams = useMemo(() => ({ cursor }), [cursor]);

  const { data, isLoading } = useManagementApiQuery({
    path: "/teams/{team_id}/list_access_tokens",
    pathParams: {
      team_id: teamId ?? 0,
    },
    queryParams,
  });

  return { data, isLoading };
}

export function useDeleteTeamAccessToken(teamId: number) {
  return useManagementApiMutation({
    path: "/teams/{team_id}/delete_access_token",
    pathParams: { team_id: teamId },
    mutateKey: "/teams/{team_id}/list_access_tokens",
    mutatePathParams: { team_id: teamId },
    successToast: "Access token deleted.",
  });
}

export function useCreateTeamAccessToken(teamId: number) {
  return useManagementApiMutation({
    path: "/teams/{team_id}/create_access_token",
    pathParams: { team_id: teamId },
    mutateKey: "/teams/{team_id}/list_access_tokens",
    mutatePathParams: {
      team_id: teamId,
    },
    successToast: "Access token created.",
  });
}
