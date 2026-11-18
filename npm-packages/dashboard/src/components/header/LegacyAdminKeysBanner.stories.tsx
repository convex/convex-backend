import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked } from "storybook/test";
import { TeamResponse } from "generatedApi";
import { useCurrentTeam } from "api/teams";
import { LegacyAdminKeysBanner } from "./LegacyAdminKeysBanner";

const team: TeamResponse = {
  id: 1,
  slug: "acme",
  name: "Acme",
  creator: 1,
  suspended: false,
  referralCode: "ACME123",
  referredBy: null,
};

const meta = {
  component: LegacyAdminKeysBanner,
  args: { deploymentCount: 2, onDismiss: fn() },
  beforeEach: () => {
    mocked(useCurrentTeam).mockReturnValue(team);
  },
} satisfies Meta<typeof LegacyAdminKeysBanner>;

export default meta;
type Story = StoryObj<typeof meta>;

export const MultipleDeployments: Story = {};

export const OneDeployment: Story = {
  args: { deploymentCount: 1 },
};

export const AtListLimit: Story = {
  args: { deploymentCount: 100 },
};
