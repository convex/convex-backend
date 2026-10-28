import { useRef } from "react";
import { usePaginatedScheduledJobs } from "@common/features/schedules/lib/usePaginatedScheduledJobs";
import { ScheduledFunctionsContentToolbar } from "@common/features/schedules/components/ScheduledFunctionsContentToolbar";
import { ScheduledFunctionsListHeader } from "@common/features/schedules/components/ScheduledFunctionsListHeader";
import { ScheduledFunctionsList } from "@common/features/schedules/components/ScheduledFunctionsList";
import { Sheet } from "@ui/Sheet";
import { ModuleFunction } from "@common/lib/functions/types";

export function ScheduledFunctionsContent({
  currentOpenFunction,
}: {
  currentOpenFunction: ModuleFunction | undefined;
}) {
  const {
    jobs,
    status,
    loadMore,
    isRateLimited,
    togglePaused,
    reload,
    isPaused,
    isLoadingPausedData,
  } = usePaginatedScheduledJobs(currentOpenFunction?.identifier);

  const outerRef = useRef<HTMLElement>(null);

  const isDataLoaded = isPaused || status !== "LoadingFirstPage";

  const hasScheduledJobs = jobs.length === 0 && status === "Exhausted";

  return (
    <div className="relative flex h-full max-w-6xl grow flex-col gap-4">
      <ScheduledFunctionsContentToolbar reload={reload} />
      <Sheet
        className="max-w-full min-w-160 grow overflow-hidden"
        padding={false}
      >
        <div className="flex h-full flex-col overflow-auto">
          <ScheduledFunctionsListHeader
            isPaused={isPaused}
            isLoadingPausedData={isLoadingPausedData}
            togglePaused={togglePaused}
            isRateLimited={isRateLimited}
            reload={reload}
          />
          {isDataLoaded && (
            <ScheduledFunctionsList
              hasScheduledJobs={hasScheduledJobs}
              currentOpenFunction={currentOpenFunction ?? undefined}
              jobs={jobs}
              outerRef={outerRef}
              status={status}
              loadMore={loadMore}
              isPaused={isPaused}
            />
          )}
        </div>
      </Sheet>
    </div>
  );
}
