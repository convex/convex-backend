import { Meta, StoryObj } from "@storybook/nextjs";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import udfs from "@common/udfs";
import { ConvexProvider } from "convex/react";
import { useMemo } from "react";
import { expect, mocked, userEvent, waitFor, within } from "storybook/test";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { Export } from "system-udfs/convex/_system/frontend/common";
import { useCurrentDeployment } from "api/deployments";
import { useIsOperationAllowed } from "hooks/useDeploymentPermissions";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { SnapshotExport } from "./SnapshotExport";

const NOW = new Date("2026-09-01T19:00:00Z").getTime();
const DAY = 24 * 60 * 60 * 1000;
const EXPORT_ID = "ex1234567890abcdefghijklmnopqr" as Id<"_exports">;

const mockDeployment = {
  id: 12,
  name: "musical-otter-456",
  deploymentType: "prod",
  kind: "cloud",
  isDefault: true,
  projectId: 7,
  creator: 1,
  createTime: NOW - 120 * DAY,
  class: "s256",
  deploymentUrl: "https://musical-otter-456.convex.cloud",
  reference: "production",
  region: "aws-us-east-1",
} satisfies PlatformDeploymentResponse;

const toNanoseconds = (milliseconds: number) =>
  BigInt(milliseconds) * BigInt(1_000_000);

const baseExport = {
  _id: EXPORT_ID,
  _creationTime: NOW - 60_000,
  requestor: "snapshotExport" as const,
};

const requestedExport: Export = {
  ...baseExport,
  state: "requested",
};

const inProgressExport: Export = {
  ...baseExport,
  state: "in_progress",
  start_ts: toNanoseconds(NOW - 45_000),
  progress_message: "Exporting tables",
};

const canceledExport: Export = {
  ...baseExport,
  state: "canceled",
  start_ts: null,
  canceled_ts: toNanoseconds(NOW),
};

const failedExport: Export = {
  ...baseExport,
  state: "failed",
  start_ts: toNanoseconds(NOW - 60_000),
  failed_ts: toNanoseconds(NOW),
};

const completedExport: Export = {
  ...baseExport,
  state: "completed",
  start_ts: toNanoseconds(NOW - 60_000),
  complete_ts: toNanoseconds(NOW - 30_000),
  expiration_ts: toNanoseconds(NOW + 7 * DAY),
  zip_object_key: "snapshot.zip",
  format: { format: "zip", include_storage: true },
};

type SnapshotExportStoryProps = {
  existingExport: Export | null;
  canCancel: boolean;
};

function SnapshotExportStory({
  existingExport,
  canCancel,
}: SnapshotExportStoryProps) {
  mocked(useCurrentDeployment).mockReturnValue(mockDeployment);
  mocked(useIsOperationAllowed).mockReturnValue(canCancel);
  mocked(useLaunchDarkly).mockReturnValue({
    ephemeralZipExportToken: true,
  } as ReturnType<typeof useLaunchDarkly>);

  const client = useMemo(
    () =>
      mockConvexReactClient().registerQueryFake(
        udfs.latestExport.default,
        () => existingExport,
      ),
    [existingExport],
  );
  const connectedDeployment = useMemo(
    () => ({
      deployment: {
        client,
        httpClient: {} as never,
        deploymentUrl: mockDeployment.deploymentUrl,
        adminKey: "storybook-admin-key",
        deploymentName: mockDeployment.name,
      },
      isDisconnected: false,
    }),
    [client],
  );

  return (
    <ConnectedDeploymentContext.Provider value={connectedDeployment}>
      <ConvexProvider client={client}>
        <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
          <div className="w-2xl">
            <SnapshotExport />
          </div>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  );
}

const meta = {
  component: SnapshotExportStory,
  args: {
    existingExport: inProgressExport,
    canCancel: true,
  },
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof SnapshotExportStory>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NoExport: Story = {
  args: { existingExport: null },
};

export const Requested: Story = {
  args: { existingExport: requestedExport },
};

export const InProgress: Story = {};

export const InProgressWithoutPermission: Story = {
  args: { canCancel: false },
};

export const CancelConfirmation: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.click(
      within(canvasElement).getByRole("button", { name: "Cancel" }),
    );
    await waitFor(() =>
      expect(
        within(canvasElement.ownerDocument.body).getByText(
          "Canceling this export will discard its progress. You will need to start a new export to try again.",
        ),
      ).toBeVisible(),
    );
  },
};

export const Canceled: Story = {
  args: { existingExport: canceledExport },
};

export const Failed: Story = {
  args: { existingExport: failedExport },
};

export const Completed: Story = {
  args: { existingExport: completedExport },
};

export const Expired: Story = {
  args: {
    existingExport: {
      ...completedExport,
      expiration_ts: toNanoseconds(NOW - DAY),
    },
  },
};
