import { Meta, StoryObj } from "@storybook/nextjs";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { BackupResponse } from "api/backups";
import { BackupRestoreSuccess } from "./BackupRestoreStatus";

const oneDayAgo = new Date();
oneDayAgo.setDate(oneDayAgo.getDate() - 1);

const inOneWeek = new Date();
inOneWeek.setDate(inOneWeek.getDate() + 7);

const backup: BackupResponse = {
  id: 1,
  snapshotId: "yo" as Id<"_exports">,
  sourceDeploymentId: 1,
  sourceDeploymentName: "joyful-capybara-123",
  state: "complete",
  requestedTime: +oneDayAgo,
  expirationTime: +inOneWeek,
  includeStorage: true,
};

const deployment: PlatformDeploymentResponse = {
  kind: "cloud",
  class: "s16",
  id: 1,
  name: "joyful-capybara-123",
  deploymentUrl: "https://joyful-capybara-123.convex.cloud",
  deploymentType: "prod",
  createTime: +oneDayAgo,
  projectId: 1,
  creator: 1,
  previewIdentifier: null,
  region: "aws-us-east-1",
  isDefault: true,
  reference: "production",
};

const meta = {
  component: BackupRestoreSuccess,
  args: {
    completedTime: new Date(),
    restoredRowsCount: 336_185,
    deployment,
    backup,
    snapshotImportCheckpoints: undefined,
  },
} satisfies Meta<typeof BackupRestoreSuccess>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: {
    completedTime: new Date(),
    restoredRowsCount: 336_185,
  },
};
