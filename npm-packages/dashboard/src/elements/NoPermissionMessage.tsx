import { LockClosedIcon } from "@radix-ui/react-icons";
import { cn } from "@ui/cn";
import { useHasCustomRole } from "hooks/useDeploymentPermissions";
import type { RoleStatementAction } from "@convex-dev/platform/managementApi";

export function NoPermissionMessage({
  message,
  missingPermission,
  // `sm` fits the message inside a panel or dialog, where the page-sized
  // lock and copy would dwarf what they stand in for.
  size = "md",
}: {
  message: string;
  missingPermission: RoleStatementAction;
  size?: "sm" | "md";
}) {
  const hasCustomRole = useHasCustomRole();
  const isSmall = size === "sm";

  return (
    <div className="flex h-full grow items-center justify-center">
      <div
        className={cn(
          "flex flex-col items-center",
          isSmall ? "gap-1.5" : "gap-3",
        )}
      >
        <LockClosedIcon
          className={cn("text-content-tertiary", isSmall ? "size-5" : "size-8")}
        />
        <p
          className={cn(
            "text-content-secondary",
            isSmall ? "text-sm" : "text-base",
          )}
        >
          {message}
        </p>
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
