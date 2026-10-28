import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import {
  CronJobLog,
  CronJobWithRuns,
  CronSchedule,
} from "system-udfs/convex/_system/frontend/common";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { CronsTable } from "./CronsTable";

const mockClient = mockConvexReactClient().registerQueryFake(
  udfs.components.list,
  () => [],
);

const now = Date.now();
const tsNs = (ms: number) => BigInt(Math.round(ms)) * BigInt(1000000);
const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;

// Crons always pass exactly one argument object to the function, so `udfArgs`
// is a single-element array even when the user didn’t specify any arguments.
const encodeArgs = (args: Record<string, unknown>) =>
  new TextEncoder().encode(JSON.stringify([args])).buffer as ArrayBuffer;

let idCounter = 0;

function cronJob({
  name,
  schedule,
  udfPath,
  args = {},
  nextIn,
  lastRunAgo,
  lastRunStatus = { type: "success", result: null },
  running = false,
}: {
  name: string;
  schedule: CronSchedule;
  udfPath: string;
  args?: Record<string, unknown>;
  nextIn: number;
  lastRunAgo?: number;
  lastRunStatus?: CronJobLog["status"];
  running?: boolean;
}): CronJobWithRuns {
  idCounter += 1;
  const id = `cron${idCounter}` as Id<"_cron_jobs">;
  return {
    _id: id,
    _creationTime: now - 30 * 24 * HOUR,
    name,
    cronSpec: { udfPath, udfArgs: encodeArgs(args), cronSchedule: schedule },
    lastRun:
      lastRunAgo === undefined
        ? null
        : {
            _id: `log${idCounter}` as Id<"_cron_job_logs">,
            _creationTime: now - lastRunAgo,
            name,
            ts: tsNs(now - lastRunAgo),
            udfPath,
            udfArgs: encodeArgs(args),
            executionTime: 1.5,
            logLines: { logLines: [], isTruncated: false },
            status: lastRunStatus,
          },
    nextRun: {
      _id: `next${idCounter}` as Id<"_cron_next_run">,
      _creationTime: now - 30 * 24 * HOUR,
      cronJobId: id,
      state: { type: running ? "inProgress" : "pending" },
      prevTs: lastRunAgo === undefined ? null : tsNs(now - lastRunAgo),
      nextTs: tsNs(now + nextIn),
    },
  };
}

const meta = {
  component: CronsTable,
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <CronsTable {...args} />
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof CronsTable>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    cronJobs: [
      cronJob({
        name: "sendDailyDigest",
        schedule: { type: "daily", hourUTC: BigInt(8), minuteUTC: BigInt(0) },
        udfPath: "actions/email:sendDailyDigest",
        nextIn: 6 * HOUR,
        lastRunAgo: 18 * HOUR,
      }),
      cronJob({
        name: "cleanup",
        schedule: { type: "interval", seconds: BigInt(3600) },
        udfPath: "crons:cleanup",
        nextIn: 12 * MINUTE,
        lastRunAgo: 48 * MINUTE,
      }),
      cronJob({
        name: "weeklyReport",
        schedule: {
          type: "weekly",
          dayOfWeek: BigInt(1),
          hourUTC: BigInt(9),
          minuteUTC: BigInt(30),
        },
        udfPath: "reports:weekly",
        args: { format: "pdf", recipients: ["team@example.com"] },
        nextIn: 3 * 24 * HOUR,
        lastRunAgo: 4 * 24 * HOUR,
      }),
      cronJob({
        name: "customExpression",
        schedule: { type: "cron", cronExpr: "*/15 9-17 * * 1-5" },
        udfPath: "internal/metrics:collect",
        nextIn: 4 * MINUTE,
        lastRunAgo: 11 * MINUTE,
      }),
    ],
  },
};

export const NeverRan: Story = {
  args: {
    cronJobs: [
      cronJob({
        name: "hourlyRollup",
        schedule: { type: "hourly", minuteUTC: BigInt(15) },
        udfPath: "crons:hourlyRollup",
        nextIn: 25 * MINUTE,
      }),
    ],
  },
};

export const Running: Story = {
  args: {
    cronJobs: [
      cronJob({
        name: "backfill",
        schedule: { type: "interval", seconds: BigInt(300) },
        udfPath: "migrations:backfill",
        nextIn: 2 * MINUTE,
        lastRunAgo: 30 * 1000,
        running: true,
      }),
    ],
  },
};

export const LastRunFailed: Story = {
  args: {
    cronJobs: [
      cronJob({
        name: "syncInventory",
        schedule: { type: "monthly", day: BigInt(1), hourUTC: BigInt(0) },
        udfPath: "actions/inventory:sync",
        nextIn: 9 * 24 * HOUR,
        lastRunAgo: 21 * 24 * HOUR,
        lastRunStatus: {
          type: "err",
          error: "Connection refused: could not reach database",
        },
      }),
      cronJob({
        name: "skippedJob",
        schedule: { type: "interval", seconds: BigInt(60) },
        udfPath: "crons:skippedJob",
        // The backend hasn't caught up yet, so the next run is in the past.
        nextIn: -3 * MINUTE,
        lastRunAgo: 4 * MINUTE,
        lastRunStatus: { type: "canceled", num_canceled: BigInt(1) },
      }),
    ],
  },
};

export const LongNames: Story = {
  args: {
    cronJobs: [
      cronJob({
        name: "aVeryLongCronJobNameThatShouldTruncateInTheNameColumn",
        schedule: { type: "interval", seconds: BigInt(90) },
        udfPath:
          "convex/deeply/nested/path/to/module:reallyLongFunctionNameThatShouldTruncate",
        args: { some: "argument", count: 42, other: "another" },
        nextIn: 45 * 1000,
        lastRunAgo: 45 * 1000,
      }),
    ],
  },
};

export const Empty: Story = {
  args: { cronJobs: [] },
};
