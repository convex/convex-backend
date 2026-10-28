import type { Meta, StoryObj } from "@storybook/nextjs";
import { DeploymentExpirySheet } from "../../components/deploymentSettings/DeploymentAdvancedSettings";

const FIXED_NOW = new Date("2026-04-02T17:09:00Z").getTime();

const meta = {
  component: DeploymentExpirySheet,
  args: {
    expiresAt: FIXED_NOW + 14 * 24 * 60 * 60 * 1000,
    deploymentType: "dev",
    previewRetentionDays: undefined,
    onSave: async () => {},
  },
  beforeEach: () => {
    const originalNow = Date.now;
    Date.now = () => FIXED_NOW;
    return () => {
      Date.now = originalNow;
    };
  },
  decorators: [
    (Story) => (
      <div className="max-w-2xl">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof DeploymentExpirySheet>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
