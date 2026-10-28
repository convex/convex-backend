import type { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { DeploymentExpirySheet } from "./DeploymentAdvancedSettings";

const meta: Meta<typeof DeploymentExpirySheet> = {
  component: DeploymentExpirySheet,
  args: {
    expiresAt: null,
    deploymentType: "dev",
    previewRetentionDays: 14,
    onSave: fn(),
  },
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Enabled: Story = {};

export const WithConfirmationOnSave: Story = {
  args: {
    deploymentType: "custom",
    expiresAt: Date.now() + 7 * 24 * 60 * 60 * 1000,
  },
};

export const WithExpiry: Story = {
  args: {
    expiresAt: Date.now() + 7 * 24 * 60 * 60 * 1000,
  },
};

export const Disabled: Story = {
  args: {
    disabled: "You can’t change this.",
  },
};
