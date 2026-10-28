import { LockClosedIcon } from "@radix-ui/react-icons";
import React, { useContext } from "react";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { RoleStatementAction } from "@convex-dev/platform/managementApi";

export function NoPermissionMessage({
  message,
  missingPermission,
}: {
  message: string;
  missingPermission: RoleStatementAction;
}) {
  const { useHasCustomRole } = useContext(DeploymentInfoContext);
  const hasCustomRole = useHasCustomRole();

  return (
    <div className="flex h-full grow items-center justify-center">
      <div className="flex flex-col items-center gap-3">
        <LockClosedIcon className="size-8 text-content-tertiary" />
        <p className="text-base text-content-secondary">{message}</p>
        {hasCustomRole && (
          <p className="text-xs text-content-tertiary">
            Missing permission:{" "}
            <code className="rounded-sm bg-background-tertiary px-1 py-0.5 font-mono">
              {missingPermission}
            </code>
          </p>
        )}
      </div>
    </div>
  );
}

/**
 * Lightweight tooltip body for permission-denied controls.
 * For custom-role members, surfaces the specific action name
 * so they (or support) can identify the missing grant.
 */
export function PermissionDeniedTip({
  message,
  action,
}: {
  message: string;
  action: RoleStatementAction;
}) {
  const { useHasCustomRole } = useContext(DeploymentInfoContext);
  const hasCustomRole = useHasCustomRole();

  return (
    <div className="flex flex-col gap-1">
      <span>{message}</span>
      {hasCustomRole && (
        <span className="text-xs opacity-75">
          Missing permission: <code className="font-mono">{action}</code>
        </span>
      )}
    </div>
  );
}
