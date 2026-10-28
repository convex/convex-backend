import type { RoleStatementAction } from "@convex-dev/platform/managementApi";
import { useCurrentTeam } from "api/teams";
import { type ConcreteResource, evaluateRoles } from "lib/permissions";
import {
  useBBMutation,
  useBBQuery,
  useManagementApiMutation,
  useManagementApiQuery,
} from "./api";
import { useCurrentDeployment } from "./deployments";
import { useProfile } from "./profile";
import { useCurrentProject } from "./projects";

export function useIsCurrentMemberTeamAdmin(): boolean {
  const team = useCurrentTeam();
  const myRoles = useMyCustomRoles(team?.id);
  return myRoles?.role === "admin";
}

export function useHasProjectAdminPermissions(projectId?: number): boolean {
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const { projectRoles } = useProjectRoles();
  const filteredProjectRoles = projectRoles?.filter(
    (r) => r.projectId === projectId,
  );
  const profile = useProfile();
  if (isTeamAdmin) {
    return true;
  }

  return (
    filteredProjectRoles?.some(
      (role) => role.role === "admin" && role.memberId === profile?.id,
    ) ?? false
  );
}

export function useProjectRoles() {
  const team = useCurrentTeam();
  const teamId = team?.id;
  const { data, isLoading } = useBBQuery({
    path: `/teams/{team_id}/get_project_roles`,
    pathParams: {
      team_id: teamId !== undefined ? teamId.toString() : "",
    },
  });
  return { isLoading, projectRoles: data };
}

export function useCurrentProjectRoles() {
  const project = useCurrentProject();
  const projectRoles = useProjectRoles();
  return (
    project &&
    (projectRoles.projectRoles?.filter(
      (role) => role.projectId === project.id,
    ) ||
      [])
  );
}

export function useUpdateProjectRoles(teamId?: number) {
  return useBBMutation({
    path: `/teams/{team_id}/update_project_roles`,
    pathParams: {
      team_id: teamId?.toString() || "",
    },
    mutateKey: `/teams/{team_id}/get_project_roles`,
    mutatePathParams: {
      team_id: teamId?.toString() || "",
    },
    successToast: "Project roles updated.",
  });
}

export function useMyCustomRoles(teamId: number | undefined) {
  const { data } = useBBQuery({
    path: `/teams/{team_id}/list_my_custom_roles`,
    pathParams: {
      team_id: teamId === undefined ? "" : teamId.toString(),
    },
    swrOptions: { refreshInterval: 5000, revalidateOnFocus: true },
  });
  if (teamId === undefined) return undefined;
  return data;
}

/**
 * Returns whether the current member is allowed to perform `action` on
 * `resource` for the given team. `undefined` while the role list is loading;
 * otherwise the result of `evaluateRoles` for custom-role members, or
 * `nonCustomRoleResult` for any built-in (admin/developer) member.
 *
 * Built-in admin/developer members have no custom-role statements
 * (`useMyCustomRoles` returns `customRoles: []` for them), so this must
 * short-circuit on `role !== "custom"` — evaluating against an empty
 * statement list would always deny.
 *
 * `nonCustomRoleResult` controls how built-in members are treated. Pick
 * based on the action's semantics:
 *
 * - `true` for read/view gates (any built-in member can see the data).
 * - `false` for mutation gates where the built-in role permission is
 *   checked separately at the call site (e.g. combined with
 *   `useIsCurrentMemberTeamAdmin()`):
 *
 *     const isTeamAdmin = useIsCurrentMemberTeamAdmin();
 *     const canCustom = useHasCustomRolePermission(
 *       teamId,
 *       "billing:subscription:changePlan",
 *       BILLING_RESOURCE,
 *       false,
 *     );
 *     const canChangePlan = isTeamAdmin || canCustom === true;
 *
 * `teamId`, `action`, and `resource` may all be `undefined`; in that case
 * the hook returns `undefined` without consulting the role list. This lets
 * callers thread a permission gate through optional config (e.g.
 * `useBBQuery`'s `permission` argument) without conditional hook calls.
 */
export function useHasCustomRolePermission(
  teamId: number | undefined,
  action: RoleStatementAction | undefined,
  resource: ConcreteResource | undefined,
  nonCustomRoleResult: boolean,
): boolean | undefined {
  const myRoles = useMyCustomRoles(teamId);
  const profile = useProfile();
  const deployment = useCurrentDeployment();
  if (deployment?.kind === "local" && action?.startsWith("deployment:")) {
    // Role permissions don't apply to local deployments
    return true;
  }

  if (action === undefined || resource === undefined) return undefined;
  if (myRoles === undefined) return undefined;
  if (myRoles.role !== "custom") return nonCustomRoleResult;
  // Wait for the profile so `creator=self` selectors can resolve; without
  // it those rules would silently deny and a gated UI would flicker.
  if (profile === undefined) return undefined;
  return (
    evaluateRoles(myRoles.customRoles, action, resource, profile.id) ===
    "allowed"
  );
}

export function useListCustomRoles(teamId?: number) {
  return useManagementApiQuery({
    path: `/teams/{team_id}/list_custom_roles`,
    pathParams: {
      team_id: teamId ?? 0,
    },
  });
}

export function useCreateCustomRole(teamId?: number) {
  return useManagementApiMutation({
    path: `/teams/{team_id}/create_custom_role`,
    pathParams: {
      team_id: teamId ?? 0,
    },
    mutateKey: `/teams/{team_id}/list_custom_roles`,
    mutatePathParams: {
      team_id: teamId ?? 0,
    },
    toastOnError: false,
  });
}

export function useUpdateCustomRole(teamId?: number) {
  return useManagementApiMutation({
    path: `/teams/{team_id}/update_custom_role`,
    pathParams: {
      team_id: teamId ?? 0,
    },
    mutateKey: `/teams/{team_id}/list_custom_roles`,
    mutatePathParams: {
      team_id: teamId ?? 0,
    },
    toastOnError: false,
  });
}

export function useDeleteCustomRole(teamId?: number) {
  return useManagementApiMutation({
    path: `/teams/{team_id}/delete_custom_role`,
    pathParams: {
      team_id: teamId ?? 0,
    },
    mutateKey: `/teams/{team_id}/list_custom_roles`,
    mutatePathParams: {
      team_id: teamId ?? 0,
    },
    successToast: "Custom role deleted.",
    // The delete confirmation dialog renders the failure inline.
    toastOnError: false,
  });
}

export function useUpdateTeamMemberRole(teamId: number) {
  return useManagementApiMutation({
    path: `/teams/{team_id}/update_team_member_role`,
    pathParams: {
      team_id: teamId,
    },
    mutateKey: `/teams/{team_id}/list_members`,
    mutatePathParams: {
      team_id: teamId.toString(),
    },
    successToast: "Member role updated.",
  });
}
