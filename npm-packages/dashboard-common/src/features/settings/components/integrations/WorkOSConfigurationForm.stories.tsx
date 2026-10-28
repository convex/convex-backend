import type { Meta, StoryObj } from "@storybook/nextjs";
import { useMemo, useState } from "react";
import { ConvexProvider } from "convex/react";
import { screen, userEvent, waitFor, within } from "storybook/test";
import udfs from "@common/udfs";
import { GenericId } from "convex/values";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import {
  ConnectedDeploymentContext,
  DeploymentInfo,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { WorkOSConfigurationForm } from "./WorkOSConfigurationForm";

type WorkOSOps = DeploymentInfo["workOSOperations"];
type Deployment = ReturnType<DeploymentInfo["useCurrentDeployment"]>;

const TEAM = { id: 2, name: "Acme", slug: "acme" };
const PROJECT = { id: 7, name: "App", slug: "app", teamId: TEAM.id };

const PROD_DEPLOYMENT = {
  id: 11,
  name: "happy-otter-123",
  deploymentType: "prod" as const,
  kind: "cloud" as const,
  isDefault: true,
  projectId: PROJECT.id,
  creator: 1,
  createTime: 0,
  class: "s256",
  deploymentUrl: "https://happy-otter-123.convex.cloud",
  reference: "production",
  region: "aws-us-east-1",
} satisfies Deployment;

const DEV_DEPLOYMENT = {
  ...PROD_DEPLOYMENT,
  id: 12,
  name: "happy-fox-456",
  deploymentType: "dev" as const,
  deploymentUrl: "https://happy-fox-456.convex.cloud",
  reference: "dev",
} satisfies Deployment;

const LOCAL_DEV_DEPLOYMENT = {
  ...DEV_DEPLOYMENT,
  id: 13,
  name: "local-dev",
  kind: "local" as const,
  previewIdentifier: null,
  port: 3210,
  isActive: true,
  deviceName: "MacBook",
} as unknown as Deployment;

const PREVIEW_DEPLOYMENT = {
  ...PROD_DEPLOYMENT,
  id: 14,
  name: "preview-pr-7",
  deploymentType: "preview" as const,
  previewIdentifier: "pr-7",
  reference: "preview/pr-7",
} satisfies Deployment;

const WORKOS_TEAM = {
  convexTeamId: TEAM.id,
  workosTeamId: "team_acme_123",
  workosTeamName: "acme-prod-team",
  workosAdminEmail: "admin@acme.com",
  creatorMemberId: 1,
};

const WORKOS_ENVIRONMENT = {
  deploymentName: PROD_DEPLOYMENT.name,
  workosEnvironmentId: "environment_prod_456",
  workosEnvironmentName: "happy-otter-123-prod",
  workosClientId: "client_prod_xyz",
  workosTeamId: WORKOS_TEAM.workosTeamId,
  isProduction: true,
};

const NON_PROD_WORKOS_ENVIRONMENT = {
  ...WORKOS_ENVIRONMENT,
  workosEnvironmentId: "environment_staging_456",
  workosEnvironmentName: "happy-otter-123-staging",
  workosClientId: "client_staging_xyz",
  isProduction: false,
};

function makeEnvVar(name: string, value: string) {
  return {
    _id: `k8envxx${name}` as GenericId<"_environment_variables">,
    _creationTime: 0,
    name,
    value,
  };
}

function buildClient(envVars: Array<{ name: string; value: string }>) {
  return mockConvexReactClient().registerQueryFake(
    udfs.listEnvironmentVariables.default,
    () => envVars.map(({ name, value }) => makeEnvVar(name, value)) as any,
  );
}

type Scenario = {
  deployment?: Deployment;
  workOSOperations?: Partial<WorkOSOps>;
  envVars?: Array<{ name: string; value: string }>;
};

function StoryShell({
  deployment = PROD_DEPLOYMENT,
  workOSOperations,
  envVars = [],
}: Scenario) {
  const client = useMemo(() => buildClient(envVars), [envVars]);

  const info: DeploymentInfo = {
    ...mockDeploymentInfo,
    useCurrentTeam: () => TEAM,
    useCurrentProject: () => PROJECT,
    useCurrentDeployment: () => deployment,
    deploymentsURI: `/t/${TEAM.slug}/${PROJECT.slug}/${deployment?.name}`,
    projectsURI: `/t/${TEAM.slug}/${PROJECT.slug}`,
    teamsURI: `/t/${TEAM.slug}`,
    workOSOperations: {
      ...mockDeploymentInfo.workOSOperations,
      ...workOSOperations,
    },
  };

  const connected = {
    deployment: {
      client,
      httpClient: {} as never,
      deploymentUrl: "https://example.convex.cloud",
      adminKey: "storybook-admin-key",
      deploymentName: deployment?.name ?? "deployment",
    },
    isDisconnected: false,
  };

  return (
    <ConnectedDeploymentContext.Provider value={connected}>
      <ConvexProvider client={client}>
        <DeploymentInfoContext.Provider value={info}>
          <div className="mx-auto max-w-xl rounded-md border bg-background-secondary p-4">
            <WorkOSConfigurationForm />
          </div>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  );
}

const meta = {
  component: WorkOSConfigurationForm,
  parameters: { layout: "padded", a11y: { test: "todo" } },
  render: (args) => <StoryShell {...(args as Scenario)} />,
} satisfies Meta<typeof StoryShell>;

export default meta;
type Story = StoryObj<Scenario>;

// 1. Loading state — workosData/workosEnvVars not yet returned.
export const Loading: Story = {
  args: {
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({ data: undefined }),
    },
  },
};

