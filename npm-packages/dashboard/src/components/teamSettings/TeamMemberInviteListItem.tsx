import { Menu, MenuItem } from "@ui/Menu";
import { DotsVerticalIcon } from "@radix-ui/react-icons";
import {
  CancelInvitationArgs,
  CreateInvitationArgs,
  InvitationResponse,
} from "@convex-dev/platform/managementApi";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { RoleDisplay } from "./RoleDisplay";

type TeamMemberInviteListItemProps = {
  invite: InvitationResponse;
  teamSlug: string;
  // Both gates may be `undefined` while permissions are loading.
  canInvite: boolean | undefined;
  canCancelInvite: boolean | undefined;
  onCreateInvite: (body: CreateInvitationArgs) => void;
  onCancelInvite: (body: CancelInvitationArgs) => void;
};

export function TeamMemberInviteListItem({
  invite,
  teamSlug,
  canInvite,
  canCancelInvite,
  onCreateInvite,
  onCancelInvite,
}: TeamMemberInviteListItemProps) {
  const onResend = () => {
    if (invite.role === "custom") {
      onCreateInvite({
        email: invite.email,
        role: "custom",
        customRoles: (invite.customRoles ?? []).map((r) => r.id),
      });
    } else {
      onCreateInvite({ email: invite.email, role: invite.role });
    }
  };
  const resendDisabled = canInvite !== true;
  const cancelDisabled = canCancelInvite !== true;
  const resendTip = resendDisabled
    ? permissionDeniedTip(
        "You do not have permission to resend invitations.",
        "member:invite",
      )
    : undefined;
  const cancelTip = cancelDisabled
    ? permissionDeniedTip(
        "You do not have permission to revoke invitations.",
        "member:cancelInvitation",
      )
    : undefined;
  return (
    <div className="flex items-center justify-between gap-4 border-b py-2 last:border-b-0">
      <div className="flex flex-col">
        <div className="text-sm text-content-secondary">{invite.email}</div>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        {invite.expired && (
          <span className="text-sm font-semibold text-content-error">
            Invitation expired
          </span>
        )}
        <RoleDisplay
          role={invite.role}
          customRoles={invite.customRoles}
          teamSlug={teamSlug}
        />
        <Menu
          placement="bottom-end"
          buttonProps={{
            variant: "neutral",
            size: "xs",
            icon: <DotsVerticalIcon />,
            "aria-label": "Invitation options",
          }}
        >
          <MenuItem
            disabled={resendDisabled}
            tip={resendTip}
            tipSide="left"
            action={onResend}
          >
            Resend
          </MenuItem>
          <MenuItem
            variant="danger"
            disabled={cancelDisabled}
            tip={cancelTip}
            tipSide="left"
            action={() => onCancelInvite({ email: invite.email })}
          >
            Revoke
          </MenuItem>
        </Menu>
      </div>
    </div>
  );
}
