import { Meta, StoryObj } from "@storybook/nextjs";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { Sheet } from "@ui/Sheet";
import {
  useConfigurePeriodicBackup,
  useDisablePeriodicBackup,
  useGetPeriodicBackupConfig,
} from "api/backups";
import { PeriodicBackupConfig } from "generatedApi";
import {
  type BackupStorageSummary,
  useBackupStorageSummary,
} from "hooks/usageMetrics";
import { expect, fn, mocked, userEvent, within } from "storybook/test";
import { AutomaticBackupSelector } from "./Backups";

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

const periodicBackup: PeriodicBackupConfig = {
  sourceDeploymentId: deployment.id,
  cronspec: "0 17 * * *",
  expirationDeltaSecs: 7 * 24 * 60 * 60,
  nextRun: new Date("2026-04-01T17:00:00Z").getTime(),
  includeStorage: false,
};

const usageSummary: BackupStorageSummary = {
  databaseStorage: 5 * 1024 * 1024 * 1024,
  fileStorage: 10 * 1024 * 1024 * 1024,
};

const meta = {
  component: AutomaticBackupSelector,
  parameters: {
    a11y: { test: "todo" },
  },
  args: {
    teamId: 2,
    deployment,
    canConfigurePeriodic: true,
    canDisablePeriodic: true,
  },
  beforeEach: () => {
    mocked(useGetPeriodicBackupConfig).mockReturnValue(periodicBackup);
    mocked(useConfigurePeriodicBackup).mockReturnValue(fn());
    mocked(useDisablePeriodicBackup).mockReturnValue(fn());
    mocked(useBackupStorageSummary).mockReturnValue({
      data: usageSummary,
      error: undefined,
    });
  },
  render: (args) => (
    <Sheet className="w-xl">
      <AutomaticBackupSelector {...args} />
    </Sheet>
  ),
} satisfies Meta<typeof AutomaticBackupSelector>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Configured: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await canvas.findByText(/\(Est\. 5\sGB\)/);
    await canvas.findByText(/\(Est\. \+10\sGB\)/);
    await canvas.findByLabelText("Backup usage pricing");
    await canvas.findByLabelText("About including file storage");
  },
};

export const LoadingEstimate: Story = {
  decorators: [
    (storyFn) => {
      mocked(useBackupStorageSummary).mockReturnValue({
        data: undefined,
        error: undefined,
      });
      return storyFn();
    },
  ],
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await canvas.findAllByText("(Est.)");
    // The size is only advisory, so it doesn't gate the checkbox while loading.
    await expect(canvas.getByLabelText(/Include file storage/)).toBeEnabled();
  },
};

export const UsagePricingTooltip: Story = {
  parameters: {
    screenshotSelector: '#storybook-root, [role="tooltip"]',
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.hover(canvas.getByLabelText("Backup usage pricing"));
    await within(document.body).findAllByRole("link", {
      name: "storage and bandwidth usage",
    });
  },
};

export const ScheduleOptions: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Daily" }));
    await canvas.findByLabelText("Never");
    await canvas.findByLabelText("Daily");
    await canvas.findByLabelText("Weekly");
  },
};

export const Never: Story = {
  decorators: [
    (storyFn) => {
      mocked(useGetPeriodicBackupConfig).mockReturnValue(null);
      return storyFn();
    },
  ],
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await canvas.findByText("Backup");
    await canvas.findByRole("button", { name: "Never" });
    await expect(
      canvas.queryByText("Include file storage"),
    ).not.toBeInTheDocument();
  },
};

export const EnablingAutomaticBackups: Story = {
  decorators: [
    (storyFn) => {
      mocked(useGetPeriodicBackupConfig).mockReturnValue(null);
      return storyFn();
    },
  ],
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Never" }));
    await userEvent.click(await canvas.findByLabelText("Daily"));
    await expect(
      canvas.getByRole("link", { name: "storage and bandwidth usage" }),
    ).toBeVisible();
  },
};

export const FileStorageTooLarge: Story = {
  parameters: {
    screenshotSelector: '#storybook-root, [role="tooltip"]',
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
    const canvas = within(canvasElement);
    await canvas.findByText(/\(Est\. \+10\sTB\)/);
    await userEvent.hover(canvas.getByText("Include file storage"));
    await within(document.body).findAllByText(
      "Backups can include up to 1 TB of file storage. This deployment has more than 1 TB.",
    );
  },
};

export const ExistingFileStorageSelectionTooLarge: Story = {
  decorators: [
    (storyFn) => {
      mocked(useGetPeriodicBackupConfig).mockReturnValue({
        ...periodicBackup,
        includeStorage: true,
      });
      mocked(useBackupStorageSummary).mockReturnValue({
        data: { ...usageSummary, fileStorage: 10 * 1024 ** 4 },
        error: undefined,
      });
      return storyFn();
    },
  ],
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const checkbox = canvas.getByLabelText(/Include file storage/);
    await expect(checkbox).toBeChecked();
    await expect(checkbox).toBeEnabled();
    await canvas.findByText(/\(Est\. \+10\sTB\)/);
  },
};
