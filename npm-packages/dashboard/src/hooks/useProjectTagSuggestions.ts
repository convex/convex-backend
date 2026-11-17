import { useTeamProjectTags } from "api/projects";
import { useHasCustomRolePermission } from "api/roles";
import { TEAM_RESOURCE } from "lib/permissions";

/**
 * The team's existing project tags to suggest while editing a project's tags,
 * or an empty list while loading or if the member can't list them. Pass an
 * undefined `teamId` to skip the request.
 */
export function useProjectTagSuggestions(teamId: number | undefined): string[] {
  const canListTags = useHasCustomRolePermission(
    teamId,
    "team:projectTags:view",
    TEAM_RESOURCE,
    true,
  );
  return useTeamProjectTags(canListTags ? teamId : undefined) ?? [];
}
