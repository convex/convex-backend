import { useCallback, useMemo } from "react";
import type { DirectorySyncResponse } from "generatedApi";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import {
  useBBMutation,
  useBBQuery,
  useMutate,
  useMutateManagementApi,
} from "./api";

const OFFERS_PATH = "/member/directory_sync_offers";
const DIRECTORY_SYNC_PATH = "/teams/{team_id}/directory_sync";
const GROUPS_PATH = "/teams/{team_id}/directory_sync/groups";
const MAPPING_PATH =
  "/teams/{team_id}/directory_sync/mappings/{workos_group_id}";

export const DIRECTORY_GROUPS_PAGE_SIZE = 25;
const STAGED_MEMBERS_PATH = "/teams/{team_id}/directory_sync/staged_members";
export const STAGED_MEMBERS_PAGE_SIZE = 50;

/**
 * Invalidates the team-members roster, which the management API serves and a
 * Big Brain mutation therefore doesn't touch. Anything that moves what the
 * directory confers can re-role a member or take them off the team.
 */
function useMutateTeamMembers(teamId: number) {
  const mutateMgmt = useMutateManagementApi();
  return useCallback(
    () =>
      mutateMgmt([
        "/teams/{team_id}/list_members",
        { params: { path: { team_id: teamId.toString() } } },
      ] as any),
    [mutateMgmt, teamId],
  );
}

export function useGetDirectorySync(
  teamId: number | undefined,
  {
    isPaused = false,
    refreshInterval,
  }: {
    isPaused?: boolean;
    refreshInterval?:
      | number
      | ((latest: DirectorySyncResponse | undefined) => number);
  } = {},
) {
  const { data, isLoading, error } = useBBQuery({
    path: DIRECTORY_SYNC_PATH,
    pathParams: {
      team_id: isPaused ? "" : (teamId?.toString() ?? ""),
    },
    swrOptions: { refreshInterval },
  });
  return { data, isLoading, error };
}

export function useGenerateDirectorySyncConfigurationLink(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/directory_sync/portal_link",
    pathParams: {
      team_id: teamId.toString(),
    },
  });
}

export function useDisableDirectorySync(teamId: number) {
  const disable = useBBMutation({
    path: "/teams/{team_id}/directory_sync/disable",
    pathParams: {
      team_id: teamId.toString(),
    },
    mutateKey: DIRECTORY_SYNC_PATH,
    mutatePathParams: {
      team_id: teamId.toString(),
    },
    successToast: "Directory Sync has been disabled for your team.",
    // The disable confirmation dialog renders the failure inline.
    toastOnError: false,
  });
  const mutate = useMutate();
  return useCallback(async () => {
    const result = await disable();
    await mutate([GROUPS_PATH]);
    await mutate([STAGED_MEMBERS_PATH]);
    return result;
  }, [disable, mutate]);
}

export function useEnableDirectorySync(teamId: number) {
  const enable = useBBMutation({
    path: "/teams/{team_id}/directory_sync/enable",
    pathParams: {
      team_id: teamId.toString(),
    },
    mutateKey: DIRECTORY_SYNC_PATH,
    mutatePathParams: {
      team_id: teamId.toString(),
    },
    successToast: "Directory Sync has been enabled for your team.",
    // The review step renders the failure inline.
    toastOnError: false,
  });
  const mutate = useMutate();
  const mutateTeamMembers = useMutateTeamMembers(teamId);
  return useCallback(async () => {
    const result = await enable();
    // Once management is on the roster only lists who can still join, so
    // the staged list has to be refetched too.
    await mutate([STAGED_MEMBERS_PATH]);
    await mutateTeamMembers();
    return result;
  }, [enable, mutate, mutateTeamMembers]);
}

