import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked } from "storybook/test";
import { TeamResponse } from "generatedApi";
import { useCurrentTeam } from "api/teams";
import { useCurrentProject } from "api/projects";
import { DeploymentLabel } from "./DeploymentDisplay";
import {
  PlatformDeploymentResponse,
  ProjectResponse,
} from "@convex-dev/platform/managementApi";

// Mock data
const mockTeam: TeamResponse = {
  id: 1,
  name: "Test Team",
  creator: 1,
  slug: "test-team",
  suspended: false,
  referralCode: "CODE123",
};

const mockProject: ProjectResponse = {
  id: 1,
  name: "Test Project",
  slug: "test-project",
  teamId: 1,
  teamSlug: "test-team",
  createTime: Date.now(),
};

function createCloudDeployment(
  overrides: Partial<Extract<PlatformDeploymentResponse, { kind: "cloud" }>> & {
    name: string;
    deploymentType: PlatformDeploymentResponse["deploymentType"];
  },
): PlatformDeploymentResponse {
  return {
    id: Math.floor(Math.random() * 1000),
    createTime: Date.now(),
    projectId: 1,
    kind: "cloud",
    class: "s16",
    region: "us-east-1",
    isDefault: true,
    deploymentUrl: `https://${overrides.name}.convex.cloud`,
    ...overrides,
  } as PlatformDeploymentResponse;
}

function createLocalDeployment(
  overrides: Partial<Extract<PlatformDeploymentResponse, { kind: "local" }>> & {
    name: string;
  },
): PlatformDeploymentResponse {
  return {
    id: Math.floor(Math.random() * 1000),
    createTime: Date.now(),
    projectId: 1,
    kind: "local",
    deploymentType: "dev",
    deviceName: "MacBook Pro",
    port: 3210,
    isActive: true,
    lastUpdateTime: Date.now(),
    creator: 1,
    isDefault: false,
    ...overrides,
  } as PlatformDeploymentResponse;
}

const prodDeployment = createCloudDeployment({
  name: "steady-capybara-123",
  deploymentType: "prod",
  isDefault: true,
});

const devDeployment = createCloudDeployment({
  name: "happy-koala-456",
  deploymentType: "dev",
  creator: 1,
  isDefault: false,
});

const localDevDeployment = createLocalDeployment({
  name: "local-nicolas_macbook_pro",
  deviceName: "MacBook Pro",
  port: 3210,
});

const previewDeployment = createCloudDeployment({
  name: "peaceful-dog-861",
  deploymentType: "preview",
  previewIdentifier: "feature/new-login",
  isDefault: false,
});

const customDeployment = createCloudDeployment({
  name: "quiet-husky-173",
  deploymentType: "custom",
  isDefault: false,
  reference: "staging",
});

const nonDefaultDevDeployment = createCloudDeployment({
  name: "swift-eagle-789",
  deploymentType: "dev",
  creator: 1,
  isDefault: false,
  reference: "dev/vercel",
});

const nonDefaultProdDeployment = createCloudDeployment({
  name: "calm-panda-321",
  deploymentType: "prod",
  isDefault: false,
  reference: "prod/shard-1",
});

const meta = {
  component: DeploymentLabel,
  render: (args) => <DeploymentLabel {...args} />,
  beforeEach: () => {
    mocked(useCurrentTeam).mockReturnValue(mockTeam);
    mocked(useCurrentProject).mockReturnValue(mockProject);
  },
  tags: ["autodocs"],
} satisfies Meta<typeof DeploymentLabel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Production: Story = {
  args: {
    deployment: prodDeployment,
    whoseName: null,
  },
};

export const ProductionWithVanityUrl: Story = {
  args: {
    deployment: prodDeployment,
    whoseName: null,
    vanityUrl: "api.myapp.com",
  },
};

export const DevelopmentCloud: Story = {
  args: {
    deployment: devDeployment,
    whoseName: null,
  },
};

export const DevelopmentCloudTeammate: Story = {
  args: {
    deployment: devDeployment,
    whoseName: "Jane Doe",
  },
};

export const DevelopmentLocal: Story = {
  args: {
    deployment: localDevDeployment,
    whoseName: null,
  },
};

export const Preview: Story = {
  args: {
    deployment: previewDeployment,
    whoseName: null,
  },
};

export const Custom: Story = {
  args: {
    deployment: customDeployment,
    whoseName: null,
  },
};

export const NonDefaultDev: Story = {
  args: {
    deployment: nonDefaultDevDeployment,
    whoseName: null,
  },
};

export const NonDefaultProd: Story = {
  args: {
    deployment: nonDefaultProdDeployment,
    whoseName: null,
  },
};
