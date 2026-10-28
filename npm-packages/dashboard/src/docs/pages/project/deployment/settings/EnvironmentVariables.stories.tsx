import { Meta, StoryObj } from "@storybook/nextjs";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import udfs from "@common/udfs";
import { ConvexProvider } from "convex/react";
import { fn } from "storybook/test";
import { EnvironmentVariablesView } from "@common/features/settings/components/EnvironmentVariablesView";
import { GenericId } from "convex/values";

// Fixed "now" so timestamps are stable.
const NOW = new Date("2026-03-10T14:25:00Z").getTime();

const mockTeam = {
  id: 2,
  slug: "acme",
  name: "Acme Corp",
};

const mockProject = {
  id: 7,
  teamId: mockTeam.id,
  name: "My amazing app",
  slug: "my-amazing-app",
};

const mockDeployment = {
  id: 11,
  name: "happy-capybara-123",
  deploymentType: "dev" as const,
  kind: "cloud",
  isDefault: true,
  projectId: mockProject.id,
  creator: 1,
  createTime: NOW,
  class: "s256",
  deploymentUrl: "https://happy-capybara-123.convex.cloud",
  reference: "dev/nicolas",
  region: "aws-us-east-1",
} as const;

type EnvVar = { name: string; value: string };

function makeClient(envVars: EnvVar[]) {
  return mockConvexReactClient()
    .registerQueryFake(udfs.deploymentState.deploymentState, () => ({
      _id: "" as any,
      _creationTime: 0,
      state: "running" as const,
    }))
    .registerQueryFake(udfs.components.list, () => [])
    .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
    .registerQueryFake(udfs.deploymentEvents.lastPushEvent, () => null)
    .registerQueryFake(
      udfs.convexCloudUrl.default,
      () => mockDeployment.deploymentUrl,
    )
    .registerQueryFake(
      udfs.convexSiteUrl.default,
      () => "https://happy-capybara-123.convex.site",
    )
    .registerQueryFake(udfs.fileStorageV2.numFiles, () => 0)
    .registerQueryFake(udfs.tableSize.sizeOfAllTables, () => 0)
    .registerQueryFake(udfs.listEnvironmentVariables.default, () =>
      envVars.map((v, i) => ({
        _id: `k8e${String(i).padStart(29, "0")}` as GenericId<"_environment_variables">,
        _creationTime: NOW,
        ...v,
      })),
    );
}

function renderWithEnvVars(envVars: EnvVar[]) {
  const client = makeClient(envVars);
  const connectedDeployment = {
    deployment: {
      client,
      httpClient: {} as never,
      deploymentUrl: mockDeployment.deploymentUrl,
      adminKey: "storybook-admin-key",
      deploymentName: mockDeployment.name,
    },
    isDisconnected: false,
  };
  return (
    <ConnectedDeploymentContext.Provider value={connectedDeployment}>
      <ConvexProvider client={client}>
        <DeploymentInfoContext.Provider
          value={{
            ...mockDeploymentInfo,
            useCurrentTeam: () => mockTeam,
            useCurrentProject: () => mockProject,
            useCurrentDeployment: () => mockDeployment,
            useIsDeploymentPaused: () => false,
            useLogDeploymentEvent: () => fn(),
            deploymentsURI: "/t/acme/my-amazing-app/happy-capybara-123",
            projectsURI: "/t/acme/my-amazing-app",
            teamsURI: "/t/acme",
            isSelfHosted: false,
          }}
        >
          <EnvironmentVariablesView />
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  );
}

const meta = {
  component: EnvironmentVariablesView,
  parameters: {
    layout: "fullscreen",
    nextjs: {
      router: {
        pathname:
          "/t/[team]/[project]/[deploymentName]/settings/environment-variables",
        route:
          "/t/[team]/[project]/[deploymentName]/settings/environment-variables",
        asPath:
          "/t/acme/my-amazing-app/happy-capybara-123/settings/environment-variables",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "happy-capybara-123",
        },
      },
    },
    a11y: { test: "todo" },
  },
  beforeEach: () => {
    const originalDateNow = Date.now;
    Date.now = () => NOW;

    return () => {
      Date.now = originalDateNow;
    };
  },
  render: (_args, { parameters }) => {
    const envVars = (parameters as { envVars?: EnvVar[] }).envVars ?? [];
    return renderWithEnvVars(envVars);
  },
} satisfies Meta<typeof EnvironmentVariablesView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  parameters: {
    envVars: [{ name: "MY_ENV_VAR", value: "value" }],
  },
};

export const Auth0: Story = {
  parameters: {
    envVars: [
      { name: "AUTH0_DOMAIN", value: "acme.us.auth0.com" },
      { name: "AUTH0_CLIENT_ID", value: "yourclientid" },
    ],
  },
};

export const Clerk: Story = {
  parameters: {
    envVars: [
      {
        name: "CLERK_JWT_ISSUER_DOMAIN",
        value: "https://clerk.acme.com",
      },
    ],
  },
};
