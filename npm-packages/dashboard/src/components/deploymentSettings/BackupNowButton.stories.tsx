import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { Meta, StoryObj } from "@storybook/nextjs";
import { Sheet } from "@ui/Sheet";
import {
  useListCloudBackupsIfAvailable,
  useRequestCloudBackup,
} from "api/backups";
import {
  type BackupStorageSummary,
  useBackupStorageSummary,
} from "hooks/usageMetrics";
import { fn, mocked, userEvent, within } from "storybook/test";
import { BackupNowButton } from "./BackupNowButton";

const deployment: PlatformDeploymentResponse = {
  kind: "cloud",
  class: "s16",
  id: 12,
  name: "musical-otter-456",
  deploymentUrl: "https://musical-otter-456.convex.cloud",
  deploymentType: "prod",
  createTime: new Date("2026-01-01T00:00:00Z").getTime(),
  projectId: 7,
  creator: 1,
  previewIdentifier: null,
  region: "aws-us-east-1",
  isDefault: true,
  reference: "production",
};

const usageSummary: BackupStorageSummary = {
  databaseStorage: 5 * 1024 * 1024 * 1024,
  fileStorage: 10 * 1024 * 1024 * 1024,
};

const meta = {
  component: BackupNowButton,
  parameters: {
    layout: "centered",
  },
  args: {
    teamId: 2,
    deployment,
    maxCloudBackups: 10,
    canCreate: true,
    onBackupRequested: fn(),
  },
  beforeEach: () => {
    mocked(useListCloudBackupsIfAvailable).mockReturnValue([]);
    mocked(useRequestCloudBackup).mockReturnValue(fn());
    mocked(useBackupStorageSummary).mockReturnValue({
      data: usageSummary,
      error: undefined,
    });
  },
  render: (args) => (
    <Sheet className="w-fit">
      <BackupNowButton {...args} />
    </Sheet>
  ),
} satisfies Meta<typeof BackupNowButton>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Ready: Story = {};

export const RequestModal: Story = {
  parameters: {
    screenshotSelector: '[role="dialog"]',
  },
  play: async ({ canvasElement }) => {
    await userEvent.click(
      within(canvasElement).getByRole("button", { name: "Backup Now" }),
    );
    const dialog = await within(document.body).findByRole("dialog");
    await within(dialog).findByText(/\(Est\. 5\sGB\)/);
    await within(dialog).findByText(/\(Est\. \+10\sGB\)/);
  },
};

export const TablesOnlyExplanation: Story = {
  parameters: {
    screenshotSelector: '[role="dialog"], [role="tooltip"]',
  },
  play: async ({ canvasElement }) => {
    await userEvent.click(
      within(canvasElement).getByRole("button", { name: "Backup Now" }),
    );
    const dialog = within(await within(document.body).findByRole("dialog"));
    await userEvent.hover(
      dialog.getByLabelText("About including file storage"),
    );
    await within(document.body).findAllByText(
      /Files in the destination deployment aren't deleted or replaced/,
    );
  },
};

export const FileStorageTooLarge: Story = {
  parameters: {
    screenshotSelector: '[role="dialog"], [role="tooltip"]',
  },
  decorators: [
    (storyFn) => {
      mocked(useBackupStorageSummary).mockReturnValue({
        data: { ...usageSummary, fileStorage: 10 * 1024 ** 4 },
        error: undefined,
      });
      return storyFn();
    },
  ],
  play: async ({ canvasElement }) => {
    await userEvent.click(
      within(canvasElement).getByRole("button", { name: "Backup Now" }),
    );
    const dialog = await within(document.body).findByRole("dialog");
    await within(dialog).findByText(/\(Est\. \+10\sTB\)/);
    await userEvent.hover(within(dialog).getByText("Include file storage"));
    await within(document.body).findAllByText(
      "Backups can include up to 1 TB of file storage. This deployment has more than 1 TB.",
    );
  },
};
