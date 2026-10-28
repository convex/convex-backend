import { Meta, StoryObj } from "@storybook/nextjs";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { Sheet } from "@ui/Sheet";
import { useAccessToken } from "hooks/useServerSideData";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { BackupResponse } from "api/backups";
import { BackupListItem } from "./BackupListItem";

const now = new Date();

const inOneWeek = new Date();
inOneWeek.setDate(inOneWeek.getDate() + 7);

const oneWeekAgo = new Date();
oneWeekAgo.setDate(oneWeekAgo.getDate() - 7);

const backup: BackupResponse = {
  id: 1,
  snapshotId: "yo" as Id<"_exports">,
  sourceDeploymentId: 1,
  sourceDeploymentName: "joyful-capybara-123",
  state: "complete",
  requestedTime: +now,
  expirationTime: +inOneWeek,
  includeStorage: true,
};

const targetDeployment: PlatformDeploymentResponse = {
  kind: "cloud",
  class: "s16",
  id: 1,
  name: "joyful-capybara-123",
  deploymentUrl: "https://joyful-capybara-123.convex.cloud",
  deploymentType: "prod",
  createTime: +oneWeekAgo,
  projectId: 1,
  creator: 1,
  previewIdentifier: null,
  region: "aws-us-east-1",
  isDefault: true,
  reference: "production",
};

const meta = {
  component: BackupListItem,
  args: {
    teamId: 1,
    backup,
    restoring: false,
    someBackupInProgress: false,
    someRestoreInProgress: false,
    latestBackupInTargetDeployment: {
      ...backup,
      requestedTime: +oneWeekAgo,
    },
    targetDeployment,
    canCreate: true,
    canImport: true,
    canDelete: true,
    downloadZipExport: async () => {},
    maxCloudBackups: 2,
    progressMessage: null,
  },
  render: StoryRender,
} satisfies Meta<typeof BackupListItem>;

export default meta;
type Story = StoryObj<typeof meta>;

export const RestorableWithNoWarning: Story = {
  args: {
    latestBackupInTargetDeployment: { ...backup, requestedTime: Date.now() },
    targetDeployment: { ...targetDeployment, deploymentType: "dev" },
  },
};

export const RestorableWithOldBackupWarning: Story = {};

export const RestorableWithNoBackupWarning: Story = {
  args: {
    latestBackupInTargetDeployment: null,
  },
};

export const RestorableFromAnotherDeployment: Story = {
  args: {
    targetDeployment: { ...targetDeployment, id: 42, deploymentType: "dev" },
  },
};

export const BackupInProgress: Story = {
  args: {
    backup: { ...backup, state: "inProgress" },
    someBackupInProgress: true,
  },
};

export const BackupFailed: Story = {
  args: {
    backup: { ...backup, state: "failed" },
    someBackupInProgress: true,
  },
};

export const Restoring: Story = {
  args: {
    restoring: true,
    someRestoreInProgress: true,
  },
};

export const OtherBackupInProgress: Story = {
  args: {
    someBackupInProgress: true,
  },
};

export const OtherBackupBeingRestored: Story = {
  args: {
    someRestoreInProgress: true,
  },
};

export const MissingAdminRights: Story = {
  args: {
    canCreate: false,
    canImport: false,
    canDelete: false,
  },
};

function StoryRender(args: Parameters<typeof BackupListItem>[0]) {
  const [_, setAccessToken] = useAccessToken();
  setAccessToken("Mock");

  return (
    <Sheet>
      <div className="border-y">
        <BackupListItem {...args} />
      </div>
    </Sheet>
  );
}
