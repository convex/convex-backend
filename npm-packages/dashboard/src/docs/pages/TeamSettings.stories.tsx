import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, fn } from "storybook/test";
import { useDeleteTeam, useUpdateTeam } from "api/teams";
import { useDeploymentRegions } from "api/deployments";
import { TeamSettingsPage } from "../../pages/t/[team]/settings";

const mockRegions: ReturnType<typeof useDeploymentRegions>["regions"] = [
  {
    displayName: "Europe (Ireland)",
    name: "aws-eu-west-1",
    available: true,
  },
  {
    displayName: "US East (N. Virginia)",
    name: "aws-us-east-1",
    available: true,
  },
];

const meta = {
  component: TeamSettingsPage,
  parameters: {
    layout: "fullscreen",
    a11y: {
      test: "todo",
    },
  },
  beforeEach: () => {
    mocked(useDeleteTeam).mockReturnValue(fn());
    mocked(useUpdateTeam).mockReturnValue(fn());
    mocked(useDeploymentRegions).mockReturnValue({
      regions: mockRegions,
      isLoading: false,
    });
  },
} satisfies Meta<typeof TeamSettingsPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
