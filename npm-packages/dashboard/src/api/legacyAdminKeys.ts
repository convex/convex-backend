import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { useBBMutation, useBBQuery } from "./api";
import { useIsCurrentMemberTeamAdmin } from "./roles";

export const LEGACY_ADMIN_KEYS_SECTION_ID = "legacy-admin-keys";

export const RECENT_LEGACY_ADMIN_KEY_USE_MS = 30 * 24 * 60 * 60 * 1000;

const MAX_LISTED_DEPLOYMENTS = 100;

export function formatDeploymentCount(count: number): string {
  if (count >= MAX_LISTED_DEPLOYMENTS) {
    return `${MAX_LISTED_DEPLOYMENTS}+ deployments`;
  }
  return count === 1 ? "1 deployment" : `${count} deployments`;
}

export function useShowLegacyAdminKeyUsage(): boolean {
  const { showLegacyKeyUsage } = useLaunchDarkly();
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  return showLegacyKeyUsage && isTeamAdmin;
}

export function useTeamLegacyAdminKeyUsage(teamId: number | undefined) {
  const { data } = useBBQuery({
    path: "/teams/{team_id}/legacy_admin_key_usage",
    pathParams: {
      team_id: teamId?.toString() ?? "",
    },
  });
  return data;
}

export function useDisableTeamLegacyAdminKeys(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/disable_legacy_admin_keys",
    pathParams: {
      team_id: teamId.toString(),
    },
    successToast: "Legacy admin keys disabled.",
    mutateKey: "/teams/{team_id}/legacy_admin_key_usage",
    mutatePathParams: {
      team_id: teamId.toString(),
    },
  });
}
