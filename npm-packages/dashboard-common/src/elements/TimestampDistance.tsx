import { formatDistanceToNow } from "date-fns";
import { useRefresh } from "@common/lib/useRefresh";
import { cn } from "@ui/cn";
import { Tooltip } from "@ui/Tooltip";

export function TimestampDistance({
  prefix = "",
  date,
  className = "",
  delayDuration,
}: {
  prefix?: string;

  date: Date;
  className?: string;
  // Milliseconds before the tooltip with the full timestamp opens.
  delayDuration?: number;
}) {
  return (
    <Tooltip
      tip={date.toLocaleString(undefined, { timeZoneName: "short" })}
      delayDuration={delayDuration}
    >
      <div className={cn("text-xs text-content-secondary", className)}>
        {`${prefix} ${formatDistanceToNow(date, {
          addSuffix: true,
        }).replace("about ", "")}`}
      </div>
    </Tooltip>
  );
}

export function LiveTimestampDistance({
  prefix,
  date,
  className,
}: {
  prefix?: string;
  date: Date;
  className?: string;
}) {
  return (
    <Tooltip
      tip={date.toLocaleString(undefined, { timeZoneName: "short" })}
      className="truncate"
    >
      <div className={cn("truncate text-xs text-content-secondary", className)}>
        {prefix} <LiveTimestampDistanceInner date={date} />
      </div>
    </Tooltip>
  );
}

export function LiveTimestampDistanceInner({ date }: { date: Date }) {
  useRefresh();
  return (
    <>
      {formatDistanceToNow(date, {
        addSuffix: true,
      }).replace("about ", "")}
    </>
  );
}
