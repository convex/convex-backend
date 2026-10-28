import type { TeamResponse } from "generatedApi";
import { useLastViewedTeam } from "hooks/useLastViewed";
import { useInitialData } from "hooks/useServerSideData";
import { useRouter } from "next/router";
import {
  useBBMutation,
  useBBQuery,
  useManagementApiQuery,
  useMutateManagementApi,
} from "./api";
import { useCallback } from "react";

export function useTeams(): {
  selectedTeamSlug?: string;
  teams?: TeamResponse[];
} {
  const [initialData] = useInitialData();
  const { data, isValidating } = useBBQuery({
    path: "/teams",
    pathParams: undefined,
    swrOptions: {
      revalidateOnMount: !initialData,
    },
  });

  const teams = data ?? (initialData?.["/teams"] as TeamResponse[] | undefined);
  const [lastViewedTeam] = useLastViewedTeam();
  const router = useRouter();

  const defaultSelectedTeamSlug =
    teams &&
    (teams.some((team) => team.slug === lastViewedTeam)
      ? lastViewedTeam
      : teams[0]
        ? teams[0].slug
        : undefined);

  const selectedTeamSlug =
    typeof router.query.team === "string"
      ? router.query.team
      : defaultSelectedTeamSlug;

  if (
    teams &&
    typeof router.query.team === "string" &&
    !teams?.some((team) => team.slug === router.query.team) &&
    !isValidating
  ) {
    void router.push("/404");
  }

  return { selectedTeamSlug, teams };
}

export function useCurrentTeam() {
  const { selectedTeamSlug, teams } = useTeams();
  const currentTeam =
    teams?.find((team) => team.slug === selectedTeamSlug) ?? undefined;
  const router = useRouter();
  if (currentTeam?.suspended) {
    void router.push("/suspended");
    return;
  }
  return currentTeam;
}

export function useTeamMembers(teamId: number | undefined) {
  const { data } = useManagementApiQuery({
    path: "/teams/{team_id}/list_members",
    pathParams: {
      team_id: teamId?.toString() ?? "",
    },
  });
  return data?.items;
}

export function useTeamEntitlements(teamId: number | undefined) {
  const { data: entitlements } = useBBQuery({
    path: "/teams/{team_id}/get_entitlements",
    pathParams: {
      team_id: teamId?.toString() || "",
    },
  });
  return entitlements;
}

export function useCreateTeam() {
  return useBBMutation({
    path: "/teams",
    pathParams: undefined,
    mutateKey: "/teams",
    successToast: "Team created.",
  });
}

export function useDeleteTeam(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/delete",
    pathParams: {
      team_id: teamId.toString(),
    },
    mutateKey: "/teams",
    successToast: "Team deleted.",
  });
}

export function useUpdateTeam(teamId: number, toast: boolean = true) {
  return useBBMutation({
    path: `/teams/{team_id}`,
    pathParams: {
      team_id: teamId.toString(),
    },
    mutateKey: "/teams",
    successToast: toast ? "Team settings updated." : undefined,
  });
}

export function useRemoveTeamMember(teamId: number) {
  const remove = useBBMutation({
    path: `/teams/{team_id}/remove_member`,
    pathParams: {
      team_id: teamId.toString(),
    },
    successToast: "Member removed from team.",
  });
  // The team-members list now comes from the management API, so we
  // invalidate that cache by hand after the BB mutation lands.
  const mutateMgmt = useMutateManagementApi();
  return useCallback(
    async (body: { memberId: number }) => {
      const result = await remove(body);
      await mutateMgmt([
        "/teams/{team_id}/list_members",
        { params: { path: { team_id: teamId.toString() } } },
      ] as any);
      return result;
    },
    [remove, mutateMgmt, teamId],
  );
}

export function useUnpauseTeam(teamId: number) {
  return useBBMutation({
    path: `/teams/{team_id}/unpause`,
    pathParams: {
      team_id: teamId.toString(),
    },
    mutateKey: `/teams/{team_id}/usage/team_usage_state`,
    successToast: "Your team has been restored.",
  });
}

export function useGetSSO(
  teamId: number | undefined,
  { isPaused = false }: { isPaused?: boolean } = {},
) {
  const {
    data: ssoOrganization,
    isLoading,
    error,
  } = useBBQuery({
    path: "/teams/{team_id}/get_sso",
    pathParams: {
      team_id: isPaused ? "" : (teamId?.toString() ?? ""),
    },
    swrOptions: {
      revalidateOnFocus: true,
    },
  });
  return { data: ssoOrganization, isLoading, error };
}

export function useDisableSSO(teamId: number) {
  return useBBMutation({
    path: `/teams/{team_id}/disable_sso`,
    pathParams: {
      team_id: teamId.toString(),
    },
    mutateKey: "/teams/{team_id}/get_sso",
    mutatePathParams: {
      team_id: teamId.toString(),
    },
    successToast: "SSO has been disabled for your team.",
    // The disable confirmation dialog renders the failure inline.
    toastOnError: false,
  });
}

export function useGenerateSSOConfigurationLink(teamId: number) {
  return useBBMutation({
    path: `/teams/{team_id}/generate_sso_configuration_link`,
    pathParams: {
      team_id: teamId.toString(),
    },
  });
}

export function useUpdateSSO(teamId: number) {
  return useBBMutation({
    path: `/teams/{team_id}/update_sso`,
    pathParams: {
      team_id: teamId.toString(),
    },
    mutateKey: "/teams/{team_id}/get_sso",
    mutatePathParams: {
      team_id: teamId.toString(),
    },
    successToast: "SSO settings updated.",
    // The confirmation dialog renders the failure inline.
    toastOnError: false,
  });
}

export function usePotentialVercelTeams() {
  const { data, error } = useBBQuery({
    path: "/vercel/potential_teams",
    pathParams: undefined,
  });
  return { data, error };
}

export function useJoinVercelTeam(proposedTeamId: number) {
  return useBBMutation({
    path: "/vercel/potential_teams/{proposed_team_id}/join",
    pathParams: { proposed_team_id: proposedTeamId.toString() },
    mutateKey: "/teams",
    successToast: "Joined team.",
  });
}
