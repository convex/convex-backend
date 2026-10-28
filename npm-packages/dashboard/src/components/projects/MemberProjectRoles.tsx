import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Sheet } from "@ui/Sheet";
import { LoadingTransition } from "@ui/Loading";
import { useCurrentProject } from "api/projects";
import { useCurrentTeam, useTeamMembers } from "api/teams";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
  useMyCustomRoles,
  useUpdateProjectRoles,
  useCurrentProjectRoles,
} from "api/roles";
import { TeamMember } from "@convex-dev/platform/managementApi";
import { Link } from "@ui/Link";
import { useState } from "react";
import sortBy from "lodash/sortBy";
import { MEMBER_RESOURCE, projectResource } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { ProjectAdminFormModal } from "./ProjectAdminsFormModal";

export function MemberProjectRoles() {
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const projectRoles = useCurrentProjectRoles();

  const [showProjectAdminsModal, setShowProjectAdminsModal] = useState(false);

  const hasAdminPermissions = useHasProjectAdminPermissions(project?.id);
  const canUpdateMemberRoleCustom = useHasCustomRolePermission(
    team?.id,
    "project:updateMemberRole",
    project ? projectResource(project) : undefined,
    false,
  );

  const myRoles = useMyCustomRoles(team?.id);
  const canViewMembersCustom = useHasCustomRolePermission(
    team?.id,
    "member:view",
    MEMBER_RESOURCE,
    true,
  );
  const isCustomRoleMember = myRoles?.role === "custom";
  const limitedToOwnRole = isCustomRoleMember && canViewMembersCustom === false;
  const fetchedMembers = useTeamMembers(
    limitedToOwnRole ? undefined : team?.id,
  );
  const members = limitedToOwnRole ? [] : fetchedMembers;

  // Managing project admins requires both write permission AND the ability
  // to view members — the form lets you toggle a project-admin badge per
  // member, which doesn't work if the member list itself is blocked.
  const canManageProjectAdmins =
    (hasAdminPermissions || canUpdateMemberRoleCustom === true) &&
    !limitedToOwnRole;

  const membersWithProjectAccess = members
    ?.map((member) => {
      const memberRole = projectRoles?.find(
        (role) => role.memberId === member.id,
      );
      return {
        ...member,
        isProjectAdmin: !!memberRole,
      };
    })
    .filter(
      (member): member is TeamMember & { isProjectAdmin: boolean } =>
        member.isProjectAdmin || member.role === "admin",
    );

  const updateProjectRoles = useUpdateProjectRoles(team?.id);

  return (
    <Sheet className="flex flex-col gap-2 text-sm">
      <div className="flex justify-between">
        <h3>Project Admins</h3>{" "}
        <Button
          className="ml-auto w-fit"
          disabled={!canManageProjectAdmins}
          tip={
            !canManageProjectAdmins
              ? limitedToOwnRole
                ? permissionDeniedTip(
                    "You need permission to view members to manage project admins.",
                    "member:view",
                  )
                : permissionDeniedTip(
                    "You do not have permission to manage project admins.",
                    "project:updateMemberRole",
                  )
              : undefined
          }
          onClick={() => setShowProjectAdminsModal(true)}
        >
          Manage Project Admins
        </Button>
      </div>
      <p>These team members have administrative access to this project. </p>
      <p className="max-w-prose">
        All other team members may create dev and preview deployments, and have
        read-only access to production data.{" "}
        <Link
          href="https://docs.convex.dev/dashboard/teams#roles-and-permissions"
          target="_blank"
        >
          Learn more
        </Link>{" "}
        about project permissions.
      </p>
      <LoadingTransition>
        {membersWithProjectAccess && (
          <div className="flex w-full flex-col">
            {sortBy(membersWithProjectAccess, (member) =>
              (member.name || member.email).toLocaleLowerCase(),
            ).map((member, idx) => (
              <div
                className="flex items-center justify-between border-b py-2 last:border-b-0"
                key={idx}
              >
                <div className="flex flex-col truncate">
                  {member.name && (
                    <div className="text-sm text-content-primary">
                      {member.name}
                    </div>
                  )}
                  <div
                    className={`truncate ${
                      member.name
                        ? "text-xs text-content-secondary"
                        : "text-sm text-content-primary"
                    }`}
                  >
                    {member.email}
                  </div>
                </div>
                <div className="flex gap-1">
                  {member.role === "admin" && (
                    <Tooltip
                      tip={
                        <div className="flex flex-col">
                          <p>
                            This member can manage all projects in the team
                            because they are a team admin.
                          </p>{" "}
                          <p>
                            You may view and manage team admins on the{" "}
                            <Link href={`/t/${team?.slug}/settings/members`}>
                              member settings
                            </Link>{" "}
                            page.
                          </p>
                        </div>
                      }
                    >
                      <div className="rounded-sm border p-1 text-xs">
                        Team Admin
                      </div>
                    </Tooltip>
                  )}
                  {member.isProjectAdmin && (
                    <Tooltip tip="This member can manage this project because they are a project admin.">
                      <div className="rounded-sm border p-1 text-xs">
                        Project Admin
                      </div>
                    </Tooltip>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </LoadingTransition>
      {limitedToOwnRole && (
        <p className="text-xs text-content-secondary">
          You can only see your own role here because your custom role does not
          grant the <code className="font-mono">member:view</code> permission.
        </p>
      )}
      {showProjectAdminsModal &&
        team &&
        members &&
        project &&
        projectRoles !== undefined && (
          <ProjectAdminFormModal
            project={project}
            members={members}
            projectRoles={projectRoles}
            onUpdateProjectRoles={async (args) => {
              await updateProjectRoles(args);
            }}
            onClose={() => setShowProjectAdminsModal(false)}
          />
        )}
    </Sheet>
  );
}
