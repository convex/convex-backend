import { StopwatchIcon } from "@radix-ui/react-icons";
import { EmptySection } from "@common/elements/EmptySection";
import { useCurrentOpenFunction } from "@common/lib/functions/FunctionsProvider";
import { displayName } from "@common/lib/functions/generateFileTree";

export function NoScheduledJobs() {
  return (
    <EmptySection
      Icon={StopwatchIcon}
      header="Schedule functions to run later"
      body="Scheduled functions can run after an amount of time passes, or at a specific date."
      sheet={false}
      learnMoreButton={{
        href: "https://docs.convex.dev/scheduling/scheduled-functions",
        children: "Learn more about scheduled functions",
      }}
    />
  );
}

export function NoScheduledJobsForFunction() {
  const currentOpenFunction = useCurrentOpenFunction();
  return (
    <div className="flex size-full items-center justify-center text-content-secondary">
      <span>
        There are no scheduled jobs for{" "}
        <code className="font-semibold">
          {currentOpenFunction
            ? displayName(
                currentOpenFunction?.displayName,
                currentOpenFunction?.componentPath ?? null,
              )
            : null}
        </code>
      </span>
    </div>
  );
}
