import { TeamResponse } from "generatedApi";
import {
  InvitationResponse,
  TeamMember,
} from "@convex-dev/platform/managementApi";

import { Sheet } from "@ui/Sheet";
import { LoadingTransition } from "@ui/Loading";
import { TextInput } from "@ui/TextInput";
import { useState } from "react";
import {
  useUpdateTeamMemberRole,
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
  useListCustomRoles,
  useProjectRoles,
  useUpdateProjectRoles,
} from "api/roles";
import { MEMBER_RESOURCE } from "lib/permissions";
import { useRemoveTeamMember, useTeamEntitlements } from "api/teams";
import { useProfile } from "api/profile";
import { useCancelInvite, useCreateInvite } from "api/invitations";
import sortBy from "lodash/sortBy";
import { TeamMemberInviteListItem } from "./TeamMemberInviteListItem";
import { TeamMemberListItem } from "./TeamMemberListItem";
import { TeamMemberListSkeleton } from "./TeamMemberListSkeleton";

type TeamMemberListProps = {
  team: TeamResponse;
  members?: TeamMember[];
  invites?: InvitationResponse[];
};

export function TeamMemberList({
  team,
  members,
  invites,
}: TeamMemberListProps) {
  const [memberSearch, setMemberSearch] = useState("");
  const [inviteSearch, setInviteSearch] = useState("");
  const filteredMembers = members?.filter(
    (member) =>
      member.name?.toLowerCase().includes(memberSearch.toLowerCase()) ||
      member.email.toLowerCase().includes(memberSearch.toLowerCase()),
  );
  const profile = useProfile();
  const me = filteredMembers?.find((member) => member.id === profile?.id);
  const filteredMembersWithoutMe = filteredMembers?.filter(
    (member) => member.id !== profile?.id,
  );

  const filteredInvites = invites?.filter((invite) =>
    invite.email.toLowerCase().includes(inviteSearch.toLowerCase()),
  );

  const onChangeRole = useUpdateTeamMemberRole(team.id);
  const onRemoveMember = useRemoveTeamMember(team.id);
  const onCreateInvite = useCreateInvite(team.id);
  const onCancelInvite = useCancelInvite(team.id);

  // All member-management actions are admin-only by default; custom roles
  // need an explicit grant per-action.
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canUpdateRoleCustom = useHasCustomRolePermission(
    team.id,
    "member:updateRole",
    MEMBER_RESOURCE,
    false,
  );
  const canRemoveMemberCustom = useHasCustomRolePermission(
    team.id,
    "member:remove",
    MEMBER_RESOURCE,
    false,
  );
  const canInviteCustom = useHasCustomRolePermission(
    team.id,
    "member:invite",
    MEMBER_RESOURCE,
    false,
  );
  const canCancelInviteCustom = useHasCustomRolePermission(
    team.id,
    "member:cancelInvitation",
    MEMBER_RESOURCE,
    false,
  );
  const canUpdateRole = isTeamAdmin || canUpdateRoleCustom;
  const canRemoveMember = isTeamAdmin || canRemoveMemberCustom;
  const canInvite = isTeamAdmin || canInviteCustom;
  const canCancelInvite = isTeamAdmin || canCancelInviteCustom;

  const entitlements = useTeamEntitlements(team.id);
  const customRolesEnabled = entitlements?.customRolesEnabled ?? false;
  const { data: customRolesData } = useListCustomRoles(
    customRolesEnabled ? team.id : undefined,
  );
  const customRoles = customRolesData?.items ?? [];

  const { projectRoles } = useProjectRoles();

  const updateProjectRoles = useUpdateProjectRoles(team.id);

  return (
    <>
      <Sheet className="min-w-[20rem]">
        <div className="mb-4 flex w-full items-center justify-between gap-2">
          <h3 className="grow">Team Members</h3>
          <div className="w-48">
            <TextInput
              type="search"
              id="memberSearch"
              value={memberSearch}
              onChange={(e) => setMemberSearch(e.target.value)}
              placeholder="Search members"
            />
          </div>
        </div>
        <LoadingTransition>
          {profile && members && projectRoles && (
            <div className="flex w-full flex-col">
              {/* Always show self at the top */}
              {me && (
                <TeamMemberListItem
                  team={team}
                  member={me}
                  members={members}
                  canChangeRole={false}
                  myProfile={profile}
                  customRoles={customRoles}
                  customRolesEnabled={customRolesEnabled}
                  onChangeRole={onChangeRole}
                  onRemoveMember={onRemoveMember}
                  canUpdateRole={canUpdateRole}
                  canRemoveMember={canRemoveMember}
                  projectRoles={projectRoles?.filter(
                    (role) => role.memberId === me.id,
                  )}
                  onUpdateProjectRoles={updateProjectRoles}
                />
              )}
              {!filteredMembersWithoutMe ? (
                <TeamMemberListSkeleton />
              ) : filteredMembers && filteredMembers.length === 0 ? (
                <div className="my-4 flex w-full justify-center text-content-secondary">
                  No members match your search.
                </div>
              ) : (
                sortBy(filteredMembersWithoutMe, (member) =>
                  (member.name || member.email).toLocaleLowerCase(),
                ).map((member) => (
                  <TeamMemberListItem
                    key={`member${member.id}`}
                    team={team}
                    member={member}
                    members={members}
                    canChangeRole
                    myProfile={profile}
                    customRoles={customRoles}
                    customRolesEnabled={customRolesEnabled}
                    onChangeRole={onChangeRole}
                    onRemoveMember={onRemoveMember}
                    canUpdateRole={canUpdateRole}
                    canRemoveMember={canRemoveMember}
                    projectRoles={projectRoles?.filter(
                      (role) => role.memberId === member.id,
                    )}
                    onUpdateProjectRoles={updateProjectRoles}
                  />
                ))
              )}
            </div>
          )}
        </LoadingTransition>
      </Sheet>
      {invites && invites.length > 0 && (
        <Sheet>
          <div className="mb-4 flex w-full items-center justify-between gap-2">
            <h3 className="grow">Pending Invitations</h3>
            <div className="w-48">
              <TextInput
                type="search"
                id="inviteSearch"
                value={inviteSearch}
                onChange={(e) => setInviteSearch(e.target.value)}
                placeholder="Search invitations"
              />
            </div>
          </div>
          <div className="flex flex-col">
            {!filteredInvites ? (
              <TeamMemberListSkeleton />
            ) : filteredInvites.length === 0 ? (
              <div className="my-4 flex w-full justify-center text-content-secondary">
                No invites match your search.
              </div>
            ) : (
              filteredInvites.map((invite, idx) => (
                <TeamMemberInviteListItem
                  key={`invite${idx}`}
                  invite={invite}
                  teamSlug={team.slug}
                  canInvite={canInvite}
                  canCancelInvite={canCancelInvite}
                  onCreateInvite={onCreateInvite}
                  onCancelInvite={onCancelInvite}
                />
              ))
            )}
          </div>
        </Sheet>
      )}
    </>
  );
}
