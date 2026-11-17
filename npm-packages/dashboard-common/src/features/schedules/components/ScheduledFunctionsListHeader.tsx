import { PlayCircleIcon, PauseCircleIcon } from "@heroicons/react/24/outline";
import {
  QuestionMarkCircledIcon,
  CaretUpIcon,
  ExclamationTriangleIcon,
  ReloadIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";

export function ScheduledFunctionsListHeader({
  isPaused,
  isLoadingPausedData,
  togglePaused,
  isRateLimited,
  reload,
}: {
  isPaused: boolean;
  isLoadingPausedData: boolean;
  togglePaused: () => void;
  isRateLimited: boolean;
  reload: () => void;
}) {
  return (
    <div className="sticky top-0 flex items-center gap-4 border-b p-2 text-xs text-content-secondary">
      <p className="flex min-w-20 items-center gap-1">
        ID{" "}
        <Tooltip
          tip="The ID of this scheduled job. Can be used to programmatically cancel this scheduled function."
          side="right"
        >
          <QuestionMarkCircledIcon />
        </Tooltip>
      </p>
      <p className="flex min-w-36 gap-1">
        Scheduled Time{" "}
        <Tooltip tip="Scheduled function data is sorted by the scheduled time, with the nearest upcoming runs coming first. The ability to sort this data differently is coming soon.">
          <CaretUpIcon />
        </Tooltip>
      </p>
      <p className="min-w-20">Status</p>
      <p>Function</p>
      <div className="ml-auto flex items-center gap-2">
        {isRateLimited && (
          <Tooltip tip="Live updates have automatically been paused because the scheduled functions are updating too frequently in this deployment.">
            <ExclamationTriangleIcon className="mt-0.5 text-content-warning" />
          </Tooltip>
        )}
        {isPaused && (
          <Button
            icon={<ReloadIcon />}
            loading={isLoadingPausedData}
            variant="neutral"
            className="animate-fadeInFromLoading text-xs"
            size="xs"
            onClick={() => {
              reload();
            }}
            tip="Refresh the list of scheduled functions. While paused, only the first page of upcoming function runs are refreshed."
          >
            <span className="sr-only">Refresh</span>
          </Button>
        )}
        <Button
          size="xs"
          variant="neutral"
          className="text-xs"
          icon={
            isPaused ? (
              <PlayCircleIcon className="size-4" />
            ) : (
              <PauseCircleIcon className="size-4" />
            )
          }
          onClick={togglePaused}
          tip={
            isRateLimited
              ? "Scheduled functions are being run too frequently to show live updates."
              : isPaused
                ? "Resume to show live updates."
                : "Pause to prevent live updates."
          }
          disabled={isRateLimited}
        >
          <span className="sr-only">{isPaused ? "Go Live" : "Pause"}</span>
        </Button>
      </div>
    </div>
  );
}
