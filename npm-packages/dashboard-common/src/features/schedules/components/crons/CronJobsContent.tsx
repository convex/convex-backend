import {
  CalendarIcon,
  CheckIcon,
  ChevronLeftIcon,
  Cross2Icon,
  ExclamationTriangleIcon,
  PlayIcon,
  QuestionMarkCircledIcon,
  ReloadIcon,
  StopwatchIcon,
} from "@radix-ui/react-icons";
import { useRouter } from "next/router";
import React, { useEffect, useRef } from "react";
import {
  CronJobLog,
  CronJobWithRuns,
} from "system-udfs/convex/_system/frontend/common";
import { CronsTable } from "@common/features/schedules/components/crons/CronsTable";
import { useCronJobs } from "@common/features/schedules/lib/CronsProvider";
import { Button } from "@ui/Button";
import { PageContent } from "@common/elements/PageContent";
import { LoadingTransition } from "@ui/Loading";
import { Sheet } from "@ui/Sheet";
import { Tooltip } from "@ui/Tooltip";
import { Link } from "@ui/Link";
import { useFunctionUrl } from "@common/lib/deploymentApi";
import { formatDateTime, msFormat } from "@common/lib/format";
import { displayName } from "@common/lib/functions/generateFileTree";
import { LogLinesOutput } from "@common/elements/LogOutput";
import { entryOutput } from "@common/lib/useLogs";
import { EmptySection } from "@common/elements/EmptySection";

export function CronJobsContent() {
  const { loading, cronJobs, cronJobRuns } = useCronJobs();
  const router = useRouter();
  const detailsCron =
    cronJobs && cronJobs.find((c) => c.name === router.query.id);

  let content: React.ReactNode;
  if (!cronJobs || cronJobs.length === 0) {
    content = <NoCronJobs />;
  } else if (detailsCron && cronJobRuns) {
    const detailsCronJobRuns = cronJobRuns.filter(
      (x) => x.name === detailsCron.name,
    );
    content = (
      <Details cronJob={detailsCron} cronJobRuns={detailsCronJobRuns} />
    );
  } else {
    content = (
      <div className="flex size-full max-w-6xl flex-col gap-2">
        <div className="flex justify-between">
          <div className="flex flex-row items-center justify-between">
            <div className="flex flex-col gap-1">
              <div className="text-content-secondary">
                <span className="mr-1">Total cron jobs</span>
                <span className="font-semibold">{cronJobs.length}</span>
              </div>
            </div>
          </div>
        </div>
        <CronsTable cronJobs={cronJobs} />
      </div>
    );
  }

  return (
    <PageContent>
      <div className="size-full max-w-6xl">
        <LoadingTransition>{!loading && content}</LoadingTransition>
      </div>
    </PageContent>
  );
}

function Details({
  cronJob,
  cronJobRuns,
}: {
  cronJob: CronJobWithRuns;
  cronJobRuns: CronJobLog[];
}) {
  const router = useRouter();
  const back = () => {
    delete router.query.id;
    void router.push({ query: router.query });
  };
  const currentlyRunning = cronJob.nextRun.state.type === "inProgress";

  return (
    <div className="flex size-full max-w-6xl flex-col gap-4">
      <div className="flex shrink-0 flex-col overflow-hidden">
        <div className="flex flex-row items-center justify-between">
          <div className="flex flex-row items-center gap-2">
            <Button
              size="sm"
              variant="neutral"
              inline
              onClick={back}
              icon={
                <ChevronLeftIcon
                  className="size-4"
                  aria-label="Back to cron jobs"
                />
              }
            />
            <h3 className="whitespace-nowrap">{cronJob.name}</h3>
          </div>
        </div>
      </div>
      <Sheet className="h-full overflow-auto" padding={false}>
        <h4 className="sticky top-0 mb-4 flex items-center gap-2 border-b bg-background-secondary px-6 py-4 whitespace-nowrap">
          <ReloadIcon /> Executions
          <Tooltip
            tip="The logs and results of the last 5 executions of a cron job are available here, as well as any that run while this view is open."
            side="right"
          >
            <QuestionMarkCircledIcon />
          </Tooltip>
        </h4>

        <ul className="flex w-full flex-col border-b px-6">
          <li
            key="current"
            className={`w-fit p-2 ${currentlyRunning ? "" : "rounded-sm border border-dashed border-border-selected"}`}
          >
            <TopCronJobLogListItem cronJob={cronJob} />
          </li>
          {cronJobRuns.map((x) => (
            <li key={x._id} className="p-2">
              <CronJobLogListItem cronJobLog={x} />
            </li>
          ))}
        </ul>
      </Sheet>
    </div>
  );
}

