import type { Meta, StoryObj } from "@storybook/nextjs";
import { userEvent, within } from "storybook/test";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { ConvexCloudReminderToast } from "./ConvexCloudReminderToast";

function deploymentInfoWithName(name: string) {
  return {
    ...mockDeploymentInfo,
    useCurrentDeployment: () => ({
      ...mockDeploymentInfo.useCurrentDeployment()!,
      name,
    }),
  };
}

const meta = {
  // Explicit title: the dashboard package also has a "components" folder.
  title: "selfHosted/ConvexCloudReminderToast",
  component: ConvexCloudReminderToast,
  args: {
    deploymentName: "anonymous-my-app",
  },
  argTypes: {
    deploymentName: {
      control: "text",
      description:
        "The toast only shows for `anonymous-*` and `tryitout-*` deployments.",
    },
  },
  // The toast positions itself in the bottom left corner of the page.
  parameters: { layout: "fullscreen" },
  render: ({ deploymentName }) => (
    <DeploymentInfoContext.Provider
      value={deploymentInfoWithName(deploymentName)}
    >
      <ConvexCloudReminderToast />
    </DeploymentInfoContext.Provider>
  ),
} satisfies Meta<{ deploymentName: string }>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Collapsed: Story = {};

export const Expanded: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", { name: /Enjoying Convex/i }),
    );
  },
};

export const TryItOutDeployment: Story = {
  args: {
    deploymentName: "tryitout-my-app",
  },
};

export const HiddenForOtherDeployments: Story = {
  args: {
    deploymentName: "happy-animal-123",
  },
};