// 2. Failure to load the WorkOS configuration.
export const ErrorLoading: Story = {
  args: {
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: undefined,
        error: { code: "InternalError", message: "Failed to reach WorkOS" },
      }),
    },
  },
};

// 3. No team, no environment, no env vars — primary CTA to create a workspace.
export const NoWorkspace_CanBeCreated: Story = {
  args: {
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: { teamId: TEAM.id, environment: null, workosTeam: null },
      }),
      useAvailableWorkOSTeamEmails: () => ({
        availableEmails: ["nicolas@acme.com", "founder@acme.com"],
        usedEmails: [],
      }),
    },
  },
};

// 4. No team and every verified email is already used on another WorkOS team.
// The create button is disabled with a tip.
export const NoWorkspace_AllEmailsAlreadyUsed: Story = {
  args: {
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: { teamId: TEAM.id, environment: null, workosTeam: null },
      }),
      useAvailableWorkOSTeamEmails: () => ({
        availableEmails: [],
        usedEmails: ["nicolas@acme.com", "founder@acme.com"],
      }),
    },
  },
};

// 5. No team — user opens the create form, sees the used-emails caveat.
export const NoWorkspace_EmailSelectionOpen: Story = {
  args: {
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: { teamId: TEAM.id, environment: null, workosTeam: null },
      }),
      useAvailableWorkOSTeamEmails: () => ({
        availableEmails: ["nicolas@acme.com"],
        usedEmails: ["founder@acme.com", "ops@acme.com"],
      }),
    },
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", { name: /Create WorkOS Workspace/i }),
    );
  },
};

// 6. Newly created workspace — shows the "Congratulations!" callout (the
// state from the bug report screenshot). The Dismiss button should span the
// full width of the callout.
function CongratulationsShell() {
  const [hasTeam, setHasTeam] = useState(false);

  const ops: Partial<WorkOSOps> = {
    useDeploymentWorkOSEnvironment: () => ({
      data: {
        teamId: TEAM.id,
        environment: null,
        workosTeam: hasTeam ? WORKOS_TEAM : null,
      },
    }),
    useAvailableWorkOSTeamEmails: () => ({
      availableEmails: ["nicolas@acme.com"],
      usedEmails: [],
    }),
    useProvisionWorkOSTeam: () => async () => {
      setHasTeam(true);
      return {
        workosTeamId: WORKOS_TEAM.workosTeamId,
        workosTeamName: WORKOS_TEAM.workosTeamName,
        adminEmail: WORKOS_TEAM.workosAdminEmail,
      };
    },
    useWorkOSTeamHealth: () =>
      hasTeam
        ? {
            data: {
              teamProvisioned: true,
              teamInfo: {
                id: WORKOS_TEAM.workosTeamId,
                name: WORKOS_TEAM.workosTeamName,
                productionState: "active" as const,
              },
            },
          }
        : undefined,
  };

  return <StoryShell workOSOperations={ops} />;
}

export const TeamJustCreated_CongratulationsCallout: Story = {
  render: () => <CongratulationsShell />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", { name: /Create WorkOS Workspace/i }),
    );
    // The combobox renders with a single option that's the only available
    // one. It uses a portal, so we look it up via `screen`.
    const combobox = await canvas.findByRole("button", {
      name: /Admin email address/i,
    });
    await userEvent.click(combobox);
    const option = await screen.findByRole("option", {
      name: "nicolas@acme.com",
    });
    await userEvent.click(option);
    await userEvent.click(
      await canvas.findByRole("button", { name: /Create Workspace/i }),
    );
    await waitFor(() =>
      canvas.getByText(/Congratulations! Your WorkOS workspace/i),
    );
  },
};

