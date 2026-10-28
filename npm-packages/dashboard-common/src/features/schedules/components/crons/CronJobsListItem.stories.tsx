import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { Sheet } from "@ui/Sheet";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { CronJobLog } from "system-udfs/convex/_system/frontend/common";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { CronJobLogListItem } from "./CronJobsContent";

const tsNs = (ms: number) => BigInt(ms) * BigInt(1000000);

const mockClient = mockConvexReactClient().registerQueryFake(
  udfs.components.list,
  () => [],
);

const udfArgs = new TextEncoder().encode("[]").buffer as ArrayBuffer;
const now = 1711411200000; // 2024-03-25 12:00:00 UTC

const baseLog: CronJobLog = {
  _id: "log1" as Id<"_cron_job_logs">,
  _creationTime: now,
  name: "sendDailyDigest",
  ts: tsNs(now - 5000),
  udfPath: "actions/email:sendDailyDigest",
  udfArgs,
  executionTime: 1.5,
  logLines: {
    logLines: [],
    isTruncated: false,
  },
  status: { type: "success", result: null },
};

const meta = {
  component: CronJobLogListItem,
  render: (args) => (
    <Sheet>
      <ConvexProvider client={mockClient}>
        <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
          <CronJobLogListItem {...args} />
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </Sheet>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof CronJobLogListItem>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Success: Story = {
  args: {
    cronJobLog: {
      ...baseLog,
      logLines: {
        logLines: ["[LOG] Sent digest to 42 users", "[LOG] Done"],
        isTruncated: false,
      },
      status: { type: "success", result: null },
    },
  },
};

export const Error: Story = {
  args: {
    cronJobLog: {
      ...baseLog,
      logLines: {
        logLines: ["[LOG] Starting...", "[ERROR] Connection refused"],
        isTruncated: false,
      },
      status: {
        type: "err",
        error: "Connection refused: could not reach database",
      },
    },
  },
};

export const Canceled: Story = {
  args: {
    cronJobLog: {
      ...baseLog,
      status: { type: "canceled", num_canceled: BigInt(1) },
    },
  },
};

export const LongFunctionName: Story = {
  args: {
    cronJobLog: {
      ...baseLog,
      udfPath:
        "convex/deeply/nested/path/to/module:reallyLongFunctionNameThatShouldTruncate",
    },
  },
};
