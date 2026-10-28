import {
  useBBMutation,
  useManagementApiMutation,
  useManagementApiQuery,
} from "./api";

export function useTeamInvites(teamId: number) {
  const { data } = useManagementApiQuery({
    path: "/teams/{team_id}/list_pending_invites",
    pathParams: {
      team_id: teamId.toString(),
    },
  });
  return data?.items;
}

export function useCreateInvite(teamId: number) {
  return useManagementApiMutation({
    path: "/teams/{team_id}/invite_team_member",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/list_pending_invites",
    mutatePathParams: { team_id: teamId.toString() },
    successToast: "Invitation sent.",
  });
}

export function useCancelInvite(teamId: number) {
  return useManagementApiMutation({
    path: "/teams/{team_id}/cancel_team_member_invite",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/list_pending_invites",
    mutatePathParams: { team_id: teamId.toString() },
    successToast: "Invitation revoked.",
  });
}

export function useAcceptInvite(code: string) {
  return useBBMutation({
    path: "/invites/{code}/accept",
    pathParams: { code },
    toastOnError: false,
  });
}