// 7. Healthy workspace, no environment yet — shows the "Create AuthKit
// Environment" button.
export const Workspace_NoEnvironment: Story = {
  args: {
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: null,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useWorkOSInvitationEligibleEmails: () => ({
        eligibleEmails: ["nicolas@acme.com"],
        adminEmail: WORKOS_TEAM.workosAdminEmail,
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 8. Workspace exists, no environment, no payment method on WorkOS.
export const Workspace_NoPaymentMethod: Story = {
  args: {
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: null,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "inactive",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 9. Healthy workspace + healthy environment + env vars match.
export const Environment_AllConfigured: Story = {
  args: {
    envVars: [
      { name: "WORKOS_CLIENT_ID", value: WORKOS_ENVIRONMENT.workosClientId },
      {
        name: "WORKOS_ENVIRONMENT_ID",
        value: WORKOS_ENVIRONMENT.workosEnvironmentId,
      },
      { name: "WORKOS_API_KEY", value: "sk_live_workos_api_key_value" },
    ],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: {
            ...WORKOS_ENVIRONMENT,
          } as any,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSEnvironmentHealth: () => ({
        data: {
          id: WORKOS_ENVIRONMENT.workosEnvironmentId,
          name: WORKOS_ENVIRONMENT.workosEnvironmentName,
          clientId: WORKOS_ENVIRONMENT.workosClientId,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 10. Environment provisioned, but env vars in the deployment are missing.
export const Environment_EnvVarsMissing: Story = {
  args: {
    envVars: [],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: { ...WORKOS_ENVIRONMENT } as any,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSEnvironmentHealth: () => ({
        data: {
          id: WORKOS_ENVIRONMENT.workosEnvironmentId,
          name: WORKOS_ENVIRONMENT.workosEnvironmentName,
          clientId: WORKOS_ENVIRONMENT.workosClientId,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 11. Environment provisioned, but env vars point to a different environment.
export const Environment_EnvVarsMismatch: Story = {
  args: {
    envVars: [
      { name: "WORKOS_CLIENT_ID", value: "client_some_other_value" },
      { name: "WORKOS_ENVIRONMENT_ID", value: "environment_other" },
      { name: "WORKOS_API_KEY", value: "sk_other_key" },
    ],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: { ...WORKOS_ENVIRONMENT } as any,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSEnvironmentHealth: () => ({
        data: {
          id: WORKOS_ENVIRONMENT.workosEnvironmentId,
          name: WORKOS_ENVIRONMENT.workosEnvironmentName,
          clientId: WORKOS_ENVIRONMENT.workosClientId,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 12. Non-production environment — shows "Show Credentials" / "Delete" buttons.
export const NonProdEnvironment: Story = {
  args: {
    deployment: DEV_DEPLOYMENT,
    envVars: [
      {
        name: "WORKOS_CLIENT_ID",
        value: NON_PROD_WORKOS_ENVIRONMENT.workosClientId,
      },
      {
        name: "WORKOS_ENVIRONMENT_ID",
        value: NON_PROD_WORKOS_ENVIRONMENT.workosEnvironmentId,
      },
      { name: "WORKOS_API_KEY", value: "sk_dev_workos_api_key_value" },
    ],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: { ...NON_PROD_WORKOS_ENVIRONMENT } as any,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSEnvironmentHealth: () => ({
        data: {
          id: NON_PROD_WORKOS_ENVIRONMENT.workosEnvironmentId,
          name: NON_PROD_WORKOS_ENVIRONMENT.workosEnvironmentName,
          clientId: NON_PROD_WORKOS_ENVIRONMENT.workosClientId,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 13. Environment created against a different WorkOS team than the linked one.
export const Environment_TeamMismatch: Story = {
  args: {
    envVars: [
      { name: "WORKOS_CLIENT_ID", value: WORKOS_ENVIRONMENT.workosClientId },
    ],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: {
            ...WORKOS_ENVIRONMENT,
            workosTeamId: "team_orphan",
          } as any,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSEnvironmentHealth: () => ({
        data: {
          id: WORKOS_ENVIRONMENT.workosEnvironmentId,
          name: WORKOS_ENVIRONMENT.workosEnvironmentName,
          clientId: WORKOS_ENVIRONMENT.workosClientId,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 14. Environment was deleted in WorkOS.
export const Environment_NotFoundInWorkOS: Story = {
  args: {
    envVars: [
      { name: "WORKOS_CLIENT_ID", value: WORKOS_ENVIRONMENT.workosClientId },
    ],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: { ...WORKOS_ENVIRONMENT } as any,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSEnvironmentHealth: () => ({
        data: undefined,
        error: { code: "WorkOSEnvironmentNotFound" },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 15. WorkOS API is unavailable for both team and environment health checks.
export const WorkOSAPIUnavailable: Story = {
  args: {
    envVars: [
      { name: "WORKOS_CLIENT_ID", value: WORKOS_ENVIRONMENT.workosClientId },
      {
        name: "WORKOS_ENVIRONMENT_ID",
        value: WORKOS_ENVIRONMENT.workosEnvironmentId,
      },
      { name: "WORKOS_API_KEY", value: "sk_live_workos_api_key_value" },
    ],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: { ...WORKOS_ENVIRONMENT } as any,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSEnvironmentHealth: () => ({
        data: undefined,
        error: { code: "WorkOSAPIUnavailable" },
      }),
      useWorkOSTeamHealth: () => ({
        data: undefined,
        error: { code: "WorkOSAPIUnavailable" },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 16. The WorkOS team was deleted in WorkOS but still linked in Convex.
export const WorkspaceDeletedInWorkOS: Story = {
  args: {
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: null,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: undefined,
        error: { code: "WorkOSTeamDeleted" },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 17. The WorkOS team could not be found (the message shown in the screenshot).
export const WorkspaceNotFoundInWorkOS: Story = {
  args: {
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: null,
          workosTeam: {
            ...WORKOS_TEAM,
            workosTeamName: "repro-workos-team-delete",
          },
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: undefined,
        error: { code: "WorkOSTeamNotFound" },
      }),
      useWorkOSInvitationEligibleEmails: () => ({
        eligibleEmails: ["nicolas@acme.com"],
        adminEmail: WORKOS_TEAM.workosAdminEmail,
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 18. The user is using a shared project-level environment via env vars.
export const UsingSharedProjectEnvironment: Story = {
  args: {
    envVars: [
      { name: "WORKOS_CLIENT_ID", value: "client_shared_project" },
      { name: "WORKOS_API_KEY", value: "sk_shared_project_key" },
    ],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: null,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [
        {
          workosEnvironmentId: "environment_shared",
          workosEnvironmentName: "previews-shared",
          workosClientId: "client_shared_project",
          userEnvironmentName: "Previews",
          isProduction: false,
        },
      ],
    },
  },
};

// 19. WORKOS_CLIENT_ID is set but not managed by Convex.
export const ManuallyConfiguredClientId: Story = {
  args: {
    envVars: [
      { name: "WORKOS_CLIENT_ID", value: "client_manual_xyz" },
      { name: "WORKOS_API_KEY", value: "sk_manual_value" },
    ],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: null,
          workosTeam: null,
        },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 20. Local dev deployment — automatic creation is not supported.
export const LocalDevNotSupported: Story = {
  args: {
    deployment: LOCAL_DEV_DEPLOYMENT,
    envVars: [{ name: "WORKOS_CLIENT_ID", value: "client_manual_xyz" }],
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: null,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [],
    },
  },
};

// 21. Preview deployment — "Create AuthKit Environment" is a neutral CTA.
export const PreviewDeployment: Story = {
  args: {
    deployment: PREVIEW_DEPLOYMENT,
    workOSOperations: {
      useDeploymentWorkOSEnvironment: () => ({
        data: {
          teamId: TEAM.id,
          environment: null,
          workosTeam: WORKOS_TEAM,
        },
      }),
      useWorkOSTeamHealth: () => ({
        data: {
          teamProvisioned: true,
          teamInfo: {
            id: WORKOS_TEAM.workosTeamId,
            name: WORKOS_TEAM.workosTeamName,
            productionState: "active",
          },
        },
      }),
      useProjectWorkOSEnvironments: () => [
        {
          workosEnvironmentId: "environment_previews",
          workosEnvironmentName: "previews",
          workosClientId: "client_previews",
          userEnvironmentName: "Previews",
          isProduction: false,
        },
      ],
    },
  },
};

// 22. The "Create AuthKit Environment" form is open.
export const CreateEnvironmentForm: Story = {
  ...Workspace_NoEnvironment,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", {
        name: /Create AuthKit Environment/i,
      }),
    );
  },
};

// 23. The invite-team-member section is expanded.
export const InviteTeamMemberFormOpen: Story = {
  ...Workspace_NoEnvironment,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", { name: /Invite to WorkOS/i }),
    );
  },
};

// 24. The disconnect-workspace section is expanded.
export const DisconnectWorkspaceFormOpen: Story = {
  ...Workspace_NoEnvironment,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", { name: /Disconnect Workspace/i }),
    );
  },
};

// 25. Non-production env with the "delete environment" form expanded.
export const DeleteEnvironmentFormOpen: Story = {
  ...NonProdEnvironment,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", {
        name: /Delete Provisioned Environment/i,
      }),
    );
  },
};

// 26. Non-production env with the credentials revealed.
export const ShowCredentials: Story = {
  ...NonProdEnvironment,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", { name: /Show Credentials/i }),
    );
  },
};
