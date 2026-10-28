import { Meta, StoryObj } from "@storybook/nextjs";
import { AvailableTeams } from "./AvailableTeams";

const meta = {
  component: AvailableTeams,
} satisfies Meta<typeof AvailableTeams>;

export default meta;
type Story = StoryObj<typeof meta>;

// Every team the member is eligible to join, including any they ignored in the
// team switcher.
export const Primary: Story = {
  args: {
    offers: [
      {
        teamId: 14,
        teamName: "Example Org",
        email: "nicolas@example.org",
      },
      {
        teamId: 22,
        teamName: "Example Team",
        email: "nicolas.ettlin@example.org",
      },
    ],
  },
};