export function useStagedDirectoryMembers(
  teamId: number | undefined,
  cursor: string | undefined,
  { isPaused = false }: { isPaused?: boolean } = {},
) {
  const queryParams = useMemo(
    () => ({ cursor, limit: STAGED_MEMBERS_PAGE_SIZE }),
    [cursor],
  );
  const { data, isLoading, error } = useBBQuery({
    path: STAGED_MEMBERS_PATH,
    pathParams: {
      team_id: isPaused ? "" : (teamId?.toString() ?? ""),
    },
    queryParams,
  });
  // The spec declares no error body for this route, so SWR types the error
  // as `never`; the roster still 404s while the directory is unconfigured.
  // Network failures arrive as an `Error`, which carries no `code`.
  return {
    data,
    isLoading,
    error: error as { code?: string; message?: string } | undefined,
  };
}

export function useDirectorySyncGroups(
  teamId: number | undefined,
  cursor: string | undefined,
  { isPaused = false }: { isPaused?: boolean } = {},
) {
  const queryParams = useMemo(
    () => ({ cursor, limit: DIRECTORY_GROUPS_PAGE_SIZE }),
    [cursor],
  );
  const { data, isLoading, error } = useBBQuery({
    path: GROUPS_PATH,
    pathParams: {
      team_id: isPaused ? "" : (teamId?.toString() ?? ""),
    },
    queryParams,
  });
  return { data, isLoading, error };
}

export function useSetGroupRoleMapping(teamId: number, workosGroupId: string) {
  const setMapping = useBBMutation({
    method: "put",
    path: MAPPING_PATH,
    pathParams: {
      team_id: teamId,
      workos_group_id: workosGroupId,
    },
    mutateKey: GROUPS_PATH,
    mutatePathParams: {
      team_id: teamId.toString(),
    },
    successToast: "Group role updated.",
  });
  const mutateTeamMembers = useMutateTeamMembers(teamId);
  return useCallback(
    async (
      body: { role: "admin" | "developer" } | { customRoles: number[] },
    ) => {
      const result = await setMapping(body);
      await mutateTeamMembers();
      return result;
    },
    [setMapping, mutateTeamMembers],
  );
}

export function useDeleteGroupRoleMapping(
  teamId: number,
  workosGroupId: string,
) {
  const deleteMapping = useBBMutation({
    method: "delete",
    path: MAPPING_PATH,
    pathParams: {
      team_id: teamId,
      workos_group_id: workosGroupId,
    },
    mutateKey: GROUPS_PATH,
    mutatePathParams: {
      team_id: teamId.toString(),
    },
    successToast: "Group role updated.",
  });
  const mutateTeamMembers = useMutateTeamMembers(teamId);
  return useCallback(async () => {
    const result = await deleteMapping();
    await mutateTeamMembers();
    return result;
  }, [deleteMapping, mutateTeamMembers]);
}

// Teams whose directory roster lists one of the caller's verified emails and
// that they aren't a member of yet, so they can join without an invitation.
// Undefined until directory sync is rolled out to the member.
export function useDirectorySyncOffers() {
  const { directorySync } = useLaunchDarkly();
  const { data } = useBBQuery({
    path: OFFERS_PATH,
    pathParams: undefined,
    swrOptions: {
      isPaused: () => !directorySync,
      // The roster behind these offers changes rarely, and they only surface
      // in the team switcher, so don't chase window focus and collapse the
      // repeat mounts into at most one request a minute.
      revalidateOnFocus: false,
      dedupingInterval: 1000 * 60,
    },
  });
  // Pausing stops new requests but leaves whatever SWR already cached, so the
  // flag has to gate the result too — otherwise turning it off mid-session
  // keeps offering teams the member should no longer be able to join.
  return directorySync ? data?.offers : undefined;
}

export function useJoinDirectorySyncedTeam() {
  const join = useBBMutation({
    path: "/member/directory_sync/join",
    pathParams: undefined,
    mutateKey: "/teams",
    successToast: "Joined team.",
  });
  const mutate = useMutate();
  return useCallback(
    async (body: { proposedTeamId: number }) => {
      const result = await join(body);
      // `mutateKey` only covers the teams list; the accepted offer also has to
      // disappear from the offers list.
      await mutate([OFFERS_PATH]);
      return result;
    },
    [join, mutate],
  );
}
