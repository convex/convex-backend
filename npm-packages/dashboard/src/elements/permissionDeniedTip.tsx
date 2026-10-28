import type { RoleStatementAction } from "@convex-dev/platform/managementApi";
import { useCurrentTeam } from "api/teams";
import { useMyCustomRoles } from "api/roles";

// Renders a tooltip body for a disabled button when the user is missing a
// permission. For custom-role members, the missing action is shown in a
// monospace code style so support / admins can grep for it. Built-in admin /
// developer members don't see the code — the action name isn't actionable
// for them.
function PermissionDeniedTipBody({
  message,
  action,
}: {
  message: string;
  action: RoleStatementAction;
}) {
  const team = useCurrentTeam();
  const myRoles = useMyCustomRoles(team?.id);
  const showAction = myRoles?.role === "custom";
  return (
    <>
      {message}
      {showAction && (
        <div className="mt-1 text-xs opacity-80">
          Missing permission: <code className="font-mono">{action}</code>
        </div>
      )}
    </>
  );
}

export function permissionDeniedTip(
  message: string,
  action: RoleStatementAction,
): React.ReactNode {
  return <PermissionDeniedTipBody message={message} action={action} />;
}
