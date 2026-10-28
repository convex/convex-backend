import { EnvelopeClosedIcon } from "@radix-ui/react-icons";
import { Callout } from "@ui/Callout";
import { Loading } from "@ui/Loading";
import { Sheet } from "@ui/Sheet";
import { useTeamMembers, useTeamEntitlements } from "api/teams";
import { useTeamInvites } from "api/invitations";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { MEMBER_RESOURCE } from "lib/permissions";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { Link } from "@ui/Link";
import { TeamResponse } from "generatedApi";
import startCase from "lodash/startCase";

import { captureMessage } from "@sentry/nextjs";
import { OpenInVercel } from "components/OpenInVercel";
import { InviteMemberForm } from "./InviteMemberForm";
import { TeamMemberList } from "./TeamMemberList";

export function TeamMembers({ team }: { team: TeamResponse }) {
  const canViewMembers = useHasCustomRolePermission(
    team.id,
    "member:view",
    MEMBER_RESOURCE,
    true,
  );
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canInviteCustom = useHasCustomRolePermission(
    team.id,
    "member:invite",
    MEMBER_RESOURCE,
    false,
  );
  // `member:invite` is admin-only by default; custom roles need an explicit
  // grant.
  const canInvite = isTeamAdmin || canInviteCustom;

  if (canViewMembers === false) {
    return (
      <>
        <h2>Members</h2>
        <NoPermissionMessage
          message="You do not have permission to view team members."
          missingPermission="member:view"
        />
      </>
    );
  }

  return <MembersContent team={team} canInvite={canInvite} />;
}

function MembersContent({
  team,
  canInvite,
}: {
  team: TeamResponse;
  canInvite: boolean | undefined;
}) {
  const members = useTeamMembers(team.id);
  const invites = useTeamInvites(team.id);
  const entitlements = useTeamEntitlements(team.id);

  const isLoading = !(members && invites && entitlements !== undefined);
  const canAddMembers =
    !isLoading && members.length < entitlements.maxTeamMembers!;

  let inviteMembers = null;
  if (isLoading) {
    // Data isn't loaded yet, show a skeleton.
    inviteMembers = (
      <Loading className="h-38 w-full rounded-sm" fullHeight={false} />
    );
  } else if (team.managedBy === "vercel") {
    inviteMembers = (
      <Sheet>
        <div className="flex items-center justify-between gap-4">
          <div>
            This team is managed by {startCase(team.managedBy)}.{" "}
            {joinInstructionsForTeamManagedBy(team.managedBy)}
          </div>
          <OpenInVercel team={team} />
        </div>
      </Sheet>
    );
  } else if (canAddMembers) {
    // Show invite form if you can add members.
    inviteMembers = (
      <InviteMemberForm team={team} members={members} canInvite={canInvite} />
    );
  } else if (members.length >= entitlements.maxTeamMembers!) {
    // Show an action item to upgrade if you have reached your member limit.
    inviteMembers = (
      <Callout>
        <div className="flex flex-col gap-2 p-2">
          <div>
            You've reached the member limit for this team.{" "}
            <Link
              href={`/${team.slug}/settings/billing`}
              className="items-center"
            >
              Upgrade
            </Link>{" "}
            to invite more team members.
          </div>
        </div>
      </Callout>
    );
  } else {
    // We've enumerated every case except for inactive subscriptions (canceled, delinquent, etc.),
    // so show an error message.
    inviteMembers = (
      <Callout variant="error">
        <div className="flex flex-col gap-2 p-2">
          <div>Your subscription is inactive.</div>
          <div>
            Contact us at{" "}
            <Link
              href="mailto:support@convex.dev"
              passHref
              className="items-center"
            >
              <EnvelopeClosedIcon className="mr-0.5 inline" />
              support@convex.dev
            </Link>{" "}
            to resolve this issue.
          </div>
        </div>
      </Callout>
    );
  }

  return (
    <>
      <h2>Members</h2>
      {inviteMembers}
      <TeamMemberList team={team} members={members} invites={invites} />
    </>
  );
}

function joinInstructionsForTeamManagedBy(managedBy: string) {
  switch (managedBy) {
    case "vercel":
      return 'Your Vercel team members may join this Convex team by clicking "Open in Convex" when viewing the Convex integration in their Vercel dashboard.';
    default:
      captureMessage(`Unknown team managed by: ${managedBy}`, "error");
      return "";
  }
}