export function CronJobLogListItem({ cronJobLog }: { cronJobLog: CronJobLog }) {
  const url = useFunctionUrl(cronJobLog.udfPath);
  return (
    <div className="flex items-start gap-4 font-mono text-xs">
      <div className="flex flex-col gap-2">
        <div className="flex h-6 items-center gap-4">
          <div className="whitespace-nowrap text-content-primary">
            {formatDateTime(new Date(Number(cronJobLog.ts / BigInt(1000000))))}
          </div>
          <div className="w-14 text-right whitespace-nowrap text-content-secondary">
            {cronJobLog.status.type !== "canceled" && cronJobLog.executionTime
              ? msFormat(cronJobLog.executionTime * 1000)
              : ""}
          </div>
          <LogStatusLine status={cronJobLog.status} />
          <Link href={url} className="truncate">
            {displayName(cronJobLog.udfPath)}
          </Link>
        </div>
        {cronJobLog.status.type === "success" ||
        cronJobLog.status.type === "err" ? (
          <LogLinesOutput
            output={entryOutput({
              logLines: cronJobLog.logLines.logLines,
              error:
                cronJobLog.status.type === "err"
                  ? cronJobLog.status.error.toString()
                  : null,
            })}
          />
        ) : null}
      </div>
    </div>
  );
}

/**
 * The next scheduled execution, or the currently running execution.
 */
export function TopCronJobLogListItem({
  cronJob,
}: {
  cronJob: CronJobWithRuns;
}) {
  const url = useFunctionUrl(cronJob.cronSpec.udfPath);

  const { nextRun } = cronJob;
  const { nextTs, state } = nextRun;
  const timestamp = formatDateTime(new Date(Number(nextTs / BigInt(1000000))));
  const currentlyRunning = state.type === "inProgress";

  // Make a quickly-updating timer to make function execution feel fast.
  // To avoid a React render every frame (often fine but can gum things up),
  // modify the DOM directly.
  const estRuntimeRef = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (currentlyRunning) {
      let handle = 0;
      const update = () => {
        if (estRuntimeRef.current) {
          const start = new Date(Number(nextTs) / 1000000);
          const s = msFormat(Date.now() - +start);
          estRuntimeRef.current.textContent = s;
          requestAnimationFrame(update);
        }
      };
      handle = requestAnimationFrame(update);
      return () => cancelAnimationFrame(handle);
    }
  }, [currentlyRunning, nextTs]);

  const textColor = currentlyRunning
    ? "text-content-primary"
    : "text-content-secondary";
  return (
    <div className="flex items-start gap-4 font-mono text-xs">
      <div className="flex flex-col gap-2">
        <div className="flex h-6 items-center gap-4">
          <div className={`${textColor} whitespace-nowrap`}>
            {timestamp ?? "unknown"}
          </div>
          <div
            className={`${textColor} w-14 text-right whitespace-nowrap text-content-secondary`}
          >
            {currentlyRunning ? <span ref={estRuntimeRef}>0ms</span> : ""}
          </div>
          <div className={`flex items-center gap-1 ${textColor}`}>
            {currentlyRunning ? (
              <PlayIcon className="animate-pulse" />
            ) : (
              <CalendarIcon />
            )}
            <span className="w-16">
              {currentlyRunning ? "running" : "scheduled"}
            </span>
          </div>
          <Link href={url} className="truncate">
            {displayName(cronJob.cronSpec.udfPath)}
          </Link>
        </div>
      </div>
    </div>
  );
}

const statusTypes: {
  [key in CronJobLog["status"]["type"]]: {
    textColor: string;
    Icon: React.FC<{ className?: string }>;
  };
} = {
  success: {
    textColor: "text-content-success",
    Icon: CheckIcon,
  },
  err: {
    textColor: "text-content-errorSecondary",
    Icon: Cross2Icon,
  },
  canceled: {
    textColor: "text-content-warning",
    Icon: ExclamationTriangleIcon,
  },
};

function LogStatusLine({ status }: { status: CronJobLog["status"] }) {
  const { textColor, Icon } = statusTypes[status.type];
  return (
    <div className={`flex items-center gap-1 ${textColor}`}>
      <Icon className="size-3.5" />
      <span className={`w-16 ${textColor}`}>
        {status.type === "success" ? (
          "success"
        ) : status.type === "err" ? (
          "failure"
        ) : status.type === "canceled" && Number(status.num_canceled) === 1 ? (
          <Tooltip tip="The previous run of this task was still in progress when this task was scheduled (or this deployment was offline)">
            skipped
          </Tooltip>
        ) : status.type === "canceled" ? (
          <Tooltip
            tip={`This run and ${
              Number(status.num_canceled) - 1
            } after it were canceled because the previous run was still in progress (or this deployment was offline)`}
          >
            canceled
          </Tooltip>
        ) : (
          "success"
        )}
      </span>
    </div>
  );
}

function NoCronJobs() {
  return (
    <EmptySection
      Icon={StopwatchIcon}
      header="Run backend code on a regular schedule"
      body={
        <>
          Cron jobs are defined in the <code>convex/crons.js file</code>.
        </>
      }
      learnMoreButton={{
        href: "https://docs.convex.dev/scheduling/cron-jobs",
        children: "Learn more about Cron Jobs.",
      }}
    />
  );
}
