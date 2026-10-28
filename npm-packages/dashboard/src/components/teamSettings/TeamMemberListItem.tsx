import type {
  MemberResponse,
  ProjectMemberRoleResponse,
  UpdateProjectRolesArgs,
  TeamResponse,
} from "generatedApi";
import type {
  CustomRoleResponse,
  TeamMember,
} from "@convex-dev/platform/managementApi";
import { useRouter } from "next/router";
import { useRef, useState, type ReactNode } from "react";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Menu, MenuItem } from "@ui/Menu";
import { Callout } from "@ui/Callout";
import { DotsVerticalIcon } from "@radix-ui/react-icons";
import { useMount } from "react-use";
import classNames from "classnames";
import startCase from "lodash/startCase";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { MemberProjectRolesModal } from "./MemberProjectRolesModal";
import { EditTeamRoleDialog } from "./EditTeamRoleDialog";
import { RoleDisplay } from "./RoleDisplay";

type TeamMemberListItemProps = {
  team: TeamResponse;
  myProfile: MemberResponse;
  member: TeamMember;
  members: TeamMember[];
  canChangeRole: boolean;
  customRoles: CustomRoleResponse[];
  customRolesEnabled: boolean;
  onChangeRole: (body: {
    memberId: number;
    role?: "admin" | "developer";
    customRoles?: number[];
  }) => Promise<unknown>;
  onRemoveMember: (body: { memberId: number }) => Promise<Response>;
  onUpdateProjectRoles: (body: UpdateProjectRolesArgs) => Promise<undefined>;
  // Action-specific gates; `undefined` while permissions are loading.
  canUpdateRole: boolean | undefined;
  canRemoveMember: boolean | undefined;
  projectRoles: ProjectMemberRoleResponse[];
};
export function TeamMemberListItem({
  team,
  myProfile,
  member,
  members,
  canChangeRole,
  customRoles,
  customRolesEnabled,
  onChangeRole,
  onUpdateProjectRoles,
  onRemoveMember,
  canUpdateRole,
  canRemoveMember,
  projectRoles,
}: TeamMemberListItemProps) {
  const router = useRouter();
  const isMemberTheLastAdmin =
    members.filter((m) => m.role === "admin" && m.id !== member.id).length ===
    0;
  const isMemberMe = member.id === myProfile?.id;

  // Members can always remove themselves (leave the team) regardless of the
  // `member:remove` gate, as long as they're not the last admin.
  const canManageMember =
    (canRemoveMember === true || isMemberMe) && !isMemberTheLastAdmin;

  const isHighlighted = window.location.hash === `#${member.id}`;

  const ref = useRef<HTMLDivElement | null>(null);
  useMount(() => {
    if (isHighlighted) {
      ref.current?.scrollIntoView();
    }
  });

  let removeMemberDisabledReason: ReactNode | undefined;
  if (isMemberTheLastAdmin) {
    removeMemberDisabledReason =
      "You cannot remove the last admin from this team. Contact us for help at support@convex.dev";
  } else if (!canManageMember) {
    removeMemberDisabledReason = permissionDeniedTip(
      "You do not have permission to remove members from this team.",
      "member:remove",
    );
  }

  let updateRoleDisabledReason: ReactNode | undefined;
  if (!canChangeRole) {
    updateRoleDisabledReason =
      "You cannot change your own team role. Ask another admin to do it for you.";
  } else if (team.managedBy === "vercel") {
    updateRoleDisabledReason = `This team is managed by ${startCase(team.managedBy)}. You may manage team roles in ${startCase(team.managedBy)}.`;
  } else if (isMemberTheLastAdmin) {
    updateRoleDisabledReason = "You cannot change the role of the last admin.";
  } else if (canUpdateRole !== true) {
    updateRoleDisabledReason = permissionDeniedTip(
      "You do not have permission to change member roles.",
      "member:updateRole",
    );
  }
  const canEditTeamRole = updateRoleDisabledReason === undefined;

  const [showRemoveMember, setShowRemoveMember] = useState(false);
  const [showEditRole, setShowEditRole] = useState(false);
  const [showProjectRolesModal, setShowProjectRolesModal] = useState(false);

  const confirmationDisplayName = member.name
    ? `${member.name} (${member.email})`
    : member.email;

  return (
    <div
      ref={ref}
      className={classNames(
        "flex flex-wrap justify-between items-center gap-4 py-2",
        isHighlighted
          ? "bg-highlight px-2 -mx-2 rounded-sm border"
          : "border-b last:border-b-0",
      )}
    >
      <div className="flex max-w-[40%] flex-col sm:max-w-[50%] md:max-w-[80%]">
        {member.name && (
          <div className="text-sm text-content-primary">{member.name}</div>
        )}
        <div
          className={`${
            member.name
              ? "text-xs text-content-secondary"
              : "text-sm text-content-primary"
          }`}
        >
          {member.email}
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <RoleDisplay
          role={member.role}
          customRoles={member.customRoles}
          teamSlug={team.slug}
        />
        <Menu
          placement="bottom-end"
          buttonProps={{
            variant: "neutral",
            size: "xs",
            icon: <DotsVerticalIcon />,
            "aria-label": "Member options",
          }}
        >
          <MenuItem
            disabled={!canEditTeamRole}
            tip={updateRoleDisabledReason}
            tipSide="left"
            action={() => {
              if (canEditTeamRole) setShowEditRole(true);
            }}
          >
            Edit team role
          </MenuItem>
          <MenuItem action={() => setShowProjectRolesModal(true)}>
            Edit project roles
          </MenuItem>
          <MenuItem
            variant="danger"
            disabled={!canManageMember}
            tip={removeMemberDisabledReason}
            tipSide="left"
            action={() => {
              if (canManageMember) setShowRemoveMember(true);
            }}
          >
            {isMemberMe ? "Leave team" : "Remove member"}
          </MenuItem>
        </Menu>
        {showRemoveMember && (
          <ConfirmationDialog
            onClose={() => setShowRemoveMember(false)}
            onConfirm={async () => {
              await onRemoveMember({ memberId: member.id });
              if (isMemberMe) {
                await router.push("/");
              }
            }}
            dialogTitle={isMemberMe ? "Leave team" : "Remove team member"}
            dialogBody={
              isMemberMe ? (
                `You are about to leave ${team.name}, are you sure you want to continue?`
              ) : (
                <div className="flex flex-col gap-1">
                  <p>
                    You are about to remove {confirmationDisplayName} from{" "}
                    {team.name}, are you sure you want to continue?{" "}
                  </p>
                  {team.managedBy === "vercel" && (
                    <Callout>
                      Note that this member may be able to re-join the team
                      through the {startCase(team.managedBy)} dashboard if they
                      are still a member of your {startCase(team.managedBy)}{" "}
                      team.
                    </Callout>
                  )}
                </div>
              )
            }
            confirmText="Confirm"
          />
        )}
        {showEditRole && (
          <EditTeamRoleDialog
            team={team}
            member={member}
            customRoles={customRoles}
            customRolesEnabled={customRolesEnabled}
            onSave={onChangeRole}
            onClose={() => setShowEditRole(false)}
          />
        )}
        {showProjectRolesModal && (
          <MemberProjectRolesModal
            member={member}
            team={team}
            projectRoles={projectRoles}
            onClose={() => setShowProjectRolesModal(false)}
            onUpdateProjectRoles={onUpdateProjectRoles}
          />
        )}
      </div>
    </div>
  );
}
