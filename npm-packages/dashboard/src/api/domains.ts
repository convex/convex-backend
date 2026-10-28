import { useBBMutation, useBBQuery } from "./api";

const DOMAINS_PATH = "/teams/{team_id}/domains";

export function useTeamDomains(
  teamId: number | undefined,
  { isPaused = false }: { isPaused?: boolean } = {},
) {
  const { data, isLoading, error } = useBBQuery({
    path: DOMAINS_PATH,
    pathParams: {
      team_id: isPaused ? "" : (teamId?.toString() ?? ""),
    },
    swrOptions: {
      revalidateOnFocus: true,
    },
  });
  return { data: data?.domains, isLoading, error };
}

export function useDeleteTeamDomain(teamId: number, domainId: string) {
  return useBBMutation({
    method: "delete",
    path: "/teams/{team_id}/domains/{domain_id}",
    pathParams: {
      team_id: teamId,
      domain_id: domainId,
    },
    mutateKey: DOMAINS_PATH,
    mutatePathParams: {
      team_id: teamId.toString(),
    },
    successToast: "Domain deleted.",
    // The delete confirmation dialog renders the failure inline.
    toastOnError: false,
  });
}

export function useDomainPortalLink(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/domains/portal_link",
    pathParams: {
      team_id: teamId.toString(),
    },
  });
}
