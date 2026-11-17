import type { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked } from "storybook/test";
import { TransferProject } from "components/projects/TransferProject";
import { useBBMutation } from "api/api";
import { useTeamOrbSubscription } from "api/billing";
import { useProfile } from "api/profile";
import { useCurrentProject } from "api/projects";
import { useHasCustomRolePermission } from "api/roles";
import { useCurrentTeam, useTeamMembers, useTeams } from "api/teams";
import type { OrbSubscriptionResponse } from "generatedApi";

const originTeam = {
  id: 2,
  creator: 1,
  slug: "acme",
  name: "Acme Corp",
  suspended: false,
  referralCode: "ACME01",
  referredBy: null,
};

const destinationTeam = {
  id: 3,
  creator: 1,
  slug: "globex",
  name: "Globex",
  suspended: false,
  referralCode: "GLOBEX01",
  referredBy: null,
};

const project = {
  id: 7,
  teamId: originTeam.id,
  name: "My amazing app",
  slug: "my-amazing-app",
  createTime: Date.now(),
  prodDeploymentName: "musical-otter-456",
  devDeploymentName: "happy-capybara-123",
} as NonNullable<ReturnType<typeof useCurrentProject>>;

const profile = {
  id: 1,
  name: "Nicolas Ettlin",
  email: "nicolas@acme.dev",
};

const members: NonNullable<ReturnType<typeof useTeamMembers>> = [
  { id: 1, name: "Nicolas Ettlin", email: "nicolas@acme.dev", role: "admin" },
];

// The component only checks whether a team has a subscription, so a partial
// response is enough to represent a team on a paid plan.
const paidSubscription = {
  plan: { name: "Starter", planType: "CONVEX_STARTER_PLUS" },
} as OrbSubscriptionResponse;

const meta = {
  component: TransferProject,
  decorators: [
    (Story) => (
      <div className="max-w-4xl">
        <Story />
      </div>
    ),
  ],
  beforeEach: () => {
    mocked(useCurrentProject).mockReturnValue(project);
    mocked(useTeams).mockReturnValue({
      selectedTeamSlug: originTeam.slug,
      teams: [originTeam, destinationTeam],
    });
    mocked(useCurrentTeam).mockReturnValue(originTeam);
    mocked(useProfile).mockReturnValue(profile);
    mocked(useTeamMembers).mockReturnValue(members);
    mocked(useHasCustomRolePermission).mockReturnValue(true);
    mocked(useBBMutation).mockReturnValue(fn());
    mocked(useTeamOrbSubscription).mockImplementation((teamId) => ({
      isLoading: false,
      hasError: false,
      subscription:
        teamId === undefined
          ? undefined
          : teamId === originTeam.id
            ? paidSubscription
            : null,
    }));
  },
} satisfies Meta<typeof TransferProject>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
