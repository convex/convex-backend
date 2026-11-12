import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, screen, userEvent } from "storybook/test";
import { DefaultEnvironmentVariablesRolloutDialog } from "components/projectSettings/DefaultEnvironmentVariablesRolloutDialog";

const meta = {
  component: DefaultEnvironmentVariablesRolloutDialog,
  parameters: {
    // Headless UI gives the modal panel this id; `[role="dialog"]` is the
    // full-screen wrapper behind it.
    screenshotSelector: '[id^="headlessui-dialog-panel-"]',
    // The dialog's table is at least 960px wide.
    screenshotViewport: { width: 1100, height: 700 },
  },
  args: {
    onClose: fn(),
    onConfirm: fn(),
    rollout: [
      {
        name: "API_URL",
        operation: "set",
        value: "https://staging-v2.api.example.com",
        deploymentTypes: ["dev", "preview"],
      },
      {
        name: "FEATURE_FLAGS",
        operation: "set",
        value: "new-checkout",
        deploymentTypes: ["dev", "preview", "prod"],
      },
      {
        name: "LEGACY_API_URL",
        operation: "delete",
        value: null,
        deploymentTypes: ["prod"],
      },
    ],
  },
} satisfies Meta<typeof DefaultEnvironmentVariablesRolloutDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  play: async () => {
    // The dialog portals to document.body, outside the story canvas.
    await userEvent.click(
      await screen.findByRole("checkbox", {
        name: "Upsert to existing deployments: API_URL",
      }),
    );
    // Clear the click's focus ring from the screenshot.
    (document.activeElement as HTMLElement | null)?.blur();
  },
};
