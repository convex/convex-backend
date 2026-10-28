import { useContext } from "react";
import classNames from "classnames";
import { GearIcon } from "@radix-ui/react-icons";
import { ActionText } from "@common/elements/DeploymentEventContent";
import { ITEM_SIZE } from "@common/features/logs/components/LogListItem";
import { formatDateTime } from "@common/lib/format";
import { DeploymentAuditLogEvent } from "@common/lib/useDeploymentAuditLog";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

export function DeploymentEventListItem({
  event,
  focused = false,
  hitBoundary,
  setShownLog,
  logKey,
}: {
  event: DeploymentAuditLogEvent;
  focused?: boolean;
  hitBoundary?: "top" | "bottom" | null;
  setShownLog: () => void;
  logKey?: string;
}) {
  const { TeamMemberLink } = useContext(DeploymentInfoContext);
  const timestamp = formatDateTime(new Date(event._creationTime));

  // Only show boundary animation on the focused item
  const showBoundary = focused && hitBoundary;

  return (
    <div
      className={classNames(
        showBoundary === "top" && "animate-[bounceTop_0.375s_ease-out]",
        showBoundary === "bottom" && "animate-[bounceBottom_0.375s_ease-out]",
      )}
    >
      {/* eslint-disable-next-line react/forbid-elements */}
      <button
        type="button"
        data-log-key={logKey}
        className={classNames(
          "animate-fadeInFromLoading",
          "group flex items-center gap-3 w-full text-xs items-center",
          "hover:bg-background-tertiary/70",
          "focus:outline-none focus:border-y focus:border-border-selected",
          focused && "bg-background-highlight",
          "select-text",
        )}
        style={{
          height: ITEM_SIZE,
        }}
        onClick={setShownLog}
        onFocus={(e) => {
          // Only set shown log if focus is on the button itself,
          // not on child elements (like links)
          if (e.target === e.currentTarget) {
            setShownLog();
          }
        }}
        tabIndex={0}
      >
        <div className="min-w-37 pl-3 text-left font-mono whitespace-nowrap text-content-primary">
          {timestamp}
          <span className="text-content-secondary">
            .
            {new Date(event._creationTime)
              .toISOString()
              .split(".")[1]
              .slice(0, -1)}
          </span>
        </div>

        <hr className="min-w-41.5 bg-background-tertiary" />

        <div className="flex h-6 items-center gap-2 truncate">
          <GearIcon className="shrink-0" />
          <span className="truncate">
            {![
              "usage_limit_exceeded",
              "change_usage_limit_stop_state",
            ].includes(event.action) && (
              <>
                <TeamMemberLink
                  memberId={Number(event.member_id)}
                  name={event.memberName}
                />{" "}
              </>
            )}
            <ActionText event={event} />
          </span>
        </div>
      </button>
    </div>
  );
}
