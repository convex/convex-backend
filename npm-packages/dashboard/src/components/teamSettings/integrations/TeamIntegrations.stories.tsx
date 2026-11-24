import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked, screen, userEvent, within } from "storybook/test";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import {
  useDisconnectWorkOSTeam,
  useTeamWorkOSIntegration,
  useWorkOSTeamHealth,
} from "api/workos";
import { TeamResponse } from "generatedApi";
import { flagDefaults, useLaunchDarkly } from "hooks/useLaunchDarkly";
import { TeamIntegrations } from "./TeamIntegrations";

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
  component: TeamIntegrations,
  args: { team },
  beforeEach: () => {
    mocked(useLaunchDarkly).mockReturnValue({
      ...flagDefaults,
      teamIntegrationsPage: true,
    });
    mocked(useIsCurrentMemberTeamAdmin).mockReturnValue(true);
    mocked(useHasCustomRolePermission).mockReturnValue(false);
    mocked(useTeamWorkOSIntegration).mockReturnValue({
      teamAssociation: {
        workosTeamId: "team_01JABCDEF",
        workosTeamName: "Acme",
        adminEmail: "ada@acme.dev",
        creatorEmail: "ada@acme.dev",
      },
      environments: [
        {
          deploymentName: "happy-animal-123",
          workosEnvironmentId: "environment_01",
          workosEnvironmentName: "happy-animal-123",
          workosClientId: "client_01",
        },
      ],
    });
    mocked(useWorkOSTeamHealth).mockReturnValue({
      data: {
        teamProvisioned: true,
        teamInfo: {
          id: "team_01JABCDEF",
          name: "Acme",
          productionState: "active",
        },
      },
      error: undefined,
    });
    mocked(useDisconnectWorkOSTeam).mockReturnValue(fn());
  },
} satisfies Meta<typeof TeamIntegrations>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Catalog: Story = {};

export const DeploymentIntegrationDetails: Story = {
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "How to set up Datadog" }),
    );
  },
};

export const WithoutWorkOSWorkspace: Story = {
  beforeEach: () => {
    mocked(useTeamWorkOSIntegration).mockReturnValue({
      teamAssociation: null,
      environments: [],
    });
  },
};

export const Developer: Story = {
  beforeEach: () => {
    mocked(useIsCurrentMemberTeamAdmin).mockReturnValue(false);
  },
};

export const Search: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.type(
      await within(canvasElement).findByRole("searchbox"),
      "logs",
    );
  },
};

export const SearchWithNoMatches: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.type(
      await within(canvasElement).findByRole("searchbox"),
      "kubernetes",
    );
  },
};
