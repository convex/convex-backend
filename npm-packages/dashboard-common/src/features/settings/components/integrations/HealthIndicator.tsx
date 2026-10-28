import classNames from "classnames";
import { ReactNode } from "react";
import { Integration } from "system-udfs/convex/_system/frontend/common";
import { Tooltip } from "@ui/Tooltip";

type HealthStatusMetadata = {
  lightTextColor: string;
  darkTextColor: string;
};

function statusToColors(
  status: Integration["status"]["type"],
): HealthStatusMetadata {
  switch (status) {
    case "active":
      return {
        lightTextColor: "text-green-700",
        darkTextColor: "dark:text-green-200",
      };
    case "pending":
      return {
        lightTextColor: "text-yellow-700",
        darkTextColor: "dark:text-yellow-200",
      };
    case "failed":
      return {
        lightTextColor: "text-content-error",
        darkTextColor: "text-content-error",
      };
    case "deleting":
      return {
        lightTextColor: "text-slate-700",
        darkTextColor: "dark:text-slate-200",
      };
    default:
      status satisfies never;
      throw new Error(`Unrecognized health status ${status}`);
  }
}

/**
 * The colored status word an integration card shows above its detail line.
 * Takes its own `children` so integrations that track something other than a
 * `_log_sinks` status can still label themselves consistently.
 */
export function HealthLabel({
  type,
  children,
}: {
  type: Integration["status"]["type"];
  children: ReactNode;
}) {
  const { lightTextColor, darkTextColor } = statusToColors(type);
  return (
    <div
      className={classNames("text-xs", `${lightTextColor} ${darkTextColor}`)}
    >
      {children}
    </div>
  );
}

export function HealthIndicator({ status }: { status: Integration["status"] }) {
  return (
    <HealthLabel type={status.type}>
      {status.type === "failed" ? (
        <Tooltip tip={`Reason: ${status.reason}`}>Failed</Tooltip>
      ) : (
        status.type.charAt(0).toUpperCase() + status.type.slice(1)
      )}
    </HealthLabel>
  );
}
