import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked, screen, userEvent } from "storybook/test";
import {
  useAvailableWorkOSTeamEmails,
  useDisconnectWorkOSTeam,
  useProvisionWorkOSTeam,
  useTeamWorkOSIntegration,
  useWorkOSTeamHealth,
} from "api/workos";
import { useTeamMembers } from "api/teams";
import { WorkOsEnvironmentInfo } from "generatedApi";
import { WorkOSTeamIntegrationCard } from "./WorkOSTeamIntegrationCard";

const workspace = {
  workosTeamId: "team_01JABCDEF",
  workosTeamName: "Acme",
  adminEmail: "ada@acme.dev",
  creatorName: "Ada Lovelace",
  creatorEmail: "ada@acme.dev",
};

const environment = (
  deploymentName: string,
  workosEnvironmentName: string,
): WorkOsEnvironmentInfo => ({
  deploymentName,
  workosEnvironmentId: `environment_${deploymentName}`,
  workosEnvironmentName,
  workosClientId: `client_${deploymentName}`,
});

const environments = [
  environment("brave-falcon-891", "chat (dev/ada)"),
  environment("happy-animal-123", "chat (production)"),
  environment("quiet-eagle-345", "docs-site (preview/fix-search)"),
  ...[
    "fix-login",
    "new-onboarding",
    "pricing-page",
    "search-v2",
    "dark-mode",
  ].map((branch, i) =>
    environment(`preview-otter-${100 + i}`, `docs-site (preview/${branch})`),
  ),
];

const healthy = {
  data: {
    teamProvisioned: true,
    teamInfo: {
      id: workspace.workosTeamId,
      name: "Acme",
      productionState: "active" as const,
    },
  },
  error: undefined,
};

const openMenuItem = (name: string) => async () => {
  await userEvent.click(
    await screen.findByRole("button", { name: "WorkOS options" }),
  );
  await userEvent.click(await screen.findByRole("menuitem", { name }));
};

const meta = {
  component: WorkOSTeamIntegrationCard,
  args: { teamId: 1, canConnect: true, canDisconnect: true },
  decorators: [
    (Story) => (
      <div className="w-80">
        <Story />
      </div>
    ),
  ],
  beforeEach: () => {
    mocked(useTeamWorkOSIntegration).mockReturnValue({
      teamAssociation: workspace,
      environments,
    });
    mocked(useWorkOSTeamHealth).mockReturnValue(healthy);
    mocked(useDisconnectWorkOSTeam).mockReturnValue(fn());
    mocked(useProvisionWorkOSTeam).mockReturnValue(fn());
    mocked(useTeamMembers).mockReturnValue([
      { id: 1, name: "Ada Lovelace", email: "ada@acme.dev", role: "admin" },
    ]);
    mocked(useAvailableWorkOSTeamEmails).mockReturnValue({
      availableEmails: ["ada@acme.dev", "ada@lovelace.dev"],
      usedEmails: [],
    });
  },
} satisfies Meta<typeof WorkOSTeamIntegrationCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Connected: Story = {};

export const Menu: Story = {
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "WorkOS options" }),
    );
  },
};

export const Details: Story = { play: openMenuItem("View details") };

export const DetailsNeedsPaymentMethod: Story = {
  beforeEach: () => {
    mocked(useWorkOSTeamHealth).mockReturnValue({
      ...healthy,
      data: {
        ...healthy.data,
        teamInfo: { ...healthy.data.teamInfo, productionState: "inactive" },
      },
    });
  },
  play: openMenuItem("View details"),
};

export const WorkspaceDeleted: Story = {
  beforeEach: () => {
    mocked(useWorkOSTeamHealth).mockReturnValue({
      data: undefined,
      error: { code: "WorkOSTeamDeleted", message: "Deleted" },
    } as unknown as ReturnType<typeof useWorkOSTeamHealth>);
  },
};

export const ConfirmDisconnect: Story = { play: openMenuItem("Disconnect") };

export const NotConnected: Story = {
  beforeEach: () => {
    mocked(useTeamWorkOSIntegration).mockReturnValue({
      teamAssociation: null,
      environments: [],
    });
  },
};

export const InstallWorkOS: Story = {
  beforeEach: NotConnected.beforeEach,
  play: openMenuItem("Install WorkOS"),
};
