import { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { DefaultEnvironmentVariablesRolloutDialog } from "./DefaultEnvironmentVariablesRolloutDialog";

const meta = {
  component: DefaultEnvironmentVariablesRolloutDialog,
  args: {
    onClose: fn(),
    onConfirm: fn(),
    rollout: [
      {
        name: "LOG_LEVEL",
        operation: "set",
        value: "debug",
        deploymentTypes: ["dev", "prod"],
      },
      {
        name: "SITE_URL",
        operation: "set",
        value: "https://example.com",
        deploymentTypes: ["preview"],
      },
      {
        name: "LEGACY_API_URL",
        operation: "delete",
        value: null,
        deploymentTypes: ["dev", "preview", "prod"],
      },
    ],
  },
} satisfies Meta<typeof DefaultEnvironmentVariablesRolloutDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
